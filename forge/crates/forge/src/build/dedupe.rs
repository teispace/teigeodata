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

use std::collections::HashMap;

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
fn absorb(kept: &mut Place, dropped: Place) {
    for s in dropped.sources {
        if !kept.sources.contains(&s) {
            kept.sources.push(s);
        }
    }
    for n in dropped.names {
        if !kept.names.iter().any(|m| m.text == n.text) {
            kept.names.push(n);
        }
    }
    if kept.population.is_none() {
        kept.population = dropped.population;
    }
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

    use super::{Place, merge_near};
    use crate::build::PlaceName;

    /// 1e-7° of latitude is about 1.11 cm: 9,000 units ≈ 100 m.
    fn place(key: &str, name: &str, kind: PlaceKind, lat_e7: i32, sources: usize) -> Place {
        Place {
            key: key.into(),
            kind,
            admin_level: 0,
            flags: 0,
            point: (850_000_000, lat_e7),
            names: vec![PlaceName {
                text: name.into(),
                lang: None,
                kind: None,
            }],
            population: None,
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
