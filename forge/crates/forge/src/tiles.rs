//! Tilesets: sources made of many tiles, such as a DEM (`kind = "tileset"` in the lock). The
//! lock pins a per-source manifest (`tile<TAB>sha256<TAB>size` per line, or `absent` for a tile
//! the upstream does not have, such as open sea); the manifest pins every tile. `--pin` lists
//! the tiles of `query_params.bbox` with the `query_params.tile` name template and downloads
//! them. Tiles are stored by hash like any other source.
//!
//! The name template takes `{NS}`/`{EW}` (hemisphere letters), `{LAT2}` and `{LON3}`
//! (zero-padded absolute degrees of the tile's south-west corner).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::fetch::{get_to, hash_file, say};
use crate::lock::Source;

/// Parallel downloads: enough to fill a link, few enough to be polite to the host.
const PARALLEL: usize = 4;

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
        .replace("{LAT2}", &format!("{:02}", lat.unsigned_abs()))
        .replace("{LON3}", &format!("{:03}", lon.unsigned_abs()))
}

/// Tile names for every south-west corner in `lon_min,lat_min,lon_max,lat_max` (inclusive).
fn names_in(bbox: &str, template: &str) -> Result<Vec<String>, String> {
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
        .flat_map(|lat| (*x0..=*x1).map(move |lon| tile_name(template, lon, lat)))
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
    let names: Vec<String> = if pin {
        let p = |k: &str| {
            s.query_params
                .get(k)
                .ok_or_else(|| format!("{}: query_params.{k} is missing", s.id))
        };
        names_in(p("bbox")?, p("tile")?)?
    } else {
        known.keys().cloned().collect()
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(PARALLEL)
        .build()
        .map_err(|e| e.to_string())?;
    let tiles: Vec<Tile> = pool.install(|| {
        names
            .par_iter()
            .map(|name| fetch_tile(root, s, name, known.get(name), pin, verify))
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

fn fetch_tile(
    root: &Path,
    s: &Source,
    name: &str,
    known: Option<&Tile>,
    pin: bool,
    verify: bool,
) -> Result<Tile, String> {
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
    let url = s.url.replace("{tile}", name);
    let partial = root
        .join("data/cache/store/sha256")
        .join(format!(".partial-{name}"));
    let Some((hash, size)) = get_to(&url, &partial)? else {
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
    use super::{names_in, parse, tile_name};

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
        let m = parse("# c\na\t0f\t10\nb\tabsent\t0\n");
        assert_eq!(m.len(), 2);
        assert!(m.get(1).is_some_and(|t| t.sha256.is_none()));
    }
}
