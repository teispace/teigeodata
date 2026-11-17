//! Admin areas of one `admin_level` from an OSM PBF extract, with their geometry (D-363).
//!
//! Three passes over the extract, each keeping only what the next needs: the relations of the
//! level (their tags and member ways), then those ways (their node IDs), then those nodes
//! (their positions on OSM's 1e-7° grid, the forge's grid). Memory holds the boundary
//! geometry, never the extract's node table, so a country or a continent reads alike.
//!
//! A relation's member ways (role `outer`, `inner` or none) are joined end to end into closed
//! rings by node ID, and the rings are read even–odd: a hole is a ring inside another, whatever
//! its role says. A relation whose ways do not close into rings, or that names a way or node
//! the extract lacks, is reported and left out.

use std::collections::HashMap;
use std::path::Path;

use osmpbf::{Element, ElementReader, RelMemberType};

/// One area: the relation's ID, its tags and its rings as `(lon, lat)` on the 1e-7° grid,
/// each closed (first vertex repeated last).
#[derive(Debug)]
pub(crate) struct Area {
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) rings: Vec<Vec<(i32, i32)>>,
}

impl Area {
    /// The value of tag `k`.
    pub(crate) fn tag(&self, k: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    }
}

/// What [`admin_areas`] read.
#[derive(Debug, Default)]
pub(crate) struct Areas {
    /// By relation ID.
    pub(crate) areas: Vec<Area>,
    /// Relations left out, with why.
    pub(crate) broken: Vec<(i64, String)>,
}

/// A relation of the level: its tags and its member ways.
struct Rel {
    id: i64,
    tags: Vec<(String, String)>,
    ways: Vec<i64>,
}

/// The `boundary=administrative` relations with `admin_level=<level>` in the extract at
/// `path`, as areas.
pub(crate) fn admin_areas(path: &Path, level: &str) -> Result<Areas, String> {
    let err = |e: osmpbf::Error| format!("{}: {e}", path.display());
    let reader = || ElementReader::from_path(path).map_err(err);

    // 1. The relations and their member ways.
    let mut rels: Vec<Rel> = reader()?
        .par_map_reduce(
            |el| match el {
                Element::Relation(r)
                    if r.tags().any(|t| t == ("boundary", "administrative"))
                        && r.tags().any(|t| t == ("admin_level", level)) =>
                {
                    let ways = r
                        .members()
                        .filter(|m| m.member_type == RelMemberType::Way)
                        .filter(|m| matches!(m.role(), Ok("outer" | "inner" | "")))
                        .map(|m| m.member_id)
                        .collect();
                    vec![Rel {
                        id: r.id(),
                        tags: r
                            .tags()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                        ways,
                    }]
                }
                _ => Vec::new(),
            },
            Vec::new,
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        )
        .map_err(err)?;
    rels.sort_by_key(|r| r.id);

    // 2. Those ways' node IDs.
    let mut want_ways: Vec<i64> = rels.iter().flat_map(|r| r.ways.iter().copied()).collect();
    want_ways.sort_unstable();
    want_ways.dedup();
    let ways: HashMap<i64, Vec<i64>> = reader()?
        .par_map_reduce(
            |el| match el {
                Element::Way(w) if want_ways.binary_search(&w.id()).is_ok() => {
                    vec![(w.id(), w.refs().collect::<Vec<i64>>())]
                }
                _ => Vec::new(),
            },
            Vec::new,
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        )
        .map_err(err)?
        .into_iter()
        .collect();

    // 3. Those nodes' positions.
    let mut want_nodes: Vec<i64> = ways.values().flatten().copied().collect();
    want_nodes.sort_unstable();
    want_nodes.dedup();
    let nodes: HashMap<i64, (i32, i32)> = reader()?
        .par_map_reduce(
            |el| {
                let (id, lon, lat) = match el {
                    Element::Node(n) => (n.id(), n.decimicro_lon(), n.decimicro_lat()),
                    Element::DenseNode(n) => (n.id(), n.decimicro_lon(), n.decimicro_lat()),
                    _ => return Vec::new(),
                };
                if want_nodes.binary_search(&id).is_ok() {
                    vec![(id, (lon, lat))]
                } else {
                    Vec::new()
                }
            },
            Vec::new,
            |mut a, mut b| {
                a.append(&mut b);
                a
            },
        )
        .map_err(err)?
        .into_iter()
        .collect();

    let mut out = Areas::default();
    for r in rels {
        match rings(&r.ways, &ways, &nodes) {
            Ok(rings) => out.areas.push(Area {
                tags: r.tags,
                rings,
            }),
            Err(why) => out.broken.push((r.id, why)),
        }
    }
    Ok(out)
}

