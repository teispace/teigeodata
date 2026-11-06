//! The `links` stage: the entities the sources themselves say are the same (`same_as` links:
//! OSM `wikidata=*`, Wikidata P1566 and P402, `GeoNames` `wkdt`), joined by union-find so that a
//! pair stated through a third source counts too. Each cross-source pair's great-circle
//! distance measures position disagreement without depending on our matcher, and the pairs
//! become silver labels for it. Clusters with two records of one source flag duplicates or
//! wrong links.
//!
//! Output: `data/build/links/<sources joined by +>/pairs.parquet` (`a`, `b`, `stated`,
//! `metres`) and `summary.md`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use arrow_array::StringArray;
use arrow_schema::{DataType, Field};

use crate::record::{self, Point, bools, column, f32s, strings, write_table};

/// Two points closer than this are one point copied between sources, not two observations.
const COPY_M: f64 = 1.0;

/// Mean Earth radius in metres (IUGG).
const R_M: f64 = 6_371_008.8;

/// Great-circle distance in metres between two grid points.
pub(crate) fn haversine(a: &Point, b: &Point) -> f64 {
    let rad = |v: i32| (f64::from(v) * 1e-7).to_radians();
    let (la, lb) = (rad(a.lat), rad(b.lat));
    let (dlat, dlon) = (lb - la, rad(b.lon) - rad(a.lon));
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R_M * h.sqrt().min(1.0).asin()
}

/// Union-find with path halving and union by size.
struct Sets {
    parent: Vec<u32>,
    size: Vec<u32>,
}

impl Sets {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..u32::try_from(n).unwrap_or(u32::MAX)).collect(),
            size: vec![1; n],
        }
    }

    fn find(&mut self, mut x: u32) -> u32 {
        while let Some(&p) = self.parent.get(x as usize) {
            if p == x {
                break;
            }
            let gp = self.parent.get(p as usize).copied().unwrap_or(p);
            if let Some(slot) = self.parent.get_mut(x as usize) {
                *slot = gp;
            }
            x = gp;
        }
        x
    }

    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return;
        }
        let size = |s: &Self, i: u32| s.size.get(i as usize).copied().unwrap_or(1);
        let (big, small) = if size(self, a) >= size(self, b) {
            (a, b)
        } else {
            (b, a)
        };
        let total = size(self, big) + size(self, small);
        if let Some(p) = self.parent.get_mut(small as usize) {
            *p = big;
        }
        if let Some(s) = self.size.get_mut(big as usize) {
            *s = total;
        }
    }
}

/// One cross-source pair.
struct Pair {
    a: u32,
    b: u32,
    stated: bool,
    metres: f64,
}

/// `forge links <points-source> <points-source> [...]`.
pub(crate) fn run(root: &Path, sources: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let read = root.join("data/build/read");
    let (pts, src) = load_points(&read, sources)?;
    let stated = stated_links(&read, sources, &pts)?;
    let members = clusters(&stated, pts.len());
    let source_of = |i: u32| src.get(i as usize).copied().unwrap_or(u8::MAX);
    // Per source: clusters holding two or more of its records.
    let mut duplicated = vec![0usize; sources.len()];
    for m in &members {
        let mut s: Vec<u8> = m.iter().map(|&i| source_of(i)).collect();
        s.sort_unstable();
        let mut twice: Vec<u8> = s
            .windows(2)
            .filter_map(|w| {
                (w.first() == w.get(1))
                    .then(|| w.first().copied())
                    .flatten()
            })
            .collect();
        twice.dedup();
        for t in twice {
            if let Some(d) = duplicated.get_mut(usize::from(t)) {
                *d += 1;
            }
        }
    }
    let pairs = cross_pairs(&members, &stated, &pts, &src);
    let dir = read
        .parent()
        .unwrap_or(&read)
        .join("links")
        .join(sources.join("+"));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let id = |i: u32| pts.get(i as usize).map_or("", |p| p.id.as_str());
    write_table(
        &dir.join("pairs.parquet"),
        vec![
            Field::new("a", DataType::Utf8, false),
            Field::new("b", DataType::Utf8, false),
            Field::new("stated", DataType::Boolean, false),
            Field::new("metres", DataType::Float32, false),
        ],
        &pairs,
        |r| {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "metres fit f32 at 0.5 m resolution"
            )]
            let m = |p: &Pair| Some(p.metres as f32);
            vec![
                strings(r.iter().map(|p| id(p.a))),
                strings(r.iter().map(|p| id(p.b))),
                bools(r.iter().map(|p| p.stated)),
                f32s(r.iter().map(m)),
            ]
        },
    )?;
    let summary = summarise(
        sources,
        &pts,
        &src,
        &pairs,
        members.len(),
        &duplicated,
        stated.len(),
    );
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    Ok(format!(
        "{summary}\n{} points in {:.1?}",
        pts.len(),
        t.elapsed()
    ))
}

