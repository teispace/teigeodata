//! Places that are one place (D-116): two settlements or localities whose canonical names fold
//! to the same key and whose final points are within [`SAME_M`] are merged. Conflation keeps
//! them apart when they come from one source (a source's records never merge) or when their
//! seeds are far apart; the settlement core then moves both onto the same buildings.
//!
//! Same-name places nearby are usually different places: Nepal has many neighbouring hamlets
//! called Dandagaun or Laksmipur. Of 5,176 same-name pairs within 2 km whose points both come
//! from a building core, 15 are within 100 m after the core and none share a point; the rest
//! sit on their own buildings. So the rule is the final point, not the name alone.
//!
//! The kept place is the one with more source records, then more names, then the smaller key;
//! it takes the other's records (so the other's IDs resolve to it), its names it lacks, and its
//! population when it has none. Groups are compared by a sweep in latitude, so a common name
//! costs its own sort, not all pairs.

use std::collections::{BTreeSet, HashMap};

use teigeodata_core::PlaceKind;
use teigeodata_names::fold;

use super::Place;
use crate::links::haversine_e7;

/// The farthest apart two same-name places may be and still be merged.
pub(super) const SAME_M: f64 = 100.0;

/// `SAME_M` in 1e-7° of latitude, rounded up: the sweep window.
const WINDOW_E7: i32 = 9_000;

/// Merges same-name settlements and localities within [`SAME_M`]; returns the merged pairs as
/// (kept key, dropped key, metres), sorted.
pub(super) fn merge_near(places: &mut Vec<Place>) -> Vec<(String, String, f64)> {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in places.iter().enumerate() {
        if matches!(p.kind, PlaceKind::Settlement | PlaceKind::Locality)
            && let Some(n) = p.names.first()
        {
            let k = fold(&n.text);
            if !k.is_empty() {
                groups.entry(k).or_default().push(i);
            }
        }
    }
    // Union-find over the places, so a chain of three merges into one.
    let mut root: Vec<usize> = (0..places.len()).collect();
    let find = |root: &mut Vec<usize>, mut i: usize| {
        while let Some(&r) = root.get(i) {
            if r == i {
                break;
            }
            let up = root.get(r).copied().unwrap_or(r);
            if let Some(slot) = root.get_mut(i) {
                *slot = up;
            }
            i = r;
        }
        i
    };
    let mut merged: Vec<(String, String, f64)> = Vec::new();
    for members in groups.values_mut() {
        if members.len() < 2 {
            continue;
        }
        members.sort_by(|&a, &b| {
            let k = |i: usize| places.get(i).map(|p| (p.point.1, p.key.as_str()));
            k(a).cmp(&k(b))
        });
        for (a, &i) in members.iter().enumerate() {
            let Some(p) = places.get(i) else { continue };
            for &j in members.iter().skip(a + 1) {
                let Some(q) = places.get(j) else { continue };
                if q.point.1 - p.point.1 > WINDOW_E7 {
                    break;
                }
                let d = haversine_e7(p.point, q.point);
                if d > SAME_M {
                    continue;
                }
                let (ri, rj) = (find(&mut root, i), find(&mut root, j));
                if ri == rj {
                    continue;
                }
                let (keep, drop) = if better(places, ri, rj) {
                    (ri, rj)
                } else {
                    (rj, ri)
                };
                if let Some(slot) = root.get_mut(drop) {
                    *slot = keep;
                }
                let key = |i: usize| places.get(i).map(|p| p.key.clone()).unwrap_or_default();
                merged.push((key(keep), key(drop), d));
            }
        }
    }
    if merged.is_empty() {
        return merged;
    }
    // Fold every dropped place into its root, then remove the dropped ones.
    let roots: Vec<usize> = (0..places.len()).map(|i| find(&mut root, i)).collect();
    let mut taken: Vec<Option<Place>> = places.drain(..).map(Some).collect();
    for (i, &r) in roots.iter().enumerate() {
        if i == r {
            continue;
        }
        let Some(dropped) = taken.get_mut(i).and_then(Option::take) else {
            continue;
        };
        if let Some(Some(kept)) = taken.get_mut(r) {
            absorb(kept, dropped);
        }
    }
    *places = taken.into_iter().flatten().collect();
    merged.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    merged
}

/// How far a `GeoNames` locality may lie from the landmark it repeats (D-381): of Nepal's 55
/// same-name pairs of an unpopulated `P.PPLL` and a landmark, all are within 929 m, and the next
/// are 2 km and more apart, other places (measured 2026-11-22).
pub(super) const SPOT_M: f64 = 1_000.0;

