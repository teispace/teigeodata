//! The `aliases` stage: names that the other members of a conflation cluster know and the
//! anchor source's record does not. Every name of every member is carried to the cluster's
//! anchor record (for example the `GeoNames` record a search index is built on), unless the
//! anchor already has it after folding. This is how a Wikidata nickname or an OSM `short_name`
//! reaches search.
//!
//! The rules of [`rules`] keep source errors out (D-060), the same ones the build applies:
//! a member lends names only when corroborated, and an alias that names an admin unit or
//! another anchor record nearby is not carried. A member with no matching name (OSM node
//! 303021945 at Lukla is named "नेपाल") is listed, not carried.
//!
//! Output, in `data/build/aliases/<conflate run>/`: `<anchor>.tsv`, sorted, one line per alias
//! (anchor ID, text, language, kind, the member it came from), and
//! `<anchor>-uncorroborated.txt`. TSV so that any tool, including the S3 spike, reads it
//! without a Parquet stack.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use arrow_array::StringArray;
use teigeodata_names::fold;

use crate::names::Namer;
use crate::record::{self, column};
use rules::{COLLISION_M, Rules};

pub(crate) mod rules;

/// (anchor record, member record).
type Member = (String, String);
/// (text, language, kind).
type NameRow = (String, String, String);
/// (anchor record, text, language, kind, member).
type Alias = (String, String, String, String, String);

/// The anchor source's records: their names, and the rules over their primary names.
struct Anchor<'a> {
    rules: Rules<'a>,
    /// Every name of a record.
    names: HashMap<String, Vec<String>>,
    /// A record's owner number in `rules` and its point.
    at: HashMap<String, (u32, (i32, i32))>,
}

/// `forge aliases <cc> <anchor-source> <conflated sources joined by +>`.
pub(crate) fn run(root: &Path, cc: &str, anchor: &str, run_name: &str) -> Result<String, String> {
    let build = root.join("data/build");
    let sources: Vec<&str> = run_name.split('+').collect();
    if !sources.contains(&anchor) {
        return Err(format!(
            "{anchor} is not one of the conflated sources {run_name}"
        ));
    }
    let profile = crate::build::Profile::read(root, cc)?;
    let namer = Namer::load(root, cc, &profile.romanise)?;
    let a = load_anchor(&build, anchor, Rules::new(profile.phonetic, Some(&namer)))?;
    let members = load_members(&build, anchor, run_name, &sources)?;
    let mut rows: BTreeSet<Alias> = BTreeSet::new();
    let (mut uncorroborated, mut collisions) = (Vec::new(), 0usize);
    let mut known: (String, HashSet<String>) = (String::new(), HashSet::new());
    for ((target, member), names) in &members {
        if known.0 != *target {
            let own = a
                .names
                .get(target)
                .into_iter()
                .flatten()
                .map(String::as_str);
            known = (target.clone(), a.rules.anchor(own));
        }
        let texts = names.iter().map(|(t, _, _)| t.as_str());
        if !a.rules.corroborates(&known.1, texts) {
            uncorroborated.push(format!("{member} → {target}"));
            continue;
        }
        let Some(&(owner, point)) = a.at.get(target) else {
            continue;
        };
        for (text, lang, kind) in names {
            // The anchor has it already.
            if fold(text).is_empty() || Rules::fold_known(&known.1, text) {
                continue;
            }
            if a.rules.collides(owner, point, text).is_some() {
                collisions += 1;
                continue;
            }
            rows.insert((
                target.clone(),
                text.clone(),
                lang.clone(),
                kind.clone(),
                member.clone(),
            ));
        }
    }
    let dir = build.join("aliases").join(run_name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{anchor}.tsv"));
    let by_kind = write_tsv(&path, &rows)?;
    let report = dir.join(format!("{anchor}-uncorroborated.txt"));
    fs::write(&report, uncorroborated.join("\n") + "\n").map_err(|e| e.to_string())?;
    let targets: BTreeSet<&String> = rows.iter().map(|r| &r.0).collect();
    Ok(format!(
        "{} aliases for {} {anchor} records ({by_kind}); {} members not carried (no name matches the anchor: {}); {collisions} aliases not carried (the primary name of an admin unit, or of another record within {} km) → {}",
        rows.len(),
        targets.len(),
        uncorroborated.len(),
        report.display(),
        COLLISION_M / 1000.0,
        path.display()
    ))
}

fn load_anchor<'a>(build: &Path, anchor: &str, mut rules: Rules<'a>) -> Result<Anchor<'a>, String> {
    let mut at = HashMap::new();
    let mut admin = HashSet::new();
    let records = record::read_points(&build.join("read").join(anchor).join("records.parquet"))?;
    for (owner, p) in (0_u32..).zip(records) {
        if p.kind == "admin" {
            admin.insert(p.id.clone());
        }
        at.insert(p.id, (owner, (p.lon, p.lat)));
    }
    let mut names: HashMap<String, Vec<String>> = HashMap::new();
    for_each_name(build, anchor, |id, text, _, kind| {
        if kind == "name" {
            if admin.contains(id) {
                rules.add_admin(text);
            } else if let Some(&(owner, point)) = at.get(id) {
                rules.add_place(owner, point, text);
            }
        }
        names
            .entry(id.to_string())
            .or_default()
            .push(text.to_string());
    })?;
    rules.finish();
    Ok(Anchor { rules, names, at })
}

