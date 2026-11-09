//! Spike S3: evaluates a name-search design on one country's golden set (ADR-0032,
//! names-and-search §5). World-general (principle P16): every country rule (abbreviations,
//! droppable suffixes, lexicon) comes from `data/countries/<cc>/`; the phonetic key is a named
//! algorithm from the library.
//!
//! Usage: `s3-eval <cc> <GeoNames CC.txt> [<COD-AB workbook.xlsx>]`
//!
//! Tiers, strongest first: exact fold (names and alternates) → loose (spaces removed, droppable
//! suffix removed, lexicon native generic replaced) → phonetic key → fuzzy (Levenshtein
//! automaton over the exact FST) → prefix. Within a tier: context match, unit-type match, then
//! importance. Results are entity groups (records of one entity across sources).
#![allow(
    clippy::print_stdout,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::needless_range_loop,
    missing_docs,
    reason = "an evaluation spike over pinned local inputs"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use spike_s3_search::engine::Engine;
use spike_s3_search::golden::{self, pct};
use spike_s3_search::{Rec, fold, lexicon, load, profile};

/// With `S3_ALIASES=<tsv>` (the forge `aliases` output), every alias joins its anchor
/// record's alternate names, so the index answers names that only OSM or Wikidata know.
fn add_aliases(recs: &mut [Rec]) {
    let Some(path) = std::env::var_os("S3_ALIASES") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read the aliases file");
    let mut by_key: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let mut f = line.split('\t');
        if let (Some(anchor), Some(alias)) = (f.next(), f.next()) {
            by_key.entry(anchor).or_default().push(alias);
        }
    }
    let mut added = 0;
    for r in recs.iter_mut() {
        if let Some(aliases) = by_key.get(r.key.as_str()) {
            for a in aliases {
                if !r.alts.iter().any(|x| x == a) && r.name != *a {
                    r.alts.push((*a).to_string());
                    added += 1;
                }
            }
        }
    }
    println!("aliases: {added} added from {}", path.to_string_lossy());
}

