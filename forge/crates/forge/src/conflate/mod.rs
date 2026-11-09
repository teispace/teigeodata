//! The `conflate` stage: which records of different sources are the same place (forge note
//! §4). Blocking on a name key within neighbouring 0.1° cells, plus the sources' stated
//! links; a Fellegi–Sunter model with u from seeded random pairs and m by EM in two sessions
//! (one blocked on names, one on stated IDs, so each field is estimated by a session that did
//! not select on it); union-find over pairs by descending weight, refusing to join two records
//! of one source (an intra-source duplicate is a QA finding, never a silent merge).
//!
//! The stated links are also the silver labels. A second model without the ID field is scored
//! against them, so the evaluation is not circular.
//!
//! Output: `data/build/conflate/<sources>/clusters.parquet` (`id`, `cluster`) and `summary.md`.

mod model;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Instant;

use arrow_array::StringArray;
use arrow_schema::{DataType, Field};
use rayon::prelude::*;
use serde::Deserialize;
use teigeodata_names::{PhoneticKey, fold, join_key};

use crate::links::{Sets, haversine, load_points, stated_links};
use crate::record::{self, Point, column, strings, write_table};
use model::{FIELDS, Gamma, ID, Model, NAME, Patterns, Rng};

/// Blocking cells: 0.1° (about 11 km); a pair must share a name key within 3×3 cells.
const CELL: i32 = 1_000_000;
/// Blocks larger than this many candidate pairs are purged (a very common name in one area).
const MAX_BLOCK_PAIRS: usize = 250_000;
/// Names kept per record, in sorted order.
const MAX_NAMES: usize = 64;
/// Random pairs for u.
const U_PAIRS: usize = 1_000_000;
/// The sampling seed: fixed, so the model and every output are reproducible.
const SEED: u64 = 0x7465_6967_656f_6461; // "teigeoda"
/// The match threshold on P(match).
const THRESHOLD: f64 = 0.9;
/// Stated links farther apart than this are not used as silver labels: S5 found the tail is
/// wrong links (D-056).
const SILVER_MAX_M: f64 = 10_000.0;

/// A record's name keys, as sorted 64-bit hashes.
#[derive(Default)]
struct Keys {
    fold: Vec<u64>,
    join: Vec<u64>,
    phonetic: Vec<u64>,
}

/// FNV-1a: a stable hash, the same on every platform and run.
fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[derive(Deserialize)]
struct Profile {
    #[serde(default)]
    phonetic_key: Option<String>,
}