/// Every non-anchor member's names, grouped by (anchor record, member).
fn load_members(
    build: &Path,
    anchor: &str,
    run_name: &str,
    sources: &[&str],
) -> Result<BTreeMap<Member, Vec<NameRow>>, String> {
    let anchor_ids = ids_of(&build.join("read").join(anchor).join("records.parquet"))?;
    let mut cluster_of: HashMap<String, String> = HashMap::new();
    let mut anchor_of: HashMap<String, String> = HashMap::new();
    record::for_each_batch(
        &build
            .join("conflate")
            .join(run_name)
            .join("clusters.parquet"),
        |b| {
            let (ids, clusters) = (
                column::<StringArray>(b, "id")?,
                column::<StringArray>(b, "cluster")?,
            );
            for i in 0..b.num_rows() {
                let (id, c) = (ids.value(i), clusters.value(i));
                if anchor_ids.contains(id) {
                    anchor_of.insert(c.to_string(), id.to_string());
                }
                cluster_of.insert(id.to_string(), c.to_string());
            }
            Ok(())
        },
    )?;
    let mut members: BTreeMap<Member, Vec<NameRow>> = BTreeMap::new();
    for source in sources.iter().filter(|s| **s != anchor) {
        for_each_name(build, source, |id, text, lang, kind| {
            if let Some(target) = cluster_of.get(id).and_then(|c| anchor_of.get(c)) {
                members
                    .entry((target.clone(), id.to_string()))
                    .or_default()
                    .push((text.to_string(), lang.to_string(), kind.to_string()));
            }
        })?;
    }
    Ok(members)
}

/// Writes the aliases; returns the count per kind, as text.
fn write_tsv(path: &Path, rows: &BTreeSet<Alias>) -> Result<String, String> {
    let mut out = String::from("anchor\ttext\tlang\tkind\tfrom\n");
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for (a, t, l, k, f) in rows {
        // A tab or newline inside a name would break the format.
        let _ = writeln!(
            out,
            "{a}\t{}\t{l}\t{k}\t{f}",
            t.replace(['\t', '\n', '\r'], " ")
        );
        *by_kind.entry(k.as_str()).or_default() += 1;
    }
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(by_kind
        .iter()
        .map(|(k, n)| format!("{k} {n}"))
        .collect::<Vec<_>>()
        .join(", "))
}

fn ids_of(path: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    record::for_each_batch(path, |b| {
        let ids = column::<StringArray>(b, "id")?;
        for i in 0..b.num_rows() {
            out.insert(ids.value(i).to_string());
        }
        Ok(())
    })?;
    Ok(out)
}

fn for_each_name(
    build: &Path,
    source: &str,
    mut f: impl FnMut(&str, &str, &str, &str),
) -> Result<(), String> {
    record::for_each_batch(
        &build.join("read").join(source).join("names.parquet"),
        |b| {
            let (ids, texts, langs, kinds) = (
                column::<StringArray>(b, "id")?,
                column::<StringArray>(b, "text")?,
                column::<StringArray>(b, "lang")?,
                column::<StringArray>(b, "kind")?,
            );
            for i in 0..b.num_rows() {
                f(ids.value(i), texts.value(i), langs.value(i), kinds.value(i));
            }
            Ok(())
        },
    )
}