/// One QWERTY neighbour, fixed, for the probe.
fn qwerty_neighbour(c: char) -> Option<char> {
    let rows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    rows.iter().find_map(|r| {
        let i = r.find(c)?;
        r.chars().nth(if i + 1 < r.len() { i + 1 } else { i - 1 })
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        args.len() >= 2,
        "usage: s3-eval <cc> <GeoNames CC.txt> [<COD-AB xlsx>]"
    );
    let cc = &args[0];
    let dir = format!("data/countries/{cc}");
    let prof = profile(&format!("{dir}/profile.yaml"));
    let t0 = Instant::now();
    let mut recs = load(&prof, &args[1], args.get(2).map(String::as_str));
    add_aliases(&mut recs);
    let t_load = t0.elapsed();
    let t0 = Instant::now();
    let engine = Engine::new(&recs, &dir);
    let t_build = t0.elapsed();
    let golden = golden::read(&format!("tests/golden/{cc}.yaml")).unwrap();

    // Key → group, and each group's member keys.
    let key_group: BTreeMap<&str, Vec<usize>> = recs
        .iter()
        .map(|r| (r.key.as_str(), vec![r.group]))
        .collect();

    let scores = golden::score(&engine, &golden, &key_group, |g| {
        recs.iter()
            .find(|r| r.group == g)
            .map_or(String::new(), |r| format!("{} [{}]", r.name, r.district))
    });

    // Probe (not part of the golden set): a first-letter keyboard typo on every exact_latin
    // query, which the golden typo class never makes. Measures what fuzzy anchoring costs.
    let (mut probe_n, mut probe_hit5) = (0u32, 0u32);
    for q in golden.queries.iter().filter(|q| q.class == "exact_latin") {
        let mut cs = q.query.chars();
        let Some(first) = cs.next() else { continue };
        let rest: String = cs.collect();
        let Some(sub) = qwerty_neighbour(first.to_ascii_lowercase()) else {
            continue;
        };
        let typo = format!("{}{rest}", sub.to_ascii_uppercase());
        let want: BTreeSet<usize> = q
            .expect
            .iter()
            .filter(|(_, g)| **g >= 2)
            .flat_map(|(k, _)| key_group.get(k.as_str()).into_iter().flatten().copied())
            .collect();
        probe_n += 1;
        if engine.search(&typo, 5).iter().any(|g| want.contains(g)) {
            probe_hit5 += 1;
        }
    }
    println!(
        "probe first-letter typo: recall@5 {:.3} ({probe_hit5}/{probe_n})",
        f64::from(probe_hit5) / f64::from(probe_n)
    );

    // Probe: one letter replaced in the middle of every exact_native query (one edit in
    // characters, two or three in UTF-8 bytes), never the first letter.
    let (mut native_n, mut native_hit5) = (0u32, 0u32);
    for q in golden.queries.iter().filter(|q| q.class == "exact_native") {
        let mut cs: Vec<char> = q.query.chars().collect();
        if cs.len() < 4 {
            continue;
        }
        let at = cs.len() / 2;
        cs[at] = if cs[at] == 'क' { 'ख' } else { 'क' };
        let typo: String = cs.into_iter().collect();
        let want: BTreeSet<usize> = q
            .expect
            .iter()
            .filter(|(_, g)| **g >= 2)
            .flat_map(|(k, _)| key_group.get(k.as_str()).into_iter().flatten().copied())
            .collect();
        native_n += 1;
        if engine.search(&typo, 5).iter().any(|g| want.contains(g)) {
            native_hit5 += 1;
        }
    }
    println!(
        "probe native one-letter typo: recall@5 {:.3} ({native_hit5}/{native_n})",
        f64::from(native_hit5) / f64::from(native_n)
    );

    // pkey pair sets (names-and-search §5.3): positives are every pair of spellings of one
    // lexicon term, Latin and native; negatives are distinct folded names of different
    // entities in the same parent unit. Plus the bucket-size distribution.
    let lex_path = format!("{dir}/lexicon.yaml");
    let lex = if std::path::Path::new(&lex_path).exists() {
        lexicon(&lex_path).terms
    } else {
        Vec::new()
    };
    let (mut pos_n, mut pos_hit) = (0u32, 0u32);
    let mut pos_miss = Vec::new();
    for t in &lex {
        let forms: Vec<String> = t
            .spellings()
            .cloned()
            .chain(t.native.iter().map(|n| n.text.clone()))
            .collect();
        for (i, a) in forms.iter().enumerate() {
            for b in &forms[i + 1..] {
                pos_n += 1;
                if engine.pkey(a) == engine.pkey(b) {
                    pos_hit += 1;
                } else {
                    pos_miss.push(format!("{a}={} {b}={}", engine.pkey(a), engine.pkey(b)));
                }
            }
        }
    }
    let mut by_parent: BTreeMap<&str, Vec<(String, usize)>> = BTreeMap::new();
    for r in recs
        .iter()
        .filter(|r| r.current && !r.district.is_empty() && r.name.is_ascii())
    {
        by_parent
            .entry(r.district.as_str())
            .or_default()
            .push((fold(&r.name), r.group));
    }
    let (mut neg_n, mut neg_merge) = (0u64, 0u64);
    for v in by_parent.values_mut() {
        v.sort();
        v.dedup_by(|a, b| a.0 == b.0);
        let keys: Vec<String> = v.iter().map(|(n, _)| engine.pkey(n)).collect();
        for i in 0..v.len() {
            for j in i + 1..v.len() {
                if v[i].1 != v[j].1 {
                    neg_n += 1;
                    neg_merge += u64::from(keys[i] == keys[j]);
                }
            }
        }
    }
    let mut buckets: Vec<f64> = engine
        .phon
        .postings
        .iter()
        .map(|p| p.len() as f64)
        .collect();
    println!(
        "pkey: positive pair recall {:.3} ({pos_hit}/{pos_n}); false-merge rate {:.5} ({neg_merge}/{neg_n}); bucket p50 {} p99 {} max {}",
        f64::from(pos_hit) / f64::from(pos_n.max(1)),
        neg_merge as f64 / neg_n.max(1) as f64,
        pct(&mut buckets, 0.5),
        pct(&mut buckets, 0.99),
        buckets.last().copied().unwrap_or(0.0)
    );
    if std::env::var_os("S3_MISSES").is_some() {
        println!("pkey positive misses: {pos_miss:?}");
    }
    println!(
        "records {}, groups {}, load {:.2?}, build {:.2?}",
        recs.len(),
        recs.iter().map(|r| r.group).collect::<BTreeSet<_>>().len(),
        t_load,
        t_build
    );
    if let Some(p) = &engine.pack {
        println!("SEARCH section: {} B; columns {:?}", p.body_len, p.columns);
    }
    println!(
        "index bytes: exact {}, loose {}, phonetic {}, total {}",
        engine.exact.bytes(),
        engine.loose.bytes(),
        engine.phon.bytes(),
        engine.exact.bytes() + engine.loose.bytes() + engine.phon.bytes()
    );
    golden::print(&scores, std::env::var_os("S3_MISSES").is_some());
}