/// `forge conflate <cc> <points-source> <points-source> [...]`.
pub(crate) fn run(root: &Path, cc: &str, sources: &[String]) -> Result<String, String> {
    let t = Instant::now();
    let key_name = phonetic_key(root, cc)?;
    let key = key_name.as_deref().and_then(PhoneticKey::from_name);
    let read = root.join("data/build/read");
    let (pts, src) = load_points(&read, sources)?;
    let keys = name_keys(&read, sources, &pts, key)?;
    let stated = stated_links(&read, sources, &pts)?;
    // Incompatible kinds (a populated place and an admin unit) are a cannot-link: never
    // candidates, so EM learns m and λ among pairs that could be one entity.
    let compatible = |&(a, b): &(u32, u32)| {
        pts.get(a as usize)
            .zip(pts.get(b as usize))
            .is_some_and(|(x, y)| model::kind_level(&x.kind, &y.kind) < 2)
    };
    let all_blocked = candidates(&pts, &src, &keys);
    let blocked: Vec<(u32, u32)> = all_blocked.iter().copied().filter(compatible).collect();
    let comparer = Comparer {
        pts: &pts,
        keys: &keys,
        stated: &stated,
    };
    let gamma = |p: &(u32, u32)| comparer.gamma(*p);
    let count = |pairs: &[(u32, u32)]| -> Patterns {
        let mut p = Patterns::new();
        for g in pairs.par_iter().map(gamma).collect::<Vec<_>>() {
            *p.entry(g).or_default() += 1;
        }
        p
    };
    let random = count(&random_pairs(&src));
    let on_names = count(&blocked);
    let cross_stated: Vec<(u32, u32)> = stated
        .iter()
        .copied()
        .filter(|&(a, b)| src.get(a as usize) != src.get(b as usize))
        .collect();
    let stated_ok: Vec<(u32, u32)> = cross_stated.iter().copied().filter(compatible).collect();
    let on_ids = count(&stated_ok);
    let cross_pairs = cross_pair_count(sources.len(), &src);
    let full = fit(
        &random,
        &on_names,
        &on_ids,
        [false; 4],
        blocked.len(),
        cross_pairs,
    );
    let mut skip = [false; 4];
    if let Some(s) = skip.get_mut(ID) {
        *s = true;
    }
    let eval = fit(
        &random,
        &on_names,
        &on_ids,
        skip,
        blocked.len(),
        cross_pairs,
    );
    // Score every candidate and every stated pair.
    let mut all: Vec<(u32, u32)> = blocked.iter().chain(&stated_ok).copied().collect();
    all.sort_unstable();
    all.dedup();
    let gammas: Vec<Gamma> = all.par_iter().map(gamma).collect();
    let kinds: Vec<&str> = pts.iter().map(|p| p.kind.as_str()).collect();
    let bridges = bridges(&cross_stated, &pts, &keys);
    let twins = twins(&cross_stated, &pts, &src, &keys);
    let full_roots = cluster(&all, &gammas, &full, &src, &kinds, &twins, &bridges);
    let eval_roots = cluster(&all, &gammas, &eval, &src, &kinds, &[], &[]);
    let dir = root.join("data/build/conflate").join(sources.join("+"));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    write_clusters(&dir, &pts, &full_roots)?;
    let mut summary = header(
        sources,
        pts.len(),
        key_name.as_deref(),
        [
            blocked.len(),
            all_blocked.len() - blocked.len(),
            cross_stated.len(),
            stated_ok.len(),
        ],
    );
    write_joins(&mut summary, bridges.len(), twins.len());
    write_model(&mut summary, &full);
    write_clusters_summary(&mut summary, sources, &src, &full_roots);
    evaluate(
        &mut summary,
        sources,
        &cross_stated,
        &pts,
        &src,
        &eval_roots,
    );
    fs::write(dir.join("summary.md"), &summary).map_err(|e| e.to_string())?;
    Ok(format!("{summary}\n{:.1?}", t.elapsed()))
}

/// The summary's lines on the joins outside the model: bridges and a source's duplicates.
fn write_joins(out: &mut String, bridges: usize, twins: usize) {
    let _ = writeln!(
        out,
        "Stated links between a populated place and a former admin unit with a shared name key, within {} km, joined after the model: {}.\n",
        SILVER_MAX_M / 1000.0,
        bridges
    );
    let _ = writeln!(
        out,
        "Records of one source that state the same record of another, with a shared name key, within {} km, joined first as duplicates: {}.\n",
        SILVER_MAX_M / 1000.0,
        twins
    );
}

/// Builds the comparison vector of a pair.
struct Comparer<'a> {
    pts: &'a [Point],
    keys: &'a [Keys],
    stated: &'a [(u32, u32)],
}

impl Comparer<'_> {
    fn gamma(&self, (a, b): (u32, u32)) -> Gamma {
        let (Some(pa), Some(pb)) = (self.pts.get(a as usize), self.pts.get(b as usize)) else {
            return [3, 4, 2, 1];
        };
        let (ka, kb) = (self.keys.get(a as usize), self.keys.get(b as usize));
        let shares =
            |f: fn(&Keys) -> &Vec<u64>| ka.zip(kb).is_some_and(|(x, y)| intersects(f(x), f(y)));
        let name = if shares(|k| &k.fold) {
            0
        } else if shares(|k| &k.join) {
            1
        } else if shares(|k| &k.phonetic) {
            2
        } else {
            3
        };
        [
            name,
            model::distance_level(haversine(pa, pb)),
            model::kind_level(&pa.kind, &pb.kind),
            u8::from(self.stated.binary_search(&(a, b)).is_err()),
        ]
    }
}

/// The summary's opening lines. `counts`: candidates kept, candidates dropped as incompatible
/// kinds, stated cross-source links, stated links between compatible kinds.
fn header(sources: &[String], points: usize, key: Option<&str>, counts: [usize; 4]) -> String {
    let [kept, dropped, stated, stated_ok] = counts;
    format!(
        "## conflate: {}\n\n{points} points; phonetic key {}; {kept} candidate pairs from name blocking ({dropped} more dropped as incompatible kinds), {stated} stated cross-source links ({stated_ok} between compatible kinds), {U_PAIRS} random pairs for u.\n\n",
        sources.join(", "),
        key.unwrap_or("none"),
    )
}

