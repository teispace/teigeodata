//! `forge embed`: the embedded baseline as shipped, one file (step 4.2).
//!
//! ```text
//! forge embed [--lite] [--key <seed-file>]
//! forge embed --profile <name> [--key <seed-file>]
//! ```
//!
//! Merges the world part (`forge world`) and every country's slice (`forge baseline <cc>`) of
//! the build into one pack, `data/build/baseline/baseline-<build>.tgd`, with
//! [`teigeodata_format::extract::merge`]: world first, then the slices by country code, so a
//! slice's rows replace the world's rows of the same ID (its country) and its outline replaces
//! the world's. One file and one engine layer, not a layer per slice: under 200 slice layers
//! `search` took 34× and `reverse` 98× the world part's time (`tests/world.rs`,
//! `the_cost_of_slices_as_layers`).
//!
//! With `--profile`, the layer is built from a consumer's profile instead
//! ([`super::embed_profile`], TG-12).
//!
//! Gates: Z1, the file is at most [`BUDGET`] bytes; every ID of every part is a row, with the
//! last part's point and ISO 3166 code (G-iso, D-197); the pack reads back verified with valid
//! geometry.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use teigeodata_core::{IsoCode, PlaceId};
use teigeodata_format::extract::{self, Part};
use teigeodata_format::idmap::IdMap;
use teigeodata_format::iso_codes::IsoCodes;
use teigeodata_format::places::Places;
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};

/// Z1: the shipped baseline's size budget in bytes (8 MB, D-144, Q-018).
pub(crate) const BUDGET: usize = 8_000_000;

struct Args {
    lite: bool,
    key: Option<String>,
    profile: Option<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        lite: false,
        key: None,
        profile: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--lite" => a.lite = true,
            "--key" => a.key = Some(it.next().ok_or("--key needs a seed file")?.clone()),
            "--profile" => {
                a.profile = Some(it.next().ok_or("--profile needs a name")?.clone());
            }
            other => return Err(format!("forge embed: unknown argument {other}")),
        }
    }
    Ok(a)
}

/// The parts of a build: the world part, then each country's slice by code.
fn parts(dir: &Path, build: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let world = dir.join(format!("world/world-baseline-{build}.tgd"));
    if !world.is_file() {
        return Err(format!(
            "{}: no world part (run `forge world` first)",
            world.display()
        ));
    }
    let mut out = vec![("world".to_owned(), world)];
    let mut slices = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let cc = entry.file_name().to_string_lossy().into_owned();
        let file = entry.path().join(format!("{cc}-baseline-{build}.tgd"));
        if cc != "world" && file.is_file() {
            slices.push((cc, file));
        }
    }
    slices.sort();
    out.extend(slices);
    Ok(out)
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let a = parse(args)?;
    if let Some(name) = &a.profile {
        return super::embed_profile::run(root, name, a.key.as_deref());
    }
    let build = if a.lite { "lite" } else { "main" };
    let dir = root.join("data/build/baseline");
    let files = parts(&dir, build)?;
    let bytes: Vec<Vec<u8>> = files
        .iter()
        .map(|(_, f)| fs::read(f).map_err(|e| format!("{}: {e}", f.display())))
        .collect::<Result<_, _>>()?;
    let e = |e: teigeodata_format::Error| e.to_string();
    let packs: Vec<Pack<'_>> = bytes
        .iter()
        .map(|b| Pack::open_verified(b).map_err(e))
        .collect::<Result<_, _>>()?;
    let keeps: Vec<Vec<u32>> = packs
        .iter()
        .map(|p| {
            let n = Places::open(&p.section(Kind::PLACES).ok_or("a part without PLACES")?)
                .map_err(e)?
                .len();
            u32::try_from(n)
                .map(|n| (0..n).collect())
                .map_err(|_| "too many rows".to_owned())
        })
        .collect::<Result<_, String>>()?;
    let list: Vec<Part<'_, '_>> = packs
        .iter()
        .zip(&keeps)
        .map(|(pack, keep)| Part {
            pack,
            keep,
            lod: 0,
            names: None,
        })
        .collect();
    let sections = extract::merge(&list, &super::baseline::region_options()).map_err(e)?;
    let out = write(&packs, &sections, a.key.as_deref(), root)?;
    super::baseline::check(&out, rows_of(&sections))?;
    let replaced = check_ids(&packs, &out)?;
    let file = dir.join(format!("baseline-{build}.tgd"));
    crate::make::write_atomic(&file, &out)?;
    let report = report(&files, &bytes, &sections, &out, replaced);
    fs::write(dir.join(format!("summary-{build}.md")), &report).map_err(|e| e.to_string())?;
    if out.len() > BUDGET {
        return Err(format!(
            "{report}\nZ1: the baseline is {} B, over its budget of {BUDGET} B",
            out.len()
        ));
    }
    Ok(format!(
        "{report}\n→ {} in {:.1?}",
        file.strip_prefix(root).unwrap_or(&file).display(),
        t.elapsed()
    ))
}

