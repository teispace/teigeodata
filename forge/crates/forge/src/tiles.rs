//! Tilesets: sources made of many tiles, such as a DEM (`kind = "tileset"` in the lock). The
//! lock pins a per-source manifest (`tile<TAB>sha256<TAB>size` per line, or `absent` for a tile
//! the upstream does not have, such as open sea); the manifest pins every tile. `--pin` lists
//! the tiles of `query_params.bbox` with the `query_params.tile` name template and downloads
//! them. Tiles are stored by hash like any other source.
//!
//! The name template takes `{NS}`/`{EW}` (hemisphere letters; `{ns}`/`{ew}` in lower case),
//! `{LAT2}` and `{LON3}` (zero-padded absolute degrees of the tile's south-west corner).
//!
//! A tileset whose tiles are not on a degree grid (quadkeys, S2 cells) with irregular file
//! names sets `query_params.list` instead of `bbox` and `tile`: a checked-in file of
//! `tile<TAB>url` lines that `--pin` downloads. The list only says where to look; the manifest
//! pins the bytes, as for any tileset.
//!
//! A host that hands out short-lived anonymous read tokens (an Azure SAS) sets
//! `query_params.token_url`: one token is fetched per run (JSON `{"token": …}`) and appended as
//! the url's query. Tokens are never stored or printed; the pinned hashes prove the bytes.
//!
//! A GET tileset puts the name in its url (`{tile}`). A POST tileset, a server-side subsetting
//! API, has a `query` template instead: its body is rendered per tile with the lock's
//! `query_params` and the tile's corners `{{W}}`, `{{S}}`, `{{E}}`, `{{N}}`. Such a source may
//! set `query_params.parallel` (for example `1`, to be polite to a shared service).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::fetch::{get_to, hash_file, mirror, render, say};
use crate::lock::Source;
use crate::sliderule;

/// Parallel downloads: enough to fill a link, few enough to be polite to the host.
const PARALLEL: usize = 4;
/// Attempts at a tile whose response reports an error.
const ATTEMPTS: u32 = 3;

/// One manifest line.
#[derive(Clone, Debug)]
pub(crate) struct Tile {
    pub(crate) name: String,
    /// Lower-case hex, or `None` when the upstream has no such tile.
    pub(crate) sha256: Option<String>,
    pub(crate) size: u64,
}

/// The name of the tile holding (lon, lat) in whole degrees of its south-west corner.
pub(crate) fn tile_name(template: &str, lon: i32, lat: i32) -> String {
    template
        .replace("{NS}", if lat < 0 { "S" } else { "N" })
        .replace("{EW}", if lon < 0 { "W" } else { "E" })
        .replace("{ns}", if lat < 0 { "s" } else { "n" })
        .replace("{ew}", if lon < 0 { "w" } else { "e" })
        .replace("{LAT2}", &format!("{:02}", lat.unsigned_abs()))
        .replace("{LON3}", &format!("{:03}", lon.unsigned_abs()))
}

/// Tile names and south-west corners for every 1° tile in `lon_min,lat_min,lon_max,lat_max`
/// (corners inclusive).
fn names_in(bbox: &str, template: &str) -> Result<Vec<(String, i32, i32)>, String> {
    let v: Vec<i32> = bbox
        .split(',')
        .map(|x| x.trim().parse::<i32>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("bbox `{bbox}`: {e}"))?;
    let [x0, y0, x1, y1] = v.as_slice() else {
        return Err(format!(
            "bbox `{bbox}`: expected lon_min,lat_min,lon_max,lat_max"
        ));
    };
    Ok((*y0..=*y1)
        .flat_map(|lat| (*x0..=*x1).map(move |lon| (tile_name(template, lon, lat), lon, lat)))
        .collect())
}