/// The phonetic key a country's profile names.
pub(crate) fn phonetic_key(root: &Path, cc: &str) -> Result<Option<String>, String> {
    let path = root.join("data/countries").join(cc).join("profile.yaml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let profile: Profile =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(profile.phonetic_key)
}

/// The number of record pairs from different sources.
fn cross_pair_count(sources: usize, src: &[u8]) -> f64 {
    let sizes: Vec<usize> = (0..sources)
        .map(|s| src.iter().filter(|&&x| usize::from(x) == s).count())
        .collect();
    #[allow(clippy::cast_precision_loss, reason = "pair counts")]
    let total = sizes
        .iter()
        .enumerate()
        .flat_map(|(i, a)| {
            sizes
                .iter()
                .skip(i + 1)
                .map(move |b| (*a as f64) * (*b as f64))
        })
        .sum();
    total
}

/// Fits a model: u from random pairs; EM on name-blocked pairs (conditioned on the name), on
/// ID-blocked pairs (conditioned on the ID), and on name-blocked pairs again. The scoring prior
/// is the global P(match) between two records of different sources: the name session's λ times
/// its pair count, over all cross-source pairs.
fn fit(
    random: &Patterns,
    on_names: &Patterns,
    on_ids: &Patterns,
    skip: [bool; 4],
    blocked_pairs: usize,
    cross_pairs: f64,
) -> Model {
    let mut m = Model {
        m: model::initial_m(),
        u: model::frequencies(random),
        lambda: 0.5,
        skip,
    };
    m.em(on_names, &[NAME], 30);
    m.lambda = 0.5;
    m.em(on_ids, &[ID], 30);
    m.lambda = 0.5;
    m.em(on_names, &[NAME], 30);
    #[allow(clippy::cast_precision_loss, reason = "pair counts")]
    let expected = m.lambda * blocked_pairs as f64;
    m.lambda = (expected / cross_pairs.max(1.0)).clamp(1e-12, 0.5);
    m
}

fn intersects(a: &[u64], b: &[u64]) -> bool {
    let (mut i, mut j) = (0, 0);
    while let (Some(x), Some(y)) = (a.get(i), b.get(j)) {
        match x.cmp(y) {
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    false
}

/// The fold, join and phonetic keys of every record's names (at most [`MAX_NAMES`] each).
fn name_keys(
    read: &Path,
    sources: &[String],
    pts: &[Point],
    key: Option<PhoneticKey>,
) -> Result<Vec<Keys>, String> {
    let index: HashMap<&str, usize> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();
    let mut texts: Vec<Vec<String>> = vec![Vec::new(); pts.len()];
    for name in sources {
        record::for_each_batch(&read.join(name).join("names.parquet"), |b| {
            let (ids, text) = (
                column::<StringArray>(b, "id")?,
                column::<StringArray>(b, "text")?,
            );
            for i in 0..b.num_rows() {
                if let Some(t) = index.get(ids.value(i)).and_then(|&r| texts.get_mut(r)) {
                    t.push(text.value(i).to_string());
                }
            }
            Ok(())
        })?;
    }
    Ok(texts
        .into_par_iter()
        .map(|mut t| {
            t.sort_unstable();
            t.dedup();
            t.truncate(MAX_NAMES);
            let set = |f: &dyn Fn(&str) -> String| {
                let mut v: Vec<u64> = t
                    .iter()
                    .map(|s| f(s))
                    .filter(|s| !s.is_empty())
                    .map(|s| hash(&s))
                    .collect();
                v.sort_unstable();
                v.dedup();
                v
            };
            Keys {
                fold: set(&fold),
                join: set(&join_key),
                phonetic: key.map(|k| set(&|s: &str| k.key(s))).unwrap_or_default(),
            }
        })
        .collect())
}

/// Cross-source pairs that share a join or phonetic key within neighbouring cells.
fn candidates(pts: &[Point], src: &[u8], keys: &[Keys]) -> Vec<(u32, u32)> {
    let mut blocks: HashMap<(u64, i32, i32), Vec<u32>> = HashMap::new();
    for (i, (p, k)) in pts.iter().zip(keys).enumerate() {
        let cell = (p.lon.div_euclid(CELL), p.lat.div_euclid(CELL));
        let mut all: Vec<u64> = k.join.iter().chain(&k.phonetic).copied().collect();
        all.sort_unstable();
        all.dedup();
        for h in all {
            blocks
                .entry((h, cell.0, cell.1))
                .or_default()
                .push(u32::try_from(i).unwrap_or(u32::MAX));
        }
    }
    let mut keys_sorted: Vec<&(u64, i32, i32)> = blocks.keys().collect();
    keys_sorted.sort_unstable();
    let source_of = |i: u32| src.get(i as usize).copied();
    let mut pairs: Vec<(u32, u32)> = keys_sorted
        .par_iter()
        .flat_map_iter(|&&(h, cx, cy)| {
            let here = blocks.get(&(h, cx, cy)).map_or(&[][..], Vec::as_slice);
            let mut out = Vec::new();
            // Each neighbouring block once: this cell and the four "later" neighbours.
            for (dx, dy) in [(0, 0), (1, -1), (1, 0), (1, 1), (0, 1)] {
                let Some(there) = blocks.get(&(h, cx + dx, cy + dy)) else {
                    continue;
                };
                if here.len().saturating_mul(there.len()) > MAX_BLOCK_PAIRS {
                    continue;
                }
                for &a in here {
                    for &b in there {
                        if source_of(a) != source_of(b) {
                            out.push((a.min(b), a.max(b)));
                        }
                    }
                }
            }
            out
        })
        .collect();
    pairs.par_sort_unstable();
    pairs.dedup();
    pairs
}

/// Seeded random cross-source pairs.
fn random_pairs(src: &[u8]) -> Vec<(u32, u32)> {
    let mut rng = Rng(SEED);
    let mut out = Vec::with_capacity(U_PAIRS);
    let n = src.len();
    let mut tries = 0usize;
    while out.len() < U_PAIRS && tries < U_PAIRS * 20 && n > 1 {
        tries += 1;
        let (a, b) = (rng.below(n), rng.below(n));
        if src.get(a) != src.get(b) {
            let (a, b) = (u32::try_from(a).unwrap_or(0), u32::try_from(b).unwrap_or(0));
            out.push((a.min(b), a.max(b)));
        }
    }
    out
}

/// Union-find over pairs at P(match) ≥ [`THRESHOLD`], by descending weight then by index (a
/// total order, so the result is deterministic), refusing any union that would put two
/// records of one source in a cluster. Returns each record's root.
/// The stated links that may join a populated place (or locality) and a former admin unit:
/// across sources, sharing a name key, within [`SILVER_MAX_M`]. Wikidata often has one item
/// for a former unit and the settlement it was named after, typed as a settlement and linked
/// to the `GeoNames` former unit, so the source itself states they are one entity. Without a
/// stated link the two stay apart: a village and its former unit are different places.
fn bridges(stated: &[(u32, u32)], pts: &[Point], keys: &[Keys]) -> Vec<(u32, u32)> {
    let comparer = Comparer { pts, keys, stated };
    stated
        .iter()
        .copied()
        .filter(|&(a, b)| {
            let (Some(x), Some(y)) = (pts.get(a as usize), pts.get(b as usize)) else {
                return false;
            };
            let (fx, fy) = (model::family(&x.kind), model::family(&y.kind));
            let settled_former = |s: u8, f: u8, former: bool| s == 0 && f == 1 && former;
            (settled_former(fx, fy, y.former) || settled_former(fy, fx, x.former))
                && haversine(x, y) <= SILVER_MAX_M
                && comparer.gamma((a, b)).first().is_some_and(|&n| n < 3)
        })
        .collect()
}

/// Records of one source that state the same record of another source: duplicates within the
/// source (Wikidata has bot-made items copied from `GeoNames` beside the main item, both
/// stating one `GeoNames` ID, which P1566's distinct-values constraint forbids). Without this,
/// one source per cluster splits the place: the main item joins one record, the copy another.
/// Kept when the two share a name key, are within [`SILVER_MAX_M`] and are not a populated place
/// and an admin unit, nor of another kind than the record they state (a settlement item stating
/// a former unit is a bridge's case, and its bot copy is the former unit's); a wrong ID
/// (another place's) fails the name or the distance.
fn twins(stated: &[(u32, u32)], pts: &[Point], src: &[u8], keys: &[Keys]) -> Vec<(u32, u32)> {
    let comparer = Comparer { pts, keys, stated };
    // Each record's stated partners, by the partner's source.
    let mut by_target: HashMap<u32, Vec<(u8, u32)>> = HashMap::new();
    for &(a, b) in stated {
        for (t, o) in [(a, b), (b, a)] {
            if let Some(&s) = src.get(o as usize) {
                by_target.entry(t).or_default().push((s, o));
            }
        }
    }
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (&t, partners) in &mut by_target {
        let Some(pt) = pts.get(t as usize) else {
            continue;
        };
        partners.sort_unstable();
        partners.dedup();
        for (i, &(sx, x)) in partners.iter().enumerate() {
            for &(_, y) in partners.iter().skip(i + 1).take_while(|(s, _)| *s == sx) {
                let (Some(px), Some(py)) = (pts.get(x as usize), pts.get(y as usize)) else {
                    continue;
                };
                if [px, py]
                    .iter()
                    .all(|p| model::kind_level(&p.kind, &pt.kind) < 2)
                    && model::kind_level(&px.kind, &py.kind) < 2
                    && haversine(px, py) <= SILVER_MAX_M
                    && comparer.gamma((x, y)).first().is_some_and(|&n| n < 3)
                {
                    out.push((x.min(y), x.max(y)));
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[allow(
    clippy::too_many_arguments,
    reason = "the clustering's inputs; each is a distinct list"
)]
fn cluster(
    pairs: &[(u32, u32)],
    gammas: &[Gamma],
    m: &Model,
    src: &[u8],
    kinds: &[&str],
    twins: &[(u32, u32)],
    bridges: &[(u32, u32)],
) -> Vec<u32> {
    let n = src.len();
    let mut edges: Vec<(f64, u32, u32)> = pairs
        .iter()
        .zip(gammas)
        .map(|(&(a, b), g)| (m.weight(*g), a, b))
        .filter(|(w, _, _)| model::probability(*w) >= THRESHOLD)
        .collect();
    edges.sort_unstable_by(|x, y| y.0.total_cmp(&x.0).then((x.1, x.2).cmp(&(y.1, y.2))));
    let mut sets = Sets::new(n);
    // Bits 0–47: sources; bits 48–50: kind families. A cluster may not hold two records of one
    // source, nor both a populated place and an admin unit (different entities that share a
    // name and a place, such as a village and its former VDC).
    let mut mask: Vec<u64> = src
        .iter()
        .zip(kinds)
        .map(|(&s, k)| (1u64 << s.min(47)) | (1u64 << (48 + model::family(k))))
        .collect();
    let conflict = |a: u64, b: u64| {
        let sources = (1u64 << 48) - 1;
        let both = a | b;
        a & b & sources != 0 || (both & (1 << 48) != 0 && both & (1 << 49) != 0)
    };
    // A source's duplicates join first: their union holds the source once, so the model's
    // edges then treat them as one record.
    for &(a, b) in twins {
        let (ra, rb) = (sets.find(a), sets.find(b));
        let (ma, mb) = (
            mask.get(ra as usize).copied().unwrap_or(0),
            mask.get(rb as usize).copied().unwrap_or(0),
        );
        if ra == rb || conflict(ma & !((1u64 << 48) - 1), mb) {
            continue;
        }
        sets.union(ra, rb);
        let r = sets.find(ra);
        if let Some(v) = mask.get_mut(r as usize) {
            *v = ma | mb;
        }
    }
    for (_, a, b) in edges {
        let (ra, rb) = (sets.find(a), sets.find(b));
        let (ma, mb) = (
            mask.get(ra as usize).copied().unwrap_or(0),
            mask.get(rb as usize).copied().unwrap_or(0),
        );
        if ra == rb || conflict(ma, mb) {
            continue;
        }
        sets.union(ra, rb);
        let r = sets.find(ra);
        if let Some(v) = mask.get_mut(r as usize) {
            *v = ma | mb;
        }
    }
    // A bridge may join a place and a former unit, never two records of one source. It runs
    // after the model's edges, so no model edge crosses the kinds through it.
    for &(a, b) in bridges {
        let (ra, rb) = (sets.find(a), sets.find(b));
        let (ma, mb) = (
            mask.get(ra as usize).copied().unwrap_or(0),
            mask.get(rb as usize).copied().unwrap_or(0),
        );
        if ra == rb || ma & mb & ((1u64 << 48) - 1) != 0 {
            continue;
        }
        sets.union(ra, rb);
        let r = sets.find(ra);
        if let Some(v) = mask.get_mut(r as usize) {
            *v = ma | mb;
        }
    }
    (0..u32::try_from(n).unwrap_or(u32::MAX))
        .map(|i| sets.find(i))
        .collect()
}

fn write_clusters(dir: &Path, pts: &[Point], roots: &[u32]) -> Result<(), String> {
    // A cluster is named by its smallest member ID, so the name does not depend on order.
    let mut name: BTreeMap<u32, &str> = BTreeMap::new();
    for (p, &r) in pts.iter().zip(roots) {
        let e = name.entry(r).or_insert(p.id.as_str());
        if p.id.as_str() < *e {
            *e = p.id.as_str();
        }
    }
    let rows: Vec<(&str, &str)> = pts
        .iter()
        .zip(roots)
        .map(|(p, r)| (p.id.as_str(), name.get(r).copied().unwrap_or("")))
        .collect();
    write_table(
        &dir.join("clusters.parquet"),
        vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("cluster", DataType::Utf8, false),
        ],
        &rows,
        |r| {
            vec![
                strings(r.iter().map(|x| x.0)),
                strings(r.iter().map(|x| x.1)),
            ]
        },
    )
}

fn write_model(out: &mut String, m: &Model) {
    let _ = writeln!(
        out,
        "Model: λ = {:.5}. Weights in bits, log2(m/u), per level (strongest agreement first):\n",
        m.lambda
    );
    let _ = writeln!(out, "| Field | Levels: m / u → bits |\n|---|---|");
    for (f, (name, n)) in FIELDS.iter().enumerate() {
        let cells: Vec<String> = (0..*n)
            .map(|l| {
                let mm = m.m.get(f).and_then(|r| r.get(l)).copied().unwrap_or(0.0);
                let uu = m.u.get(f).and_then(|r| r.get(l)).copied().unwrap_or(0.0);
                format!("{mm:.3} / {uu:.5} → {:+.1}", (mm / uu).log2())
            })
            .collect();
        let _ = writeln!(out, "| {name} | {} |", cells.join("; "));
    }
    out.push('\n');
}

fn write_clusters_summary(out: &mut String, sources: &[String], src: &[u8], roots: &[u32]) {
    let mut members: HashMap<u32, u64> = HashMap::new();
    for (&r, &s) in roots.iter().zip(src) {
        *members.entry(r).or_default() |= 1u64 << s.min(63);
    }
    let mut by_mask: BTreeMap<u64, usize> = BTreeMap::new();
    for m in members.values() {
        *by_mask.entry(*m).or_default() += 1;
    }
    let _ = writeln!(out, "| Sources in the cluster | Clusters |\n|---|---:|");
    for (mask, n) in by_mask.iter().rev() {
        let names: Vec<&str> = sources
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1u64 << (*i).min(63)) != 0)
            .map(|(_, s)| s.as_str())
            .collect();
        let _ = writeln!(out, "| {} | {n} |", names.join(" + "));
    }
    out.push('\n');
}

/// The model without the ID field against the stated links (silver labels). Recall: the
/// share of stated links (closer than [`SILVER_MAX_M`], between compatible kinds) whose two
/// records end in one cluster. Precision: of the merged cross-source pairs whose records both
/// carry stated links to the other source, the share the stated links agree with.
fn evaluate(
    out: &mut String,
    sources: &[String],
    stated: &[(u32, u32)],
    pts: &[Point],
    src: &[u8],
    roots: &[u32],
) {
    let root = |i: u32| roots.get(i as usize).copied();
    let source_of = |i: u32| src.get(i as usize).copied().unwrap_or(0);
    let name = |s: u8| sources.get(usize::from(s)).map_or("?", String::as_str);
    let compatible = |a: u32, b: u32| {
        pts.get(a as usize)
            .zip(pts.get(b as usize))
            .is_some_and(|(x, y)| {
                haversine(x, y) < SILVER_MAX_M && model::kind_level(&x.kind, &y.kind) < 2
            })
    };
    // Stated clusters, and which other sources each record has stated links to.
    let mut truth = Sets::new(pts.len());
    let mut covered: HashMap<u32, u64> = HashMap::new();
    for &(a, b) in stated {
        truth.union(a, b);
        *covered.entry(a).or_default() |= 1u64 << source_of(b).min(63);
        *covered.entry(b).or_default() |= 1u64 << source_of(a).min(63);
    }
    // Per source pair: links, recovered, merged pairs with links on both sides, agreeing.
    let mut rows: BTreeMap<(u8, u8), [usize; 4]> = BTreeMap::new();
    let mut excluded = 0usize;
    for &(a, b) in stated {
        if !compatible(a, b) {
            excluded += 1;
            continue;
        }
        let [n, hit, _, _] = rows.entry((source_of(a), source_of(b))).or_default();
        *n += 1;
        *hit += usize::from(root(a) == root(b));
    }
    let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (i, &r) in roots.iter().enumerate() {
        members
            .entry(r)
            .or_default()
            .push(u32::try_from(i).unwrap_or(u32::MAX));
    }
    let towards = |x: u32, s: u8| {
        covered
            .get(&x)
            .is_some_and(|c| c & (1u64 << s.min(63)) != 0)
    };
    for m in members.values().filter(|m| m.len() > 1) {
        for (k, &a) in m.iter().enumerate() {
            for &b in m.iter().skip(k + 1) {
                let (sa, sb) = (source_of(a), source_of(b));
                if sa == sb || !towards(a, sb) || !towards(b, sa) {
                    continue;
                }
                let [_, _, merged, agree] = rows.entry((sa.min(sb), sa.max(sb))).or_default();
                *merged += 1;
                *agree += usize::from(truth.find(a) == truth.find(b));
            }
        }
    }
    let _ = writeln!(
        out,
        "Silver labels: the model **without** the ID field, scored against the stated links ({excluded} left out: farther than {} km or between a populated place and an admin unit).\n\n| Source pair | Links | Recovered | Recall | Merged pairs with links on both sides | Agreeing | Precision |\n|---|---:|---:|---:|---:|---:|---:|",
        SILVER_MAX_M / 1000.0
    );
    #[allow(clippy::cast_precision_loss, reason = "counts")]
    let ratio = |a: usize, b: usize| {
        if b == 0 {
            f64::NAN
        } else {
            a as f64 / b as f64
        }
    };
    for ((a, b), [n, hit, merged, agree]) in rows {
        let _ = writeln!(
            out,
            "| {}–{} | {n} | {hit} | {:.3} | {merged} | {agree} | {:.3} |",
            name(a),
            name(b),
            ratio(hit, n),
            ratio(agree, merged)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Keys, Model, Point, bridges, cluster, hash, twins};

    fn point(id: &str, kind: &str, lat: i32, former: bool) -> Point {
        Point {
            id: id.into(),
            kind: kind.into(),
            lon: 856_000_000,
            lat,
            former,
        }
    }

    fn keys(name: &str) -> Keys {
        Keys {
            fold: vec![hash(name)],
            ..Keys::default()
        }
    }

    /// No model edge passes: only bridges can join.
    fn closed() -> Model {
        Model {
            m: [[1e-9; 5]; 4],
            u: [[1.0; 5]; 4],
            lambda: 1e-9,
            skip: [false; 4],
        }
    }

    #[test]
    fn a_stated_link_joins_a_settlement_and_its_former_unit_only() {
        // 1e-7° of latitude ≈ 1.11 cm: 22,000,000 ≈ 2.4 km, 99,000,000 ≈ 11 km.
        let pts = vec![
            point("wikidata:Q1", "populated_place", 271_000_000, false),
            point("geonames:1", "admin", 271_216_000, true),
            point("wikidata:Q2", "populated_place", 271_000_000, false),
            point("geonames:2", "admin", 271_216_000, false), // current unit
            point("wikidata:Q3", "populated_place", 271_000_000, false),
            point("geonames:3", "admin", 271_990_000, true), // 11 km away
            point("wikidata:Q4", "populated_place", 271_000_000, false),
            point("geonames:4", "admin", 271_216_000, true), // another name
        ];
        let k = [
            "atrouli", "atrouli", "bocha", "bocha", "masel", "masel", "gwadha", "hawangdi",
        ];
        let keys: Vec<Keys> = k.iter().map(|n| keys(n)).collect();
        let stated = vec![(0, 1), (2, 3), (4, 5), (6, 7)];
        assert_eq!(bridges(&stated, &pts, &keys), vec![(0, 1)]);
        let src = [0, 1, 0, 1, 0, 1, 0, 1];
        let kinds: Vec<&str> = pts.iter().map(|p| p.kind.as_str()).collect();
        let roots = cluster(&[], &[], &closed(), &src, &kinds, &[], &[(0, 1)]);
        assert_eq!(roots[0], roots[1]);
        assert!((2..8u32).all(|i| roots[i as usize] == i));
        // Never two records of one source.
        let roots = cluster(&[], &[], &closed(), &[0, 0], &kinds[..2], &[], &[(0, 1)]);
        assert_ne!(roots[0], roots[1]);
    }

    #[test]
    fn duplicates_of_one_source_join_and_carry_the_place() {
        // Two Wikidata items state geonames:1 (a main item and a bot copy); the OSM node states
        // the main item. geonames:2 is stated by an item of another name, and geonames:3 by two
        // items 11 km apart.
        let pts = vec![
            point("wikidata:Q1", "populated_place", 271_000_000, false),
            point("wikidata:Q2", "populated_place", 271_010_000, false),
            point("geonames:1", "populated_place", 271_005_000, false),
            point("osm:node/1", "populated_place", 271_002_000, false),
            point("wikidata:Q3", "populated_place", 272_000_000, false),
            point("wikidata:Q4", "populated_place", 272_000_000, false),
            point("geonames:2", "populated_place", 272_000_000, false),
            point("wikidata:Q5", "populated_place", 273_000_000, false),
            point("wikidata:Q6", "populated_place", 273_990_000, false),
            point("geonames:3", "populated_place", 273_000_000, false),
        ];
        let k = [
            "simikot", "simikot", "simikot", "simikot", "kodari", "kothari", "kodari", "waling",
            "waling", "waling",
        ];
        let keys: Vec<Keys> = k.iter().map(|n| keys(n)).collect();
        let src = [0, 0, 1, 2, 0, 0, 1, 0, 0, 1];
        let stated = vec![(0, 2), (0, 3), (1, 2), (4, 6), (5, 6), (7, 9), (8, 9)];
        assert_eq!(twins(&stated, &pts, &src, &keys), vec![(0, 1)]);
        // A settlement item and its bot copy (no class) both stating a former unit: no twins.
        let vdc = vec![
            point("wikidata:Q7", "populated_place", 274_000_000, false),
            point("wikidata:Q8", "other", 274_000_000, false),
            point("geonames:4", "admin", 274_000_000, true),
        ];
        let vk: Vec<Keys> = ["bocha"; 3].iter().map(|n| self::keys(n)).collect();
        assert_eq!(twins(&[(0, 2), (1, 2)], &vdc, &[0, 0, 1], &vk), vec![]);
        // Without the twins, the stated links split the place; with them, one cluster.
        let kinds: Vec<&str> = pts.iter().map(|p| p.kind.as_str()).collect();
        let open = Model {
            m: [[1.0; 5]; 4],
            u: [[1e-9; 5]; 4],
            lambda: 0.5,
            skip: [false; 4],
        };
        let gammas = vec![[0, 0, 0, 0]; 3];
        let pairs = [(0, 3), (1, 2), (2, 3)];
        let split = cluster(&pairs, &gammas, &open, &src, &kinds, &[], &[]);
        assert_ne!(split[0], split[1]);
        let one = cluster(&pairs, &gammas, &open, &src, &kinds, &[(0, 1)], &[]);
        assert!([1, 2, 3].iter().all(|&i| one[i] == one[0]));
        // A third record of the source still may not join.
        let three = cluster(&[(0, 4)], &gammas[..1], &open, &src, &kinds, &[(0, 1)], &[]);
        assert_ne!(three[0], three[4]);
        // Never a populated place and an admin unit, even as duplicates.
        let mixed = cluster(
            &[],
            &[],
            &open,
            &[0, 0],
            &["populated_place", "admin"],
            &[(0, 1)],
            &[],
        );
        assert_ne!(mixed[0], mixed[1]);
    }
}
