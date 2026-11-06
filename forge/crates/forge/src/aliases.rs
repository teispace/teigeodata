//! The `aliases` stage: names that the other members of a conflation cluster know and the
//! anchor source's record does not. Every name of every member is carried to the cluster's
//! anchor record (for example the `GeoNames` record a search index is built on), unless the
//! anchor already has it after folding. This is how a Wikidata nickname or an OSM `short_name`
//! reaches search.
//!
//! Two rules keep source errors out, world-general:
//! - **Corroboration:** a member's names are carried only if at least one of them matches a
//!   name of the anchor (fold, join key or the profile's phonetic key). A member with no
//!   matching name (OSM node 303021945 at Lukla is named "नेपाल") is listed, not carried.
//! - **Collision:** an alias that is, by fold or phonetic key, the primary name of another
//!   anchor record within [`COLLISION_M`], or of an admin unit anywhere, names that record
//!   (Wikidata lists हेटौंडा, Hetauda, as an alias of Makwanpur District, whose seat it is; an
//!   OSM node at Lukla is named नेपाल), so it is not carried.
//!
//! Output, in `data/build/aliases/<conflate run>/`: `<anchor>.tsv`, sorted, one line per alias
//! (anchor ID, text, language, kind, the member it came from), and
//! `<anchor>-uncorroborated.txt`. TSV so that any tool, including the S3 spike, reads it
//! without a Parquet stack.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use arrow_array::StringArray;
use teigeodata_names::{PhoneticKey, fold, join_key};

use crate::links::haversine;
use crate::record::{self, Point, column};

/// An alias equal to the primary name of another anchor record this close names that record.
const COLLISION_M: f64 = 30_000.0;

/// (anchor record, member record).
type Member = (String, String);
/// (text, language, kind).
type NameRow = (String, String, String);
/// (anchor record, text, language, kind, member).
type Alias = (String, String, String, String, String);

/// The anchor source's records: their name keys, primary names and points.
struct Anchor {
    keys: HashMap<String, BTreeSet<String>>,
    primary: HashMap<String, Vec<String>>,
    points: HashMap<String, Point>,
    phonetic: Option<PhoneticKey>,
}

impl Anchor {
    /// The fold, join and phonetic keys of a name, tagged so they never collide.
    fn keys_of(&self, t: &str) -> [String; 3] {
        [
            format!("f:{}", fold(t)),
            format!("j:{}", join_key(t)),
            format!("p:{}", self.phonetic.map(|k| k.key(t)).unwrap_or_default()),
        ]
    }

    fn corroborates(&self, target: &str, names: &[NameRow]) -> bool {
        let Some(known) = self.keys.get(target) else {
            return false;
        };
        names.iter().any(|(t, _, _)| {
            self.keys_of(t)
                .iter()
                .any(|k| k.len() > 2 && known.contains(k))
        })
    }

    fn collides(&self, target: &str, text: &str) -> bool {
        let here = self.points.get(target);
        let [f, _, p] = self.keys_of(text);
        [f, p].iter().filter(|k| k.len() > 2).any(|k| {
            self.primary.get(k).is_some_and(|others| {
                others.iter().any(|o| {
                    o != target
                        && self.points.get(o).is_some_and(|b| {
                            b.kind == "admin" || here.is_some_and(|a| haversine(a, b) < COLLISION_M)
                        })
                })
            })
        })
    }
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
    let phonetic = crate::conflate::phonetic_key(root, cc)?
        .as_deref()
        .and_then(PhoneticKey::from_name);
    let a = load_anchor(&build, anchor, phonetic)?;
    let members = load_members(&build, anchor, run_name, &sources)?;
    let mut rows: BTreeSet<Alias> = BTreeSet::new();
    let (mut uncorroborated, mut collisions) = (Vec::new(), 0usize);
    for ((target, member), names) in &members {
        if !a.corroborates(target, names) {
            uncorroborated.push(format!("{member} → {target}"));
            continue;
        }
        for (text, lang, kind) in names {
            let folded = fold(text);
            let known = a
                .keys
                .get(target)
                .is_some_and(|k| k.contains(&format!("f:{folded}")));
            if folded.is_empty() || known {
                continue;
            }
            if a.collides(target, text) {
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

fn load_anchor(
    build: &Path,
    anchor: &str,
    phonetic: Option<PhoneticKey>,
) -> Result<Anchor, String> {
    let points = record::read_points(&build.join("read").join(anchor).join("records.parquet"))?
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let mut a = Anchor {
        keys: HashMap::new(),
        primary: HashMap::new(),
        points,
        phonetic,
    };
    let mut rows: Vec<(String, String, bool)> = Vec::new();
    for_each_name(build, anchor, |id, text, _, kind| {
        rows.push((id.to_string(), text.to_string(), kind == "name"));
    })?;
    for (id, text, is_primary) in rows {
        let keys = a.keys_of(&text);
        a.keys
            .entry(id.clone())
            .or_default()
            .extend(keys.into_iter().filter(|k| k.len() > 2));
        if is_primary {
            let [f, _, p] = a.keys_of(&text);
            for k in [f, p].into_iter().filter(|k| k.len() > 2) {
                a.primary.entry(k).or_default().push(id.clone());
            }
        }
    }
    Ok(a)
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
