//! Generates the golden query set v0 for one country (ADR-0032, names-and-search §5.7) from a
//! `GeoNames` country file, an optional COD-AB workbook and the country's data folder, with a
//! fixed seed. World-general (principle P16): every country rule (unit types, abbreviations,
//! droppable suffixes, lexicon terms) comes from `data/countries/<cc>/`; hand-curated queries
//! come from `data/countries/<cc>/golden-curated.yaml` and are appended unchanged.
//!
//! Usage: `gen-golden <cc> <GeoNames CC.txt> [<COD-AB workbook.xlsx>] > tests/golden/<cc>.yaml`
#![allow(
    clippy::print_stdout,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    missing_docs,
    reason = "a generator spike over pinned local inputs"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use spike_s3_search::{
    Rec, Rng, fold, is_native_script, levenshtein, lexicon, load, profile, script_of,
};

struct Generated {
    class: &'static str,
    query: String,
    /// (key, grade 1–3)
    expect: Vec<(String, u8)>,
    max_rank: usize,
    rule: &'static str,
}

/// QWERTY neighbours for keyboard typos.
fn neighbours(c: char) -> &'static str {
    match c {
        'a' => "qwsz",
        'b' => "vghn",
        'c' => "xdfv",
        'd' => "serfcx",
        'e' => "wsdr",
        'f' => "drtgvc",
        'g' => "ftyhbv",
        'h' => "gyujnb",
        'i' => "ujko",
        'j' => "huikmn",
        'k' => "jiolm",
        'l' => "kop",
        'm' => "njk",
        'n' => "bhjm",
        'o' => "iklp",
        'p' => "ol",
        'q' => "wa",
        'r' => "edft",
        's' => "awedxz",
        't' => "rfgy",
        'u' => "yhji",
        'v' => "cfgb",
        'w' => "qase",
        'x' => "zsdc",
        'y' => "tghu",
        'z' => "asx",
        _ => "",
    }
}