/// The points of every source, concatenated, with each point's source index.
fn load_points(read: &Path, sources: &[String]) -> Result<(Vec<Point>, Vec<u8>), String> {
    let mut pts: Vec<Point> = Vec::new();
    let mut src: Vec<u8> = Vec::new();
    for (s, name) in sources.iter().enumerate() {
        let more = record::read_points(&read.join(name).join("records.parquet"))?;
        src.extend(std::iter::repeat_n(
            u8::try_from(s).unwrap_or(u8::MAX),
            more.len(),
        ));
        pts.extend(more);
    }
    Ok((pts, src))
}

/// Every `same_as` link whose two ends both have points, as sorted, unique index pairs.
fn stated_links(read: &Path, sources: &[String], pts: &[Point]) -> Result<Vec<(u32, u32)>, String> {
    let index: HashMap<&str, u32> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    let mut stated: Vec<(u32, u32)> = Vec::new();
    for name in sources {
        record::for_each_batch(&read.join(name).join("links.parquet"), |b| {
            let (ids, rels, targets) = (
                column::<StringArray>(b, "id")?,
                column::<StringArray>(b, "rel")?,
                column::<StringArray>(b, "target")?,
            );
            for i in 0..b.num_rows() {
                if rels.value(i) != "same_as" {
                    continue;
                }
                if let (Some(&x), Some(&y)) = (index.get(ids.value(i)), index.get(targets.value(i)))
                {
                    stated.push((x.min(y), x.max(y)));
                }
            }
            Ok(())
        })?;
    }
    stated.sort_unstable();
    stated.dedup();
    Ok(stated)
}

/// The members of every linked cluster, each sorted, in a fixed order.
fn clusters(stated: &[(u32, u32)], n: usize) -> Vec<Vec<u32>> {
    let mut sets = Sets::new(n);
    for &(a, b) in stated {
        sets.union(a, b);
    }
    let mut by_root: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(a, b) in stated {
        by_root.entry(sets.find(a)).or_default().extend([a, b]);
    }
    let mut members: Vec<Vec<u32>> = by_root.into_values().collect();
    for m in &mut members {
        m.sort_unstable();
        m.dedup();
    }
    members.sort_unstable();
    members
}

/// Every pair of records from different sources within a cluster, with its distance.
fn cross_pairs(
    members: &[Vec<u32>],
    stated: &[(u32, u32)],
    pts: &[Point],
    src: &[u8],
) -> Vec<Pair> {
    let source_of = |i: u32| src.get(i as usize).copied().unwrap_or(u8::MAX);
    let mut pairs = Vec::new();
    for m in members {
        for (k, &a) in m.iter().enumerate() {
            for &b in m.iter().skip(k + 1) {
                if source_of(a) == source_of(b) {
                    continue;
                }
                if let (Some(pa), Some(pb)) = (pts.get(a as usize), pts.get(b as usize)) {
                    let stated = stated.binary_search(&(a, b)).is_ok();
                    pairs.push(Pair {
                        a,
                        b,
                        stated,
                        metres: haversine(pa, pb),
                    });
                }
            }
        }
    }
    pairs
}

/// The nearest-rank percentile of sorted values.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an index into a slice"
    )]
    let rank = ((q * sorted.len() as f64).ceil() as usize).max(1);
    sorted.get(rank - 1).copied().unwrap_or(f64::NAN)
}