pub(super) fn rows_of(sections: &[extract::Body]) -> usize {
    sections
        .iter()
        .find(|s| s.0 == Kind::PLACES)
        .and_then(|s| usize::try_from(s.2).ok())
        .unwrap_or(0)
}

/// The world part's address, release and licence; every part's attribution, each line once.
pub(super) fn write(
    packs: &[Pack<'_>],
    sections: &[extract::Body],
    key: Option<&str>,
    root: &Path,
) -> Result<Vec<u8>, String> {
    let e = |e: teigeodata_format::Error| e.to_string();
    let world = packs.first().ok_or("no parts")?.manifest();
    let mut w = Writer::new(world.address(), "baseline", world.release()).map_err(e)?;
    let mut seen = BTreeSet::new();
    let mut notices = String::new();
    for p in packs {
        let text = p
            .manifest()
            .get(Tag::ATTRIBUTION)
            .map(|v| String::from_utf8_lossy(v).into_owned())
            .unwrap_or_default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if seen.insert(line.to_owned()) {
                notices.push_str(line);
                notices.push('\n');
            }
        }
    }
    w.manifest(Tag::ATTRIBUTION, notices.as_bytes())
        .map_err(e)?;
    if let Some(v) = world.get(Tag::LICENCE) {
        w.manifest(Tag::LICENCE, v).map_err(e)?;
    }
    let key = key
        .map(|p| super::pack::signing_key(&root.join(p)))
        .transpose()?;
    let signer = key.as_ref().map(teigeodata_format::signer);
    let bodies: Vec<(Kind, Encoding, u64, &[u8])> = sections
        .iter()
        .map(|s| (s.0, s.1, s.2, s.3.as_slice()))
        .collect();
    w.pack(
        &bodies,
        signer.as_ref().map(|s| s as &teigeodata_format::Sign<'_>),
    )
    .map_err(e)
}

/// A pack's IDs and places.
fn open<'a>(p: &Pack<'a>) -> Result<(IdMap<'a>, Places<'a>), String> {
    let e = |e: teigeodata_format::Error| format!("the baseline: {e}");
    Ok((
        IdMap::open(&p.section(Kind::IDMAP).ok_or("no IDMAP")?).map_err(e)?,
        Places::open(&p.section(Kind::PLACES).ok_or("no PLACES")?).map_err(e)?,
    ))
}

/// The ISO 3166 code of each row of `p`, by row (D-197).
fn codes(p: &Pack<'_>, rows: usize) -> Result<BTreeMap<u32, IsoCode>, String> {
    let e = |e: teigeodata_format::Error| format!("the baseline: {e}");
    let Some(s) = p.section(Kind::ISO_CODES) else {
        return Ok(BTreeMap::new());
    };
    let t = IsoCodes::open(&s, rows).map_err(e)?;
    t.check().map_err(e)?;
    Ok(t.iter().collect())
}