fn typo(name: &str, rng: &mut Rng) -> String {
    let mut c: Vec<char> = name.chars().collect();
    let n = c.len();
    // Never touch the first letter: users rarely get it wrong, and it keeps the test fair.
    let i = 1 + rng.below(n - 2);
    match rng.below(4) {
        0 => {
            let lower = c[i].to_ascii_lowercase();
            let nb = neighbours(lower);
            if nb.is_empty() {
                c.remove(i);
            } else {
                c[i] = nb.chars().nth(rng.below(nb.len())).unwrap();
            }
        }
        1 => c.swap(i, i + 1),
        2 => {
            c.remove(i);
        }
        _ => c.insert(i, c[i]),
    }
    c.into_iter().collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cc = args[0].to_lowercase();
    let dir = format!("data/countries/{cc}");
    let prof = profile(&format!("{dir}/profile.yaml"));
    let recs = load(&prof, &args[1], args.get(2).map(String::as_str));
    let lex_path = format!("{dir}/lexicon.yaml");
    let lex = if std::path::Path::new(&lex_path).exists() {
        lexicon(&lex_path).terms
    } else {
        Vec::new()
    };
    let curated = std::fs::read_to_string(format!("{dir}/golden-curated.yaml")).unwrap_or_default();
    let native_scripts: BTreeSet<&str> = prof
        .scripts
        .iter()
        .map(String::as_str)
        .filter(|s| *s != "Latn")
        .collect();
    let droppable: Vec<String> = prof
        .droppable_suffixes
        .iter()
        .map(|t| t.text.to_lowercase())
        .collect();
    let abbreviations: BTreeMap<&str, Vec<&str>> = prof
        .unit_types
        .iter()
        .map(|u| {
            (
                u.id.as_str(),
                u.abbreviations.iter().map(|a| a.text.as_str()).collect(),
            )
        })
        .collect();
    let mut rng = Rng(0x7E16_E0DA_7A00_0001);

    // Folded string → groups that carry it (names and alternates), current records only, and
    // a separate map that includes historical records.
    let mut current: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut all: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for r in &recs {
        for s in std::iter::once(&r.name).chain(&r.alts) {
            let f = fold(s);
            if f.is_empty() {
                continue;
            }
            all.entry(f.clone()).or_default().insert(r.group);
            if r.current {
                current.entry(f).or_default().insert(r.group);
            }
        }
    }
    let mut by_group: BTreeMap<usize, Vec<&Rec>> = BTreeMap::new();
    for r in &recs {
        by_group.entry(r.group).or_default().push(r);
    }
    // A group's representative: its most important record; equivalents get grade 2.
    let expect_group = |g: usize, grade: u8| -> Vec<(String, u8)> {
        let mut v: Vec<&&Rec> = by_group[&g].iter().collect();
        v.sort_by(|a, b| b.importance.cmp(&a.importance).then(a.key.cmp(&b.key)));
        v.iter()
            .enumerate()
            .map(|(i, r)| (r.key.clone(), if i == 0 { grade } else { grade.min(2) }))
            .collect()
    };
    let importance = |g: usize| by_group[&g].iter().map(|r| r.importance).max().unwrap_or(0);
    let unique_current = |s: &str| current.get(&fold(s)).is_some_and(|g| g.len() == 1);

    let mut out: Vec<Generated> = Vec::new();
    let take = |mut v: Vec<Generated>, n: usize, rng: &mut Rng, out: &mut Vec<Generated>| {
        // Deterministic sample of n, then back in a stable order.
        let mut picked = Vec::new();
        while !v.is_empty() && picked.len() < n {
            picked.push(v.swap_remove(rng.below(v.len())));
        }
        picked.sort_by(|a, b| a.query.cmp(&b.query));
        out.extend(picked);
    };

    // exact_latin: unique names of notable records (importance ≥ 30), ASCII only.
    let mut v = Vec::new();
    let mut seen = BTreeSet::new();
    for r in recs
        .iter()
        .filter(|r| r.current && r.importance >= 30 && r.name.is_ascii() && r.name.len() >= 4)
    {
        if unique_current(&r.name) && seen.insert(r.group) {
            v.push(Generated {
                class: "exact_latin",
                query: r.name.clone(),
                expect: expect_group(r.group, 3),
                max_rank: 1,
                rule: "unique name",
            });
        }
    }
    take(v, 70, &mut rng, &mut out);

    // exact_native: unique alternates in one of the country's own non-Latin scripts (profile).
    let mut v = Vec::new();
    for r in recs.iter().filter(|r| r.current) {
        for a in r
            .alts
            .iter()
            .filter(|a| is_native_script(a) && native_scripts.contains(script_of(a).short_name()))
        {
            if unique_current(a) {
                v.push(Generated {
                    class: "exact_native",
                    query: a.clone(),
                    expect: expect_group(r.group, 3),
                    max_rank: 1,
                    rule: "unique native-script alternate",
                });
                break;
            }
        }
    }
    take(v, 40, &mut rng, &mut out);

    // typo: one edit of a unique notable ASCII name (length ≥ 6).
    let mut v = Vec::new();
    let mut seen = BTreeSet::new();
    for r in recs.iter().filter(|r| {
        r.current
            && r.importance >= 30
            && r.name.is_ascii()
            && r.name.len() >= 6
            && !r.name.contains(' ')
    }) {
        if unique_current(&r.name) && seen.insert(r.group) {
            let q = typo(&r.name, &mut rng);
            if !current.contains_key(&fold(&q)) && levenshtein(&fold(&q), &fold(&r.name)) >= 1 {
                v.push(Generated {
                    class: "typo",
                    query: q,
                    expect: expect_group(r.group, 3),
                    max_rank: 5,
                    rule: "one keyboard edit",
                });
            }
        }
    }
    take(v, 80, &mut rng, &mut out);

    // prefix: 4 letters of very notable names where the target is the most important record
    // sharing that prefix.
    let mut v = Vec::new();
    let mut seen = BTreeSet::new();
    for r in recs
        .iter()
        .filter(|r| r.current && r.importance >= 60 && r.name.is_ascii() && r.name.len() >= 6)
    {
        let p: String = fold(&r.name).chars().take(4).collect();
        let best = current
            .iter()
            .filter(|(k, _)| k.starts_with(&p))
            .flat_map(|(_, g)| g.iter())
            .map(|&g| importance(g))
            .max()
            .unwrap_or(0);
        if importance(r.group) >= best && seen.insert(p.clone()) {
            v.push(Generated {
                class: "prefix",
                query: r.name.chars().take(4).collect(),
                expect: expect_group(r.group, 3),
                max_rank: 5,
                rule: "4-letter prefix, target most important",
            });
        }
    }
    take(v, 40, &mut rng, &mut out);

    // ambiguous: shared names where one entity clearly dominates (importance ≥ 2× the next).
    let mut v = Vec::new();
    for (name, groups) in &current {
        if groups.len() < 2 || is_native_script(name) || name.len() < 4 {
            continue;
        }
        let mut imp: Vec<(u32, usize)> = groups.iter().map(|&g| (importance(g), g)).collect();
        imp.sort_unstable_by(|a, b| b.cmp(a));
        if imp[0].0 >= 50 && imp[0].0 >= 2 * imp[1].0 {
            let display = by_group[&imp[0].1]
                .iter()
                .map(|r| r.name.as_str())
                .find(|n| fold(n) == *name && n.is_ascii())
                .unwrap_or(name)
                .to_string();
            let mut expect = expect_group(imp[0].1, 3);
            for &(_, g) in imp.iter().skip(1).take(4) {
                expect.extend(expect_group(g, 1));
            }
            v.push(Generated {
                class: "ambiguous",
                query: display,
                expect,
                max_rank: 1,
                rule: "shared name, dominant entity first",
            });
        }
    }
    take(v, 40, &mut rng, &mut out);

    // with_context: "Name, District" for shared names, unique within the district.
    let mut v = Vec::new();
    for (name, groups) in &current {
        if groups.len() < 2 || is_native_script(name) || name.len() < 4 {
            continue;
        }
        let mut per_district: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for &g in groups {
            let d = by_group[&g]
                .iter()
                .map(|r| fold(&r.district))
                .find(|d| !d.is_empty())
                .unwrap_or_default();
            per_district.entry(d).or_default().push(g);
        }
        for (d, gs) in &per_district {
            if gs.len() == 1 && !d.is_empty() && per_district.len() >= 2 {
                let g = gs[0];
                let r = by_group[&g]
                    .iter()
                    .find(|r| r.name.is_ascii())
                    .copied()
                    .unwrap_or(by_group[&g][0]);
                let dist = by_group[&g]
                    .iter()
                    .map(|r| r.district.clone())
                    .find(|x| !x.is_empty())
                    .unwrap_or_default();
                v.push(Generated {
                    class: "with_context",
                    query: format!("{}, {dist}", r.name),
                    expect: expect_group(g, 3),
                    max_rank: 1,
                    rule: "shared name, unique in its district",
                });
            }
        }
    }
    take(v, 60, &mut rng, &mut out);

    // historical: former units (GeoNames *H) whose name is unique everywhere.
    let mut v = Vec::new();
    for r in recs.iter().filter(|r| {
        !r.current
            && r.name.is_ascii()
            && r.name.len() >= 5
            && !r.name.chars().any(|c| c.is_ascii_digit())
    }) {
        if all.get(&fold(&r.name)).is_some_and(|g| g.len() == 1) {
            v.push(Generated {
                class: "historical",
                query: r.name.clone(),
                expect: vec![(r.key.clone(), 3)],
                max_rank: 3,
                rule: "unique former unit",
            });
        }
    }
    take(v, 40, &mut rng, &mut out);

    // variant and mixed_script, from the lexicon: a generic term at the end of a name, spelled
    // another way (variant) or in Devanagari (mixed_script).
    let mut variant = Vec::new();
    let mut mixed = Vec::new();
    for r in recs
        .iter()
        .filter(|r| r.current && r.name.is_ascii() && r.importance >= 10)
    {
        let lower = r.name.to_lowercase();
        for t in lex
            .iter()
            .filter(|t| t.kind == "generic" || t.kind == "admin_suffix")
        {
            let Some((stem_len, used)) = t
                .spellings()
                .filter(|s| s.len() >= 3)
                .filter_map(|s| {
                    lower
                        .strip_suffix(&s.to_lowercase())
                        .map(|stem| (stem.len(), s))
                })
                .max_by_key(|(_, s)| s.len())
            else {
                continue;
            };
            if stem_len < 3 || !unique_current(&r.name) {
                continue;
            }
            let stem = &r.name[..stem_len];
            for other in t.spellings().filter(|s| *s != used) {
                let q = format!(
                    "{stem}{}",
                    if stem.ends_with(' ') {
                        other.clone()
                    } else {
                        other.to_lowercase()
                    }
                );
                if !current.contains_key(&fold(&q)) {
                    variant.push(Generated {
                        class: "variant",
                        query: q,
                        expect: expect_group(r.group, 3),
                        max_rank: 5,
                        rule: "lexicon spelling swap",
                    });
                    break;
                }
            }
            if let Some(native) = t.native.first().map(|n| &n.text) {
                mixed.push(Generated {
                    class: "mixed_script",
                    query: format!("{}{native}", stem.trim_end()),
                    expect: expect_group(r.group, 3),
                    max_rank: 5,
                    rule: "Latin stem + native-script generic",
                });
            }
        }
    }
    take(variant, 45, &mut rng, &mut out);
    take(mixed, 35, &mut rng, &mut out);

    // abbreviation: "<name> <abbreviation>" for each unit type the profile gives one.
    let mut v = Vec::new();
    for r in &recs {
        let Some(list) = r.unit_type.as_deref().and_then(|u| abbreviations.get(u)) else {
            continue;
        };
        for abbr in list {
            v.push(Generated {
                class: "abbreviation",
                query: format!("{} {abbr}", r.name),
                expect: expect_group(r.group, 3),
                max_rank: 3,
                rule: "unit-type abbreviation from the profile",
            });
        }
    }
    take(v, 40, &mut rng, &mut out);

    // colloquial: a sourced short form. Either the source lists it as an alternate whose words
    // are a proper subset of the name's (Madhyapur Thimi → Thimi), or it is the name without
    // one of the profile's droppable suffixes. No guessed short forms.
    let mut v = Vec::new();
    let mut seen = BTreeSet::new();
    for (g, members) in &by_group {
        if importance(*g) < 30 || !members.iter().any(|r| r.current) {
            continue;
        }
        let mut shorts: Vec<String> = Vec::new();
        for r in members {
            let name_words: BTreeSet<String> =
                fold(&r.name).split(' ').map(str::to_string).collect();
            for alt in r.alts.iter().filter(|a| a.is_ascii()) {
                let words: BTreeSet<String> = fold(alt).split(' ').map(str::to_string).collect();
                if !words.is_empty()
                    && words.len() < name_words.len()
                    && words.is_subset(&name_words)
                {
                    shorts.push(alt.clone());
                }
            }
            let lower = r.name.to_lowercase();
            for d in &droppable {
                if let Some(stem) = lower.strip_suffix(d.as_str()) {
                    let stem = r.name[..stem.len()].trim_end().to_string();
                    if stem.len() >= 3 {
                        shorts.push(stem);
                    }
                }
            }
        }
        for q in shorts {
            let generic = lex
                .iter()
                .any(|t| t.spellings().any(|s| s.eq_ignore_ascii_case(&q)));
            // Fair only if no more important entity carries that short form.
            let rival = current.get(&fold(&q)).is_some_and(|gs| {
                gs.iter()
                    .any(|&o| o != *g && importance(o) >= importance(*g))
            });
            if !generic && !rival && seen.insert(fold(&q)) {
                v.push(Generated {
                    class: "colloquial",
                    query: q,
                    expect: expect_group(*g, 3),
                    max_rank: 5,
                    rule: "sourced short form",
                });
            }
        }
    }
    take(v, 35, &mut rng, &mut out);

    // Emit.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for q in &out {
        *counts.entry(q.class).or_default() += 1;
    }
    let mut y = String::new();
    let up = cc.to_uppercase();
    let _ = writeln!(
        y,
        "# Golden query set v0 for {up} (ADR-0032; names-and-search §5.7). GENERATED: do not edit."
    );
    let _ = writeln!(
        y,
        "# Regenerate: cargo run --release -p spike-s3-search --bin gen-golden -- {cc} \\"
    );
    let _ = writeln!(
        y,
        "#   <GeoNames {up}.txt> [<COD-AB workbook>] > tests/golden/{cc}.yaml"
    );
    let _ = writeln!(
        y,
        "# Inputs: {}",
        args[1..]
            .iter()
            .map(|a| a.rsplit('/').next().unwrap_or(a))
            .collect::<Vec<_>>()
            .join(", ")
    );
    y.push_str("# Derived from GeoNames (CC-BY 4.0, geonames.org) and, where used, OCHA COD-AB (licence per dataset).\n");
    y.push_str("# Keys are source keys until our stable IDs exist; a hit is any expected key with grade ≥ 2\n");
    let _ = writeln!(
        y,
        "# within max_rank. Hand-written queries come from data/countries/{cc}/golden-curated.yaml."
    );
    let _ = writeln!(
        y,
        "# Counts: {}",
        counts
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(y, "schema: teigeodata/golden/v0\ncountry: {up}\nqueries:");
    for (i, q) in out.iter().enumerate() {
        let _ = writeln!(y, "  - id: {cc}-{:04}", i + 1);
        let _ = writeln!(y, "    class: {}", q.class);
        let _ = writeln!(y, "    query: {}", json(&q.query));
        let _ = writeln!(
            y,
            "    expect: {{ {} }}",
            q.expect
                .iter()
                .map(|(k, g)| format!("{}: {g}", json(k)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(y, "    max_rank: {}", q.max_rank);
        let _ = writeln!(y, "    source: {}", json(&format!("generated: {}", q.rule)));
    }
    // Curated entries are already in the same shape; skip their header lines.
    for l in curated.lines().skip_while(|l| !l.starts_with("  - id:")) {
        let _ = writeln!(y, "{l}");
    }
    print!("{y}");
}

/// A YAML double-quoted scalar (JSON string syntax is valid YAML).
fn json(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
