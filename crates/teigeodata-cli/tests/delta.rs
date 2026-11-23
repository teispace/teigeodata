//! `tgd delta` (D-391), the metamorphic gate: the older release with the delta above it answers
//! every query exactly as the newer release does, for every ID and at every probe; and a pair
//! that differs in what a delta cannot carry is refused.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::print_stdout,
    clippy::many_single_char_names,
    reason = "tests: failures panic with their message; rows and steps are small; the gate reports its sizes; short names for packs and paths"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{EAST, Model, NORTH, SOUTH, WEST};
use teigeodata_core::{E7, PlaceId, Point};
use teigeodata_engine::{Engine, Layer, LayerOptions, Outline, Scratch};
use teigeodata_format::region::Options;
use teigeodata_format::{Kind, Pack, Writer, extract};
use teigeodata_server::output::{self, Bufs, Item};

fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("delta-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn write(d: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = d.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// Runs `tgd delta old new -o out`: the delta's bytes, or the refusal on standard error.
fn delta(old: &Path, new: &Path, out: &Path) -> Result<Vec<u8>, String> {
    let o = Command::new(env!("CARGO_BIN_EXE_tgd"))
        .args(["delta", s(old), s(new), "-o", s(out)])
        .output()
        .unwrap();
    if o.status.success() {
        Ok(std::fs::read(out).unwrap())
    } else {
        assert_eq!(o.status.code(), Some(1), "a refusal exits 1");
        Err(String::from_utf8(o.stderr).unwrap())
    }
}

fn engine(packs: &[&[u8]]) -> Engine {
    let mut e = Engine::new();
    for p in packs {
        e.add(Layer::from_bytes(p.to_vec(), LayerOptions::default()).unwrap());
    }
    e
}

/// Everything the engine says of `id`: its record with every name, its class, its outline.
fn about(e: &Engine, id: PlaceId) -> Option<String> {
    let place = e.get(id).unwrap()?;
    let mut item = Item::from(place);
    item.all_names = true;
    let mut bufs = Bufs::default();
    let record =
        serde_json::to_string(&output::record(e, &item, &mut bufs, true).unwrap()).unwrap();
    let mut outline = Outline::default();
    let shape = e
        .boundary(&place, 0, &mut outline)
        .unwrap()
        .map(|_| format!("{:?}|{:?}", outline.points, outline.rings));
    Some(format!("{record} {:?} {shape:?}", e.class(&place).unwrap()))
}

/// The points of a grid over the model's country, `n` by `n`.
fn grid(n: i32) -> Vec<Point<E7>> {
    let mut out = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let lat = SOUTH + (NORTH - SOUTH) / n * i + (NORTH - SOUTH) / (2 * n);
            let lon = WEST + (EAST - WEST) / n * j + (EAST - WEST) / (2 * n);
            out.push(Point::<E7>::from_units(lat, lon).unwrap());
        }
    }
    out
}

/// The gate: `layered` (the older pack and the delta) answers as `newer` for each ID of
/// `ids`, at each point of `points`, and for each query of `queries`.
fn answers_alike(
    newer: &Engine,
    layered: &Engine,
    ids: &BTreeSet<PlaceId>,
    points: &[Point<E7>],
    queries: &[String],
) {
    for &id in ids {
        assert_eq!(about(newer, id), about(layered, id), "place {id}");
    }
    let (mut sa, mut sb) = (Scratch::default(), Scratch::default());
    let (mut na, mut nb) = (Vec::new(), Vec::new());
    let (mut ua, mut ub) = (Vec::new(), Vec::new());
    let near = |v: &[teigeodata_engine::Near]| {
        v.iter()
            .map(|n| (n.place.id, n.meters.to_bits()))
            .collect::<Vec<_>>()
    };
    let ids_of = |v: &[teigeodata_engine::Place]| v.iter().map(|p| p.id).collect::<Vec<_>>();
    for &at in points {
        let at6 = at.to_e6();
        newer.reverse(at6, 8, &mut sa, &mut na).unwrap();
        layered.reverse(at6, 8, &mut sb, &mut nb).unwrap();
        assert_eq!(near(&na), near(&nb), "reverse at {at:?}");
        newer.admin_at(at, &mut sa, &mut ua).unwrap();
        layered.admin_at(at, &mut sb, &mut ub).unwrap();
        assert_eq!(ids_of(&ua), ids_of(&ub), "admin at {at:?}");
        assert_eq!(
            newer.tz_at(at).unwrap().map(|p| p.id),
            layered.tz_at(at).unwrap().map(|p| p.id),
            "zone at {at:?}"
        );
        newer.countries_near(at, 50_000, &mut ua).unwrap();
        layered.countries_near(at, 50_000, &mut ub).unwrap();
        assert_eq!(ids_of(&ua), ids_of(&ub), "countries near {at:?}");
    }
    let (mut ha, mut hb) = (Vec::new(), Vec::new());
    for q in queries {
        newer.search(q, 10, &mut sa, &mut ha).unwrap();
        layered.search(q, 10, &mut sb, &mut hb).unwrap();
        let hits = |v: &[teigeodata_engine::Hit]| {
            v.iter().map(|h| (h.place.id, h.score)).collect::<Vec<_>>()
        };
        assert_eq!(hits(&ha), hits(&hb), "search {q:?}");
    }
}

