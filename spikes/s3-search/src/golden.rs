//! The golden query set (ADR-0032, names-and-search §5.7) and how a run is scored, shared by
//! `s3-eval` and `pack-eval` so both use one definition of a hit.
//!
//! An expected key maps to an entity group; a query's grade per group is the best grade among
//! its keys. A hit is the first result whose group has grade ≥ 2; it passes within `max_rank`.

use std::collections::BTreeMap;
use std::time::Instant;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct GoldenFile {
    #[serde(default)]
    pub country: Option<String>,
    pub queries: Vec<GoldenQuery>,
}

#[derive(Deserialize)]
pub struct GoldenQuery {
    pub id: String,
    pub class: String,
    pub query: String,
    pub expect: BTreeMap<String, u8>,
    pub max_rank: u8,
}

/// Reads and parses a golden file.
pub fn read(path: &str) -> Result<GoldenFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    serde_saphyr::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

#[must_use]
pub fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// Per class: n, hits@1, hits@5, pass (within `max_rank`), MRR@10, nDCG@10; latencies; misses.
#[derive(Default)]
pub struct Scores {
    pub per: BTreeMap<String, [f64; 6]>,
    pub misses: BTreeMap<String, Vec<String>>,
    pub lat_ns: Vec<f64>,
    pub lat_by_class: BTreeMap<String, Vec<f64>>,
}

/// Runs every query (top 10, latency as the median of 21 runs) and scores it. `key_group` maps
/// an expected key to its groups (one, unless a source key sits on several entities); `label`
/// names a group in the misses list.
pub fn score(
    search: impl Fn(&str) -> Vec<usize>,
    golden: &GoldenFile,
    key_group: &BTreeMap<&str, Vec<usize>>,
    label: impl Fn(usize) -> String,
) -> Scores {
    let mut s = Scores::default();
    for q in &golden.queries {
        // Grade per expected group (the best grade among its keys).
        let mut grade: BTreeMap<usize, u8> = BTreeMap::new();
        for (k, g) in &q.expect {
            for &gr in key_group.get(k.as_str()).map_or(&[][..], Vec::as_slice) {
                let e = grade.entry(gr).or_default();
                *e = (*e).max(*g);
            }
        }
        let res = search(&q.query);
        // Latency: median of repeated runs.
        let mut runs = Vec::with_capacity(21);
        for _ in 0..21 {
            let t = Instant::now();
            std::hint::black_box(search(std::hint::black_box(&q.query)));
            runs.push(t.elapsed().as_nanos() as f64);
        }
        let med = pct(&mut runs, 0.5);
        s.lat_ns.push(med);
        s.lat_by_class.entry(q.class.clone()).or_default().push(med);

        let first = res
            .iter()
            .position(|g| grade.get(g).is_some_and(|&x| x >= 2));
        let dcg: f64 = res
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let rel = f64::from(*grade.get(g).unwrap_or(&0));
                (2f64.powf(rel) - 1.0) / ((i + 2) as f64).log2()
            })
            .sum();
        let mut ideal: Vec<u8> = grade.values().copied().collect();
        ideal.sort_unstable_by(|a, b| b.cmp(a));
        let idcg: f64 = ideal
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, &rel)| (2f64.powf(f64::from(rel)) - 1.0) / ((i + 2) as f64).log2())
            .sum();
        let m = s.per.entry(q.class.clone()).or_default();
        m[0] += 1.0;
        m[1] += f64::from(u8::from(first == Some(0)));
        m[2] += f64::from(u8::from(first.is_some_and(|r| r < 5)));
        m[3] += f64::from(u8::from(first.is_some_and(|r| r < usize::from(q.max_rank))));
        m[4] += first.map_or(0.0, |r| 1.0 / (r + 1) as f64);
        m[5] += if idcg > 0.0 { dcg / idcg } else { 0.0 };
        // `S3_MISSES=first` lists every query whose first result is not a hit.
        let limit = if std::env::var_os("S3_MISSES").is_some_and(|v| v == "first") {
            1
        } else {
            usize::from(q.max_rank)
        };
        if first.is_none_or(|r| r >= limit) {
            let got: Vec<String> = res.iter().take(3).map(|&g| label(g)).collect();
            let want = grade.keys().next().map_or(String::new(), |&g| label(g));
            s.misses.entry(q.class.clone()).or_default().push(format!(
                "{} {:?} want {want} got {got:?} (rank {:?})",
                q.id,
                q.query,
                first.map(|r| r + 1)
            ));
        }
    }
    s
}

/// The per-class table (Markdown) and, when `misses`, every miss by class.
#[allow(clippy::print_stdout, reason = "the evaluators' report")]
pub fn print(s: &Scores, misses: bool) {
    println!(
        "| class | n | recall@1 | recall@5 | pass@max_rank | MRR@10 | nDCG@10 | p50 µs | p99 µs |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut tot = [0f64; 6];
    for (c, m) in &s.per {
        let mut l = s.lat_by_class.get(c).cloned().unwrap_or_default();
        println!(
            "| {c} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} | {:.1} |",
            m[0],
            m[1] / m[0],
            m[2] / m[0],
            m[3] / m[0],
            m[4] / m[0],
            m[5] / m[0],
            pct(&mut l, 0.5) / 1e3,
            pct(&mut l, 0.99) / 1e3
        );
        for (t, v) in tot.iter_mut().zip(m) {
            *t += v;
        }
    }
    println!(
        "| **all** | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} | {:.1} |",
        tot[0],
        tot[1] / tot[0],
        tot[2] / tot[0],
        tot[3] / tot[0],
        tot[4] / tot[0],
        tot[5] / tot[0],
        pct(&mut s.lat_ns.clone(), 0.5) / 1e3,
        pct(&mut s.lat_ns.clone(), 0.99) / 1e3
    );
    if misses {
        for (c, v) in &s.misses {
            println!("\n## misses: {c} ({})", v.len());
            for m in v {
                println!("  {m}");
            }
        }
    }
}