/// A row's point and ISO 3166 code.
type Want = (teigeodata_core::Point<teigeodata_core::E6>, Option<IsoCode>);

/// Every ID of every part is one row of the merged pack, at the point and with the ISO 3166
/// code of the last part that holds it. Returns how many rows a later part replaced.
pub(super) fn check_ids(packs: &[Pack<'_>], merged: &[u8]) -> Result<usize, String> {
    let e = |e: teigeodata_format::Error| format!("the baseline: {e}");
    let mut want: BTreeMap<PlaceId, Want> = BTreeMap::new();
    let mut total = 0;
    for p in packs {
        let (ids, places) = open(p)?;
        let coded = codes(p, places.len())?;
        for r in 0..ids.len() {
            let id = ids.id(r).ok_or("a row without an ID")?;
            let point = places.point(r).ok_or("a row without a point")?;
            let row = u32::try_from(r).map_err(|_| "too many rows")?;
            want.insert(id, (point, coded.get(&row).copied()));
            total += 1;
        }
    }
    let m = Pack::open(merged).map_err(e)?;
    let (ids, places) = open(&m)?;
    let coded = codes(&m, places.len())?;
    if ids.len() != want.len() {
        return Err(format!(
            "the baseline has {} rows for {} IDs",
            ids.len(),
            want.len()
        ));
    }
    for r in 0..ids.len() {
        let id = ids.id(r).ok_or("a row without an ID")?;
        let (point, code) = want.get(&id).copied().ok_or("a row of no part")?;
        if Some(point) != places.point(r) {
            return Err(format!("{id:?}: not the last part's row"));
        }
        let row = u32::try_from(r).map_err(|_| "too many rows")?;
        if coded.get(&row).copied() != code {
            return Err(format!("{id:?}: not the last part's ISO 3166 code (G-iso)"));
        }
    }
    Ok(total - want.len())
}

fn report(
    files: &[(String, PathBuf)],
    bytes: &[Vec<u8>],
    sections: &[extract::Body],
    out: &[u8],
    replaced: usize,
) -> String {
    let mut s = String::from("# The embedded baseline\n\n");
    let parts: Vec<String> = files
        .iter()
        .zip(bytes)
        .map(|((name, _), b)| format!("{name} ({} B)", b.len()))
        .collect();
    let _ = writeln!(
        s,
        "{} rows from {} parts: {}; {replaced} rows replaced by a later part. Budget (Z1): {} of {BUDGET} B.\n",
        rows_of(sections),
        files.len(),
        parts.join(", "),
        out.len()
    );
    s.push_str("| Section | Bytes |\n|---|---:|\n");
    for sec in sections {
        let _ = writeln!(s, "| {} | {} |", sec.0, sec.3.len());
    }
    let _ = writeln!(s, "| **pack** | **{}** |", out.len());
    let _ = writeln!(s, "\nBLAKE3 of the pack: `{}`.", blake3::hash(out).to_hex());
    s
}

#[cfg(test)]
mod tests {
    use super::check_ids;
    use teigeodata_core::{E6, IsoCode, Namespace, PlaceId, PlaceKind, Point, hilbert};
    use teigeodata_format::extract::{self, Part};
    use teigeodata_format::places::{self, PlaceColumns};
    use teigeodata_format::{Encoding, Kind, Pack, Writer, idmap, iso_codes};

    /// A pack of places at `points` (lat, lon on the 1e-6° grid) with serials `serials` and
    /// these codes by serial.
    fn pack(points: &[(i32, i32)], serials: &[u64], with: &[(u64, &str)]) -> Vec<u8> {
        let ns = Namespace::from_country(*b"np").unwrap();
        let mut rows: Vec<(Point<E6>, u64)> = points
            .iter()
            .zip(serials)
            .map(|(&(lat, lon), &s)| (Point::<E6>::from_units(lat, lon).unwrap(), s))
            .collect();
        rows.sort_by_key(|r| hilbert(r.0));
        let pts: Vec<Point<E6>> = rows.iter().map(|r| r.0).collect();
        let ids: Vec<PlaceId> = rows
            .iter()
            .map(|r| PlaceId::new(ns, r.1).unwrap())
            .collect();
        let kinds = vec![PlaceKind::Admin; pts.len()];
        let mut codes: Vec<(u32, IsoCode)> = with
            .iter()
            .map(|&(s, c)| {
                let r = rows.iter().position(|x| x.1 == s).unwrap();
                (u32::try_from(r).unwrap(), c.parse().unwrap())
            })
            .collect();
        codes.sort_unstable_by_key(|c| c.0);
        let n = pts.len() as u64;
        let mut bodies = vec![
            (Kind::IDMAP, n, idmap::encode(&ids).unwrap()),
            (
                Kind::PLACES,
                n,
                places::encode(&PlaceColumns {
                    points: &pts,
                    kinds: &kinds,
                    ..PlaceColumns::default()
                })
                .unwrap(),
            ),
        ];
        if let Some(b) = iso_codes::encode(&codes).unwrap() {
            bodies.push((Kind::ISO_CODES, codes.len() as u64, b));
        }
        let list: Vec<(Kind, Encoding, u64, &[u8])> = bodies
            .iter()
            .map(|b| (b.0, Encoding::TABLE, b.1, b.2.as_slice()))
            .collect();
        Writer::new("np", "places", "2026.11.0")
            .unwrap()
            .pack(&list, None)
            .unwrap()
    }

    /// The merge keeps every part's codes, the later part's for a replaced row; a merge that
    /// drops `ISO_CODES`, or keeps a replaced row's code, fails the check.
    #[test]
    fn the_baseline_keeps_every_parts_codes() {
        let world = pack(
            &[(10, 10), (20, 20), (30, 30)],
            &[1, 2, 3],
            &[(1, "NP-P1"), (2, "NP-P2")],
        );
        let slice = pack(&[(20, 21), (40, 40)], &[2, 4], &[(2, "NP-P3"), (4, "NP")]);
        let packs = [Pack::open(&world).unwrap(), Pack::open(&slice).unwrap()];
        let keeps = [vec![0, 1, 2], vec![0, 1]];
        let parts: Vec<Part<'_, '_>> = packs
            .iter()
            .zip(&keeps)
            .map(|(pack, keep)| Part {
                pack,
                keep,
                lod: 0,
                names: None,
            })
            .collect();
        let sections = extract::merge(&parts, &super::super::baseline::region_options()).unwrap();
        let write = |skip: Option<Kind>| {
            let list: Vec<(Kind, Encoding, u64, &[u8])> = sections
                .iter()
                .filter(|s| Some(s.0) != skip)
                .map(|s| (s.0, s.1, s.2, s.3.as_slice()))
                .collect();
            Writer::new("world", "baseline", "2026.11.0")
                .unwrap()
                .pack(&list, None)
                .unwrap()
        };
        let merged = write(None);
        assert_eq!(check_ids(&packs, &merged).unwrap(), 1);
        // Planted: the merge drops the section.
        let dropped = write(Some(Kind::ISO_CODES));
        let err = check_ids(&packs, &dropped).unwrap_err();
        assert!(err.contains("ISO 3166"), "{err}");
        // Planted: the world's code for the row the slice replaced.
        let stale = pack(
            &[(10, 10), (20, 21), (30, 30), (40, 40)],
            &[1, 2, 3, 4],
            &[(1, "NP-P1"), (2, "NP-P2"), (4, "NP")],
        );
        let err = check_ids(&packs, &stale).unwrap_err();
        assert!(err.contains("ISO 3166"), "{err}");
    }
}