fn ids_of(models: &[&Model]) -> BTreeSet<PlaceId> {
    models
        .iter()
        .flat_map(|m| m.ids.iter().copied().chain(m.redirects.iter().map(|r| r.0)))
        .collect()
}

#[test]
fn the_older_release_and_its_delta_answer_as_the_newer() {
    let d = dir("alike");
    let mut m = common::synthetic(600, 3);
    let linked = m.linked_rows();
    let free: Vec<u32> = (0..m.ids.len() as u32)
        .filter(|r| !linked.contains(r) && *r > 0)
        .collect();
    // The older release lacks five places the newer adds.
    let added: BTreeSet<u32> = free[20..25].iter().copied().collect();
    let keep_old: Vec<u32> = (0..m.ids.len() as u32)
        .filter(|r| !added.contains(r))
        .collect();
    let mut old = m.subset("np", &keep_old, |_| false);
    old.release = "2026.11.0".into();
    // The newer release: a place moves about 111 m, one is renamed, one gets a new height,
    // one a new importance; one is merged into another and one removed.
    let (moved, renamed, raised, ranked) = (free[0], free[1], free[2], free[3]);
    let p = m.points[moved as usize];
    m.points[moved as usize] =
        Point::from_degrees(p.lat_degrees() + 0.001, p.lon_degrees()).unwrap();
    m.names[renamed as usize][0].text = "Renamed Chowk".into();
    m.elevations[raised as usize] = Some(1_234);
    m.importance[ranked as usize] = m.importance[ranked as usize].wrapping_add(7);
    let (merged, survivor, removed) = (free[10], free[11], free[12]);
    let keep_new: Vec<u32> = (0..m.ids.len() as u32)
        .filter(|r| *r != merged && *r != removed)
        .collect();
    let gone = m.ids[removed as usize];
    let mut new = m.subset("np", &keep_new, |id| id == gone);
    new.redirects
        .push((m.ids[merged as usize], Some(m.ids[survivor as usize])));
    new.release = "2026.11.1".into();
    let (ob, nb) = (old.encode(), new.encode());
    let (op, np) = (write(&d, "old.tgd", &ob), write(&d, "new.tgd", &nb));
    let db = delta(&op, &np, &d.join("delta.tgd")).unwrap();
    let dp = Pack::open(&db).unwrap();
    assert_eq!(dp.manifest().delta_of(), Some("2026.11.0"));
    assert_eq!(dp.manifest().release(), "2026.11.1");
    // The renumbered timezone index is left to the older pack, as it means the same.
    assert!(dp.section(Kind::REGION).is_none());
    assert!(dp.section(Kind::REDIRECTS).is_some());
    println!("delta {} bytes for a pack of {} bytes", db.len(), nb.len());
    assert!(db.len() * 4 < nb.len(), "{} of {}", db.len(), nb.len());
    let newer = engine(&[&nb]);
    let layered = engine(&[&ob, &db]);
    let probes = common::probes(&nb);
    let mut queries = probes.queries.clone();
    queries.push("Renamed Chowk".into());
    let points: Vec<Point<E7>> = probes
        .points7
        .iter()
        .copied()
        .take(600)
        .chain(grid(12))
        .collect();
    answers_alike(&newer, &layered, &ids_of(&[&old, &new]), &points, &queries);
    // Without the delta the older pack answers otherwise: the gate can fail.
    assert_ne!(
        about(&newer, m.ids[renamed as usize]),
        about(&engine(&[&ob]), m.ids[renamed as usize])
    );
}