/// `SPOT_M` in 1e-7° of latitude, rounded up.
const SPOT_WINDOW_E7: i32 = 90_000;

/// Joins each spot (a place held only by unpopulated `GeoNames` localities, keyed in `spots`)
/// to the nearest landmark of the same folded name within [`SPOT_M`]: the landmark keeps its
/// class and takes the spot's records, so the spot's ID resolves to it. Landmarks never join
/// each other here, so two temples of one name stay two. Returns (landmark key, spot key,
/// metres), sorted.
pub(super) fn join_spots(
    places: &mut Vec<Place>,
    spots: &BTreeSet<String>,
) -> Vec<(String, String, f64)> {
    let key_of = |p: &Place| {
        p.names
            .first()
            .map(|n| fold(&n.text))
            .filter(|k| !k.is_empty())
    };
    let mut landmarks: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in places.iter().enumerate() {
        if p.class != 0
            && let Some(k) = key_of(p)
        {
            landmarks.entry(k).or_default().push(i);
        }
    }
    if landmarks.is_empty() {
        return Vec::new();
    }
    // (spot, landmark, metres) for each spot with a landmark in reach.
    let mut pairs: Vec<(usize, usize, f64)> = Vec::new();
    for (i, p) in places.iter().enumerate() {
        if p.class != 0 || !spots.contains(&p.key) {
            continue;
        }
        let Some(near) = key_of(p).and_then(|k| landmarks.get(&k)) else {
            continue;
        };
        let best = near
            .iter()
            .filter_map(|&j| {
                let q = places.get(j)?;
                ((q.point.1 - p.point.1).abs() <= SPOT_WINDOW_E7)
                    .then(|| (j, haversine_e7(p.point, q.point)))
            })
            .filter(|&(_, d)| d <= SPOT_M)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((j, d)) = best {
            pairs.push((i, j, d));
        }
    }
    if pairs.is_empty() {
        return Vec::new();
    }
    let key = |i: usize| places.get(i).map(|p| p.key.clone()).unwrap_or_default();
    let mut joined: Vec<(String, String, f64)> =
        pairs.iter().map(|&(i, j, d)| (key(j), key(i), d)).collect();
    let mut taken: Vec<Option<Place>> = places.drain(..).map(Some).collect();
    for &(i, j, _) in &pairs {
        let Some(spot) = taken.get_mut(i).and_then(Option::take) else {
            continue;
        };
        if let Some(Some(landmark)) = taken.get_mut(j) {
            absorb(landmark, spot);
        }
    }
    *places = taken.into_iter().flatten().collect();
    joined.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    joined
}

/// Whether place `a` is kept over place `b`.
fn better(places: &[Place], a: usize, b: usize) -> bool {
    let rank = |i: usize| {
        places.get(i).map(|p| {
            (
                p.sources.len(),
                p.names.len(),
                std::cmp::Reverse(p.key.clone()),
            )
        })
    };
    rank(a) >= rank(b)
}

/// Gives `kept` the records, missing names and population of `dropped`.
pub(super) fn absorb(kept: &mut Place, dropped: Place) {
    for s in dropped.sources {
        if !kept.sources.contains(&s) {
            kept.sources.push(s);
        }
    }
    for n in dropped.names {
        if !kept.names.iter().any(|m| m.same(&n)) {
            kept.names.push(n);
        }
    }
    if kept.population.is_none() {
        kept.population = dropped.population;
    }
    kept.rank = kept.rank.max(dropped.rank);
}

/// Writes `merged-<build>.tsv` beside the pack and returns the summary line.
pub(super) fn report(
    merged: &[(String, String, f64)],
    out_dir: &std::path::Path,
    build: &str,
) -> Result<String, String> {
    use std::fmt::Write as _;
    let mut tsv = String::from(
        "# Same-name places within 100 m merged into one (D-116).\nkept\tmerged\tmetres\n",
    );
    for (k, d, m) in merged {
        let _ = writeln!(tsv, "{k}\t{d}\t{m:.0}");
    }
    let file = out_dir.join(format!("merged-{build}.tsv"));
    std::fs::write(&file, tsv).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(format!(
        "Same place (D-116): {} same-name settlements and localities within {SAME_M} m merged into another, listed in `merged-{build}.tsv`.\n",
        merged.len()
    ))
}

#[cfg(test)]
mod tests {
    use teigeodata_core::PlaceKind;

    use std::collections::BTreeSet;