/// Joins `members` (way IDs) end to end into closed rings of positions.
fn rings(
    members: &[i64],
    ways: &HashMap<i64, Vec<i64>>,
    nodes: &HashMap<i64, (i32, i32)>,
) -> Result<Vec<Vec<(i32, i32)>>, String> {
    let mut open: Vec<Vec<i64>> = Vec::with_capacity(members.len());
    for id in members {
        let w = ways
            .get(id)
            .ok_or_else(|| format!("way {id} is not in the extract"))?;
        if w.len() >= 2 {
            open.push(w.clone());
        }
    }
    let rings = join(open).map_err(|n| format!("its ways do not close at node {n}"))?;
    rings
        .into_iter()
        .map(|ring| {
            ring.iter()
                .map(|n| {
                    nodes
                        .get(n)
                        .copied()
                        .ok_or_else(|| format!("node {n} is not in the extract"))
                })
                .collect()
        })
        .collect()
}

/// Closed rings of node IDs from `ways`, each joined to the next at a shared end node, either
/// way round. Deterministic: rings start from the ways in the order given. Err names a node
/// where a ring stays open.
fn join(ways: Vec<Vec<i64>>) -> Result<Vec<Vec<i64>>, i64> {
    let mut out = Vec::new();
    // End node → the ways (by index) that start or end there.
    let mut ends: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, w) in ways.iter().enumerate() {
        if let (Some(&a), Some(&b)) = (w.first(), w.last()) {
            ends.entry(a).or_default().push(i);
            ends.entry(b).or_default().push(i);
        }
    }
    // A way is taken out of its slot when a ring uses it.
    let mut slots: Vec<Option<Vec<i64>>> = ways.into_iter().map(Some).collect();
    for start in 0..slots.len() {
        let Some(mut ring) = slots.get_mut(start).and_then(Option::take) else {
            continue;
        };
        while let (Some(&head), Some(&tail)) = (ring.first(), ring.last()) {
            if head == tail {
                break;
            }
            let mut w = ends
                .get(&tail)
                .and_then(|c| {
                    c.iter()
                        .find_map(|&j| slots.get_mut(j).and_then(Option::take))
                })
                .ok_or(tail)?;
            if w.first() != Some(&tail) {
                w.reverse();
            }
            ring.extend(w.into_iter().skip(1));
        }
        if ring.len() >= 4 {
            out.push(ring);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use super::*;

    #[test]
    fn ways_join_into_rings_either_way_round() {
        // A square of two ways, the second reversed, and a triangle of one closed way.
        let rings = join(vec![vec![1, 2, 3], vec![1, 4, 3], vec![7, 8, 9, 7]]).unwrap();
        assert_eq!(rings, [vec![1, 2, 3, 4, 1], vec![7, 8, 9, 7]]);
    }

    #[test]
    fn an_open_ring_is_refused() {
        assert_eq!(join(vec![vec![1, 2, 3], vec![3, 4]]), Err(4));
    }

    #[test]
    fn a_missing_way_or_node_is_named() {
        let ways = HashMap::from([(10, vec![1, 2, 3, 1])]);
        let nodes = HashMap::from([(1, (0, 0)), (2, (10, 0))]);
        assert!(rings(&[11], &ways, &nodes).unwrap_err().contains("way 11"));
        assert!(rings(&[10], &ways, &nodes).unwrap_err().contains("node 3"));
    }
}