/// Model `m`'s pack with outlines and their admin index, re-encoded whole by `extract`.
fn outlined_pack(m: &Model) -> Vec<u8> {
    let bytes = m.encode();
    let pack = Pack::open(&bytes).unwrap();
    let all: Vec<u32> = (0..m.ids.len() as u32).collect();
    let bodies = extract::rows(&pack, &all, 0, &Options::default()).unwrap();
    let refs: Vec<(Kind, teigeodata_format::Encoding, u64, &[u8])> = bodies
        .iter()
        .map(|(k, e, n, b)| (*k, *e, *n, b.as_slice()))
        .collect();
    Writer::new(&m.address, &m.component, &m.release)
        .unwrap()
        .pack(&refs, None)
        .unwrap()
}

#[test]
fn a_changed_outline_answers_from_the_delta_and_the_rest_from_the_older() {
    let d = dir("outline");
    let (mut old, shapes) = common::outlined(false);
    old.release = "2026.11.0".into();
    // A district's square moves east by a fifth of its width; another district is renamed and
    // keeps its outline.
    let mut base = common::synthetic(300, 7);
    let (moved, renamed) = (shapes[1].0 as usize, shapes[2].0 as usize);
    let p = base.points[moved];
    base.points[moved] = Point::from_units(p.lat(), p.lon() + 200_000).unwrap();
    base.names[renamed][0].text = "Renamed District".into();
    let (mut new, _) = common::outlined_from(base, false);
    new.release = "2026.11.1".into();
    let (ob, nb) = (outlined_pack(&old), outlined_pack(&new));
    let (op, np) = (write(&d, "old.tgd", &ob), write(&d, "new.tgd", &nb));
    let db = delta(&op, &np, &d.join("delta.tgd")).unwrap();
    let dp = Pack::open(&db).unwrap();
    // One outline travels: the moved district's. The renamed one and the country stay below.
    let polygons =
        teigeodata_format::polygons::Polygons::open(&dp.section(Kind::POLYGONS).unwrap()).unwrap();
    assert_eq!(polygons.len(), 1);
    let newer = engine(&[&nb]);
    let layered = engine(&[&ob, &db]);
    answers_alike(
        &newer,
        &layered,
        &ids_of(&[&old, &new]),
        &grid(40),
        &["Renamed District".into()],
    );
    // The moved district's old square, east of its new one, is answered by the delta's outline:
    // the older pack alone still finds the district there.
    let (mut s, mut units) = (Scratch::default(), Vec::new());
    let [w, lo, _, hi] = shapes[1].1;
    let west_edge = Point::<E7>::from_units(lo + (hi - lo) / 2, w + 100_000).unwrap();
    let id = old.ids[moved];
    engine(&[&ob])
        .admin_at(west_edge, &mut s, &mut units)
        .unwrap();
    assert!(units.iter().any(|u| u.id == id));
    layered.admin_at(west_edge, &mut s, &mut units).unwrap();
    assert!(!units.iter().any(|u| u.id == id));
}

#[test]
fn what_a_delta_cannot_carry_is_refused() {
    let d = dir("refused");
    let mut m = common::synthetic(300, 3);
    m.release = "2026.11.0".into();
    let a = write(&d, "a.tgd", &m.encode());
    // The same release, and an older one, are not a delta's newer pack.
    let e = delta(&a, &a, &d.join("x.tgd")).unwrap_err();
    assert!(e.contains("not newer"), "{e}");
    // A substring index is keyed by row and not carried.
    m.release = "2026.11.1".into();
    m.with_substring();
    let b = write(&d, "b.tgd", &m.encode());
    let e = delta(&a, &b, &d.join("x.tgd")).unwrap_err();
    assert!(e.contains("SUBSTRING"), "{e}");
    assert!(!d.join("x.tgd").exists(), "a refusal writes nothing");
    // Another pack's release.
    let mut other = common::synthetic(300, 3);
    other.address = "in".into();
    other.release = "2026.11.2".into();
    let c = write(&d, "c.tgd", &other.encode());
    let e = delta(&a, &c, &d.join("x.tgd")).unwrap_err();
    assert!(e.contains("not releases of one pack"), "{e}");
    // A delta is made from whole packs, not from another delta.
    let mut n = common::synthetic(300, 3);
    n.release = "2026.11.1".into();
    n.names[5][0].text = "Changed".into();
    let nb = write(&d, "n.tgd", &n.encode());
    delta(&a, &nb, &d.join("d1.tgd")).unwrap();
    let e = delta(&a, &d.join("d1.tgd"), &d.join("x.tgd")).unwrap_err();
    assert!(e.contains("whole packs"), "{e}");
}