/// The manifest of a tileset source, read and checked against the lock's pin.
pub(crate) fn manifest(root: &Path, s: &Source) -> Result<Vec<Tile>, String> {
    let rel = s
        .tiles
        .as_ref()
        .ok_or_else(|| format!("{}: a tileset needs a tiles manifest", s.id))?;
    let path = root.join(rel);
    let (hash, size) = hash_file(&path)?;
    if hash != s.sha256 || size != s.size {
        return Err(format!(
            "{}: {rel} does not match the lock (sha256 {hash}); fetch with --pin to re-pin deliberately",
            s.id
        ));
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
    Ok(parse(&text))
}

/// A tile list: `tile<TAB>https url` per line, `#` comments; names unique.
fn parse_list(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (i, l) in text.lines().enumerate() {
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        let (name, url) = l
            .split_once('\t')
            .ok_or_else(|| format!("line {}: expected tile<TAB>url", i + 1))?;
        let safe = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !safe || !url.starts_with("https://") {
            return Err(format!("line {}: bad tile name or url", i + 1));
        }
        if out.insert(name.to_string(), url.to_string()).is_some() {
            return Err(format!("line {}: tile {name} listed twice", i + 1));
        }
    }
    Ok(out)
}

fn parse(text: &str) -> Vec<Tile> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (name, sha, size) = (f.next()?, f.next()?, f.next()?.parse().ok()?);
            Some(Tile {
                name: name.to_string(),
                sha256: (sha != "absent").then(|| sha.to_string()),
                size,
            })
        })
        .collect()
}

/// Where a tile is stored.
pub(crate) fn tile_path(root: &Path, t: &Tile) -> Option<PathBuf> {
    t.sha256
        .as_ref()
        .map(|h| root.join("data/cache/store/sha256").join(h))
}