    use super::{Place, join_spots, merge_near};
    use crate::build::PlaceName;

    /// 1e-7° of latitude is about 1.11 cm: 9,000 units ≈ 100 m.
    fn place(key: &str, name: &str, kind: PlaceKind, lat_e7: i32, sources: usize) -> Place {
        Place {
            key: key.into(),
            kind,
            admin_level: 0,
            flags: 0,
            class: 0,
            point: (850_000_000, lat_e7),
            names: vec![PlaceName {
                text: name.into(),
                lang: None,
                kind: None,
                origin: crate::build::Origin::Unknown,
            }],
            population: None,
            rank: 0,
            parent: None,
            sources: (0..sources).map(|i| format!("{key}/{i}")).collect(),
        }
    }

    fn keys(places: &[Place]) -> Vec<&str> {
        let mut k: Vec<&str> = places.iter().map(|p| p.key.as_str()).collect();
        k.sort_unstable();
        k
    }

    #[test]
    fn near_same_name_places_merge_into_the_better_one() {
        let s = PlaceKind::Settlement;
        let mut places = vec![
            place("a", "Chhorepatan", s, 280_000_000, 1),
            place("b", "Chhorepatan", s, 280_004_500, 2),
        ];
        let merged = merge_near(&mut places);
        assert_eq!(keys(&places), ["b"]);
        assert_eq!(merged.len(), 1);
        assert_eq!((merged[0].0.as_str(), merged[0].1.as_str()), ("b", "a"));
        assert!(merged[0].2 > 40.0 && merged[0].2 < 60.0, "{}", merged[0].2);
        // The dropped place's records now resolve to the kept one.
        assert!(places[0].sources.contains(&"a/0".to_string()));
    }

    #[test]
    fn far_different_or_admin_places_stay() {
        let s = PlaceKind::Settlement;
        let mut places = vec![
            place("a", "Laksmipur", s, 280_000_000, 1),
            place("b", "Laksmipur", s, 280_014_000, 1), // about 156 m
            place("c", "Sitapur", s, 280_001_000, 1),
            place("d", "Laksmipur", PlaceKind::Admin, 280_000_500, 1),
            // Same latitude, about 150 m east: inside the sweep window, outside the distance.
            Place {
                point: (850_015_300, 280_000_000),
                ..place("e", "Laksmipur", s, 0, 1)
            },
        ];
        assert_eq!(merge_near(&mut places), Vec::new());
        assert_eq!(places.len(), 5);
    }

    #[test]
    fn a_spot_joins_the_nearest_landmark_of_its_name() {
        let (s, l) = (PlaceKind::Settlement, PlaceKind::Locality);
        let temple = |key: &str, lat_e7: i32| Place {
            class: 2,
            ..place(key, "Kālikā Temple", l, lat_e7, 1)
        };
        let mut places = vec![
            temple("t1", 280_000_000),
            temple("t2", 280_040_000), // about 445 m north of t1
            place("s1", "Kalika Temple", s, 280_036_000, 1), // 44 m from t2, 400 m from t1
            place("s2", "Kalika Temple", s, 280_200_000, 1), // 1.8 km from t2: stays
            place("v", "Kalika Temple", s, 280_001_000, 1), // not a spot: stays
        ];
        let spots: BTreeSet<String> = ["s1", "s2"].map(String::from).into();
        let joined = join_spots(&mut places, &spots);
        assert_eq!(keys(&places), ["s2", "t1", "t2", "v"]);
        assert_eq!(joined.len(), 1);
        assert_eq!((joined[0].0.as_str(), joined[0].1.as_str()), ("t2", "s1"));
        let t2 = places.iter().find(|p| p.key == "t2").unwrap();
        assert_eq!(t2.class, 2);
        assert!(t2.sources.contains(&"s1/0".to_string()));
        // Without a landmark, nothing joins.
        let mut plain = vec![place("s1", "Kalika Temple", s, 280_000_000, 1)];
        assert_eq!(join_spots(&mut plain, &spots), Vec::new());
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn a_chain_becomes_one_place_and_folding_matches() {
        let l = PlaceKind::Locality;
        let mut places = vec![
            place("a", "Dāndāgāun", l, 280_000_000, 1),
            place("b", "Dandagaun", l, 280_008_000, 3),
            place("c", "dandagaun", l, 280_016_000, 1),
        ];
        let merged = merge_near(&mut places);
        assert_eq!(keys(&places), ["b"]);
        assert_eq!(merged.len(), 2);
        assert_eq!(places[0].sources.len(), 5);
    }
}