#[test]
fn the_newer_query_rules_travel_and_key_rules_are_refused() {
    use teigeodata_format::search::rules::{self, Rule};
    let d = dir("rules");
    let mut m = common::synthetic(300, 5);
    m.release = "2026.11.0".into();
    let ob = m.encode();
    // The newer release changes no place, only its query rules: a native generic with a Latin
    // spelling that ends one settlement's name, so a query in the native script finds it.
    let linked = m.linked_rows();
    let row = (1..m.ids.len())
        .find(|&r| {
            let t = &m.names[r][0].text;
            !linked.contains(&(r as u32))
                && !t.contains(' ')
                && m.names.iter().flatten().filter(|n| &n.text == t).count() == 1
        })
        .unwrap();
    let name = teigeodata_names::fold(&m.names[row][0].text).replace(' ', "");
    let cut = name.char_indices().rev().nth(1).unwrap().0;
    let (stem, tail) = name.split_at(cut);
    let query = format!("{stem}चोक");
    m.rules = Some(
        rules::encode(&[Rule::Native {
            native: "चोक",
            spelling: tail,
        }])
        .unwrap(),
    );
    m.release = "2026.11.1".into();
    let nb = m.encode();
    let (op, np) = (write(&d, "old.tgd", &ob), write(&d, "new.tgd", &nb));
    let db = delta(&op, &np, &d.join("delta.tgd")).unwrap();
    let newer = engine(&[&nb]);
    let layered = engine(&[&ob, &db]);
    let mut queries = common::probes(&nb).queries;
    queries.push(query.clone());
    answers_alike(&newer, &layered, &ids_of(&[&m]), &grid(8), &queries);
    // The rule finds the place, and the older pack alone does not: the gate can fail.
    let (mut sc, mut hits) = (Scratch::default(), Vec::new());
    let found = |e: &Engine, sc: &mut Scratch, hits: &mut Vec<teigeodata_engine::Hit>| {
        e.search(&query, 10, sc, hits).unwrap();
        hits.iter().any(|h| h.place.id == m.ids[row])
    };
    assert!(found(&layered, &mut sc, &mut hits), "{query}");
    assert!(!found(&engine(&[&ob]), &mut sc, &mut hits), "{query}");
    // A droppable suffix shapes the keys the older pack stored: refused.
    m.rules = Some(rules::encode(&[Rule::Drop("chowk")]).unwrap());
    m.release = "2026.11.2".into();
    let kp = write(&d, "keys.tgd", &m.encode());
    let e = delta(&op, &kp, &d.join("x.tgd")).unwrap_err();
    assert!(e.contains("key rules"), "{e}");
}

/// The gate on real packs: `TEIGEODATA_DELTA=old,new,delta[,nosearch]` (paths; `nosearch` to
/// leave search out). Every ID, 4,096 points near places and a grid over the packs' box; the
/// probe queries unless `nosearch`.
#[test]
#[ignore = "needs real packs: TEIGEODATA_DELTA=old,new,delta"]
fn real_packs_answer_alike() {
    let spec = std::env::var("TEIGEODATA_DELTA").unwrap();
    let parts: Vec<&str> = spec.split(',').collect();
    let read = |p: &str| std::fs::read(p).unwrap();
    let (ob, nb, db) = (read(parts[0]), read(parts[1]), read(parts[2]));
    let search = parts.get(3) != Some(&"nosearch");
    let newer = engine(&[&nb]);
    let layered = engine(&[&ob, &db]);
    let ids: BTreeSet<PlaceId> = ids_of(&[&Model::from_pack(&ob), &Model::from_pack(&nb)]);
    let probes = common::probes(&nb);
    let queries = if search {
        probes.queries.clone()
    } else {
        Vec::new()
    };
    let started = std::time::Instant::now();
    answers_alike(&newer, &layered, &ids, &probes.points7, &queries);
    println!(
        "{} IDs, {} points, {} queries alike in {:.1} s",
        ids.len(),
        probes.points7.len(),
        queries.len(),
        started.elapsed().as_secs_f64()
    );
}