/// Fetches every tile. With `pin`, lists the tiles of the bbox, downloads what is missing and
/// writes the manifest, returning its new pin.
pub(crate) fn fetch(
    root: &Path,
    s: &Source,
    pin: bool,
    verify: bool,
) -> Result<Option<(String, u64)>, String> {
    let store = root.join("data/cache/store/sha256");
    fs::create_dir_all(&store).map_err(|e| e.to_string())?;
    let known: BTreeMap<String, Tile> = if pin {
        // Tiles already pinned and stored are not downloaded again.
        manifest(root, s)
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect()
    } else {
        manifest(root, s)?
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect()
    };
    let p = |k: &str| {
        s.query_params
            .get(k)
            .ok_or_else(|| format!("{}: query_params.{k} is missing", s.id))
    };
    let (mut names, urls) = match s.query_params.get("list") {
        Some(rel) => {
            let text = fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
            let urls = parse_list(&text).map_err(|e| format!("{rel}: {e}"))?;
            (urls.keys().map(|n| (n.clone(), 0, 0)).collect(), urls)
        }
        None => (names_in(p("bbox")?, p("tile")?)?, BTreeMap::new()),
    };
    if !pin {
        // A plain fetch restores the pinned tiles only.
        names.retain(|(n, _, _)| known.contains_key(n));
    }
    let parallel = s
        .query_params
        .get("parallel")
        .map_or(Ok(PARALLEL), |v| v.parse::<usize>())
        .map_err(|e| format!("{}: query_params.parallel: {e}", s.id))?;
    let missing = |n: &String| {
        known
            .get(n)
            .is_none_or(|t| t.sha256.is_some() && tile_path(root, t).is_none_or(|p| !p.is_file()))
    };
    // A token whenever a tile is to be downloaded: a mirror that lacks a tile falls back to
    // upstream.
    let token = match s.query_params.get("token_url") {
        Some(u) if pin || names.iter().any(|(n, _, _)| missing(n)) => Some(read_token(u)?),
        _ => None,
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(parallel.max(1))
        .build()
        .map_err(|e| e.to_string())?;
    let tiles: Vec<Tile> = pool.install(|| {
        names
            .par_iter()
            .map(|(name, lon, lat)| {
                let want = Want {
                    name,
                    url: urls.get(name).map(String::as_str),
                    lon: *lon,
                    lat: *lat,
                    token: token.as_deref(),
                };
                fetch_tile(root, s, &want, known.get(name), pin, verify)
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    let present = tiles.iter().filter(|t| t.sha256.is_some()).count();
    say(&format!(
        "{}: {present} tiles stored, {} absent upstream",
        s.id,
        tiles.len() - present
    ));
    if !pin {
        return Ok(None);
    }
    let rel = s
        .tiles
        .as_ref()
        .ok_or_else(|| format!("{}: a tileset needs a tiles manifest", s.id))?;
    let mut text = format!(
        "# Tiles of {} (forge fetch --pin). tile, sha256 (or absent), size.\n",
        s.id
    );
    for t in &tiles {
        let _ = writeln!(
            text,
            "{}\t{}\t{}",
            t.name,
            t.sha256.as_deref().unwrap_or("absent"),
            t.size
        );
    }
    let path = root.join(rel);
    fs::write(&path, &text).map_err(|e| format!("{rel}: {e}"))?;
    hash_file(&path).map(Some)
}

/// Downloads a tile and, when the source names a response format (`query_params.response`),
/// checks it: a response that reports a server error is fetched again, up to [`ATTEMPTS`]
/// times, and never stored.
fn download(
    s: &Source,
    name: &str,
    url: &str,
    body: Option<&str>,
    partial: &Path,
) -> Result<Option<(String, u64)>, String> {
    let check = s.query_params.get("response").map(String::as_str);
    let mut last = String::new();
    for attempt in 1..=ATTEMPTS {
        let Some(got) = get_to(url, body, partial)? else {
            return Ok(None);
        };
        let verdict = match check {
            None => Ok(()),
            Some("sliderule") => fs::read(partial)
                .map_err(|e| e.to_string())
                .and_then(|b| sliderule::read(&b))
                .and_then(|r| confirm_empty(s, url, &r.empty_beams)),
            Some(other) => Err(format!("unknown response format `{other}`")),
        };
        match verdict {
            Ok(()) => return Ok(Some(got)),
            Err(e) => last = e,
        }
        say(&format!("{}: tile {name}, attempt {attempt}: {last}", s.id));
        std::thread::sleep(std::time::Duration::from_secs(u64::from(attempt) * 10));
    }
    let _ = fs::remove_file(partial);
    Err(format!("{}: tile {name}: {last}", s.id))
}

/// Confirms with the server that every beam a `SlideRule` response could not read is absent
/// from its granule; an existing beam that failed is a transient error.
fn confirm_empty(s: &Source, url: &str, beams: &[(String, String)]) -> Result<(), String> {
    let service = url.split("/source/").next().unwrap_or(url);
    let asset = s
        .query_params
        .get("asset")
        .map_or("icesat2-atl08", String::as_str);
    for (granule, beam) in beams {
        if !sliderule::beam_absent(service, asset, granule, beam)? {
            return Err(format!("{granule} {beam} exists but failed to read"));
        }
        say(&format!(
            "{}: {granule} has no {beam}/land_segments (confirmed)",
            s.id
        ));
    }
    Ok(())
}

/// A tile to fetch: its name, south-west corner and the run's read token.
struct Want<'a> {
    name: &'a str,
    /// From `query_params.list`; else the url template with `{tile}`.
    url: Option<&'a str>,
    lon: i32,
    lat: i32,
    token: Option<&'a str>,
}

/// A short-lived anonymous read token from `url` (JSON `{"token": "…"}`).
fn read_token(url: &str) -> Result<String, String> {
    let mut resp = ureq::get(url)
        .header("User-Agent", crate::fetch::USER_AGENT)
        .call()
        .map_err(|e| format!("token {url}: {e}"))?;
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("token {url}: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("token {url}: {e}"))?;
    v.get("token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("token {url}: no `token` field"))
}

fn fetch_tile(
    root: &Path,
    s: &Source,
    want: &Want<'_>,
    known: Option<&Tile>,
    pin: bool,
    verify: bool,
) -> Result<Tile, String> {
    let name = want.name;
    if let Some(t) = known {
        match tile_path(root, t) {
            None => return Ok(t.clone()),
            Some(p) if p.is_file() => {
                if verify {
                    let (h, n) = hash_file(&p)?;
                    if Some(h.as_str()) != t.sha256.as_deref() || n != t.size {
                        return Err(format!(
                            "{}: tile {name} is corrupt; delete it and fetch again",
                            s.id
                        ));
                    }
                }
                return Ok(t.clone());
            }
            Some(_) => {}
        }
    }
    // A pinned tile comes from the mirror when there is one, as a plain GET, else upstream.
    let mirrored = known
        .and_then(|t| t.sha256.as_deref())
        .filter(|_| !pin)
        .and_then(|h| mirror(s, h));
    let base = want
        .url
        .map_or_else(|| s.url.replace("{tile}", name), str::to_string);
    let upstream = match want.token {
        None => base,
        Some(t) => format!("{base}?{t}"),
    };
    let body = || -> Result<Option<String>, String> {
        let Some(template) = &s.query else {
            return Ok(None);
        };
        let corners = [
            ("W", want.lon),
            ("S", want.lat),
            ("E", want.lon + 1),
            ("N", want.lat + 1),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let params = s
            .query_params
            .iter()
            .chain(corners.iter().map(|(k, v)| (k, v)));
        render(root, template, params).map(Some)
    };
    let partial = root
        .join("data/cache/store/sha256")
        .join(format!(".partial-{name}"));
    // The mirror first; a tile it does not hold yet comes from upstream, checked against the
    // same pin below.
    let from_mirror = match &mirrored {
        Some(m) => download(s, name, m, None, &partial)?,
        None => None,
    };
    let got = match from_mirror {
        Some(g) => Some(g),
        None => download(s, name, &upstream, body()?.as_deref(), &partial)?,
    };
    let Some((hash, size)) = got else {
        return Ok(Tile {
            name: name.to_string(),
            sha256: None,
            size: 0,
        });
    };
    if !pin && known.is_some_and(|t| t.sha256.as_deref() != Some(hash.as_str()) || t.size != size) {
        let _ = fs::remove_file(&partial);
        return Err(format!(
            "{}: tile {name} changed upstream (sha256 {hash}); investigate before re-pinning",
            s.id
        ));
    }
    let fin = root.join("data/cache/store/sha256").join(&hash);
    fs::rename(&partial, &fin).map_err(|e| format!("{}: {e}", fin.display()))?;
    Ok(Tile {
        name: name.to_string(),
        sha256: Some(hash),
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::{names_in, parse, parse_list, tile_name};

    #[test]
    fn names_and_manifests() {
        let t = "Copernicus_DSM_COG_10_{NS}{LAT2}_00_{EW}{LON3}_00_DEM";
        assert_eq!(
            tile_name(t, 85, 27),
            "Copernicus_DSM_COG_10_N27_00_E085_00_DEM"
        );
        assert_eq!(
            tile_name(t, -3, -1),
            "Copernicus_DSM_COG_10_S01_00_W003_00_DEM"
        );
        assert_eq!(names_in("80,26,88,30", t).map(|v| v.len()), Ok(45));
        assert_eq!(
            tile_name("NASADEM_HGT_{ns}{LAT2}{ew}{LON3}", 85, 27),
            "NASADEM_HGT_n27e085"
        );
        assert_eq!(tile_name("{ns}{LAT2}{ew}{LON3}", -71, -13), "s13w071");
        assert_eq!(
            names_in("85,27,86,27", "x{EW}{LON3}").ok(),
            Some(vec![
                ("xE085".to_string(), 85, 27),
                ("xE086".to_string(), 86, 27)
            ])
        );
        let l = parse_list("# c\n123\thttps://h/a.gz\n3995\thttps://h/b.gz\n").expect("list");
        assert_eq!(l.get("3995").map(String::as_str), Some("https://h/b.gz"));
        for bad in [
            "a b\thttps://h\n",
            "a\thttp://h\n",
            "../x\thttps://h\n",
            "a\thttps://h\na\thttps://i\n",
            "a\n",
        ] {
            assert!(parse_list(bad).is_err(), "{bad:?}");
        }
        let m = parse("# c\na\t0f\t10\nb\tabsent\t0\n");
        assert_eq!(m.len(), 2);
        assert!(m.get(1).is_some_and(|t| t.sha256.is_none()));
    }
}