fn summarise(
    sources: &[String],
    pts: &[Point],
    src: &[u8],
    pairs: &[Pair],
    clusters: usize,
    duplicated: &[usize],
    stated: usize,
) -> String {
    let mut out = format!(
        "## links: {}\n\n{stated} stated links between records with points; {clusters} clusters. Clusters with two or more records of one source: {}.\n\n| Pair | Kinds | Pairs | Stated | Copied (< 1 m) | P50 | P90 | P99 | > 1 km | > 10 km |\n|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        sources.join(", "),
        sources
            .iter()
            .zip(duplicated)
            .map(|(s, d)| format!("{s} {d}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let source_of = |i: u32| src.get(i as usize).copied().unwrap_or(u8::MAX);
    // The kinds of both ends, in source order: a village linked to a former unit is a
    // granularity mismatch, not a position error, so it is reported apart.
    let kind_of = |p: &Pair| {
        let k = |i: u32| pts.get(i as usize).map_or("", |p| p.kind.as_str());
        if k(p.a) == k(p.b) {
            k(p.a).to_string()
        } else {
            format!("{} / {}", k(p.a), k(p.b))
        }
    };
    let name = |s: u8| sources.get(usize::from(s)).map_or("?", String::as_str);
    let mut groups: Vec<((u8, u8), String)> = pairs
        .iter()
        .flat_map(|p| {
            let key = (source_of(p.a), source_of(p.b));
            [(key, "all".to_string()), (key, kind_of(p))]
        })
        .collect();
    groups.sort_unstable();
    groups.dedup();
    for ((sa, sb), kind) in groups {
        let group: Vec<&Pair> = pairs
            .iter()
            .filter(|p| {
                (source_of(p.a), source_of(p.b)) == (sa, sb)
                    && (kind == "all" || kind_of(p) == kind)
            })
            .collect();
        let direct = group.iter().filter(|p| p.stated).count();
        // Copies (the same point to within rounding) are counted, not measured: they say
        // nothing about position error and would pull every percentile towards zero.
        let copied = group.iter().filter(|p| p.metres < COPY_M).count();
        let mut d: Vec<f64> = group
            .iter()
            .map(|p| p.metres)
            .filter(|&m| m >= COPY_M)
            .collect();
        d.sort_unstable_by(f64::total_cmp);
        let over = |m: f64| d.iter().filter(|&&x| x > m).count();
        let km = |m: f64| {
            if m >= 1000.0 {
                format!("{:.1} km", m / 1000.0)
            } else {
                format!("{m:.0} m")
            }
        };
        let _ = writeln!(
            out,
            "| {}–{} | {kind} | {} | {direct} | {copied} | {} | {} | {} | {} | {} |",
            name(sa),
            name(sb),
            group.len(),
            km(percentile(&d, 0.5)),
            km(percentile(&d, 0.9)),
            km(percentile(&d, 0.99)),
            over(1000.0),
            over(10_000.0)
        );
    }
    out.push_str("\nPercentiles are over the pairs that are not copies.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::{Point, Sets, haversine, percentile};

    #[test]
    fn distances_sets_and_ranks() {
        let p = |lon, lat| Point {
            lon,
            lat,
            ..Point::default()
        };
        // One degree of latitude on the mean sphere.
        let d = haversine(&p(0, 0), &p(0, 10_000_000));
        assert!((d - 111_195.08).abs() < 0.1, "{d}");
        assert!(haversine(&p(853_206_000, 277_016_900), &p(853_206_000, 277_016_900)) < 1e-9);
        let mut s = Sets::new(5);
        s.union(0, 1);
        s.union(3, 4);
        s.union(1, 4);
        assert_eq!(s.find(0), s.find(3));
        assert_ne!(s.find(0), s.find(2));
        let v = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((percentile(&v, 0.5) - 5.0).abs() < 1e-12);
        assert!((percentile(&v, 0.9) - 9.0).abs() < 1e-12);
        assert!((percentile(&v, 0.99) - 10.0).abs() < 1e-12);
    }
}
