//! The reviewed tier, the top of the names chain (D-359,
//! [research](../../../../../docs/02-research/notes/reviewed-names.md)): names a person read
//! against the sources and corrected.
//!
//! `data/countries/<cc>/reviewed-names.tsv` (P16: countries are data) holds one row per name,
//! made from a reviewer's table by `forge reviewed-import`. A row names the place by the source
//! keys behind the stable ID it was reviewed under, since IDs are numbered per build (lite's
//! `np/1617` is not main's) and source keys are what every build shares; it carries the text it
//! replaces (the guard), the corrected text, the fault, the reviewer's confidence, its sources
//! and their licence class, whether a source is an official list, and a second reading.
//!
//! Each build gives each row one outcome:
//!
//! - **applied:** the place holds the guarded name in the row's language; the corrected text
//!   replaces it, marked [`Origin::Reviewed`] and placed right after the canonical name, so a
//!   reader asking for the language takes it first. An English row replaces the canonical name
//!   only where an official list writes it so; otherwise the corrected spelling is added as an
//!   official name and the canonical one stays. A replaced canonical name is kept as an
//!   alternate, so a search for the old spelling still finds the place;
//! - **already:** the corrected text is there and the guarded one is not (fixed upstream);
//! - **held:** a `likely` row on the reviewer's judgement alone, or any `uncertain` row, until a
//!   second reading is recorded in its `review` column;
//! - **refused:** no source of a class this build may read (`CLEAN_ROOM.md` rule 3, ADR-0030:
//!   lite reads attribution-only classes, main also `ODbL`, neither share-alike);
//! - **absent:** no place of this build holds any of its keys;
//! - **stale:** the place holds neither text. In the build the table was reviewed against (its
//!   `# build:` line) a stale row fails the build, so a changed source is read again instead of
//!   being overwritten silently (gate G-reviewed); in another build, whose names come from other
//!   sources, it is counted.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::str::FromStr as _;

use teigeodata_core::PlaceId;
use teigeodata_format::names::NameType;

use super::{LITE, Origin, Place, PlaceName, ids};

/// The table's file name in a country's directory.
const FILE: &str = "reviewed-names.tsv";
/// The source classification's file name in a country's directory.
const SOURCES: &str = "reviewed-sources.yaml";
const HEADER: &str = "# teigeodata reviewed names 1";
const COLUMNS: [&str; 12] = [
    "keys",
    "id",
    "kind",
    "lang",
    "current",
    "corrected",
    "fault",
    "confidence",
    "class",
    "official",
    "source",
    "review",
];
/// A reviewer's verdict for a row resting on no document.
const JUDGEMENT: &str = "judgement";
/// Between spellings a reviewer could not choose from (`A | B`): held until a second reading
/// picks one.
const ALTERNATIVES: &str = " | ";

/// How sure the reviewer was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confidence {
    Sure,
    Likely,
    Uncertain,
}

impl Confidence {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "sure" => Ok(Self::Sure),
            "likely" => Ok(Self::Likely),
            "uncertain" => Ok(Self::Uncertain),
            o => Err(format!("confidence {o:?} is not sure, likely or uncertain")),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Sure => "sure",
            Self::Likely => "likely",
            Self::Uncertain => "uncertain",
        }
    }
}

/// One reviewed name.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    /// The source keys behind the ID it was reviewed under.
    keys: Vec<String>,
    /// That ID, for reading the row back against the reviewer's table.
    id: String,
    kind: String,
    /// BCP 47 primary subtag.
    lang: String,
    current: String,
    corrected: String,
    fault: String,
    confidence: Confidence,
    /// The licence class of its best source: a lite-clean one when it has one.
    class: String,
    /// Whether a source is an official list.
    official: bool,
    source: String,
    /// A second reading: who and when (`name 2026-11-16`), or empty.
    review: String,
}

impl Row {
    /// A row is applied unless it waits on a second reading.
    fn waits(&self) -> bool {
        self.review.is_empty()
            && (self.corrected.contains(ALTERNATIVES)
                || match self.confidence {
                    Confidence::Sure => false,
                    Confidence::Likely => self.source.trim() == JUDGEMENT,
                    Confidence::Uncertain => true,
                })
    }

    fn render(&self, out: &mut String) {
        let official = if self.official { "yes" } else { "" };
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{official}\t{}\t{}",
            self.keys.join(" "),
            self.id,
            self.kind,
            self.lang,
            self.current,
            self.corrected,
            self.fault,
            self.confidence.name(),
            self.class,
            self.source,
            self.review
        );
    }
}

/// A country's table: the build its rows were reviewed against, and the rows.
#[derive(Debug, Default, PartialEq, Eq)]
struct Table {
    build: String,
    rows: Vec<Row>,
}

/// One field: no tab, no line break, no leading or trailing space.
fn field(s: &str, what: &str) -> Result<String, String> {
    if s.contains(['\t', '\n', '\r']) || s.trim() != s {
        return Err(format!(
            "{what} {s:?} has a tab, a line break or outer spaces"
        ));
    }
    Ok(s.to_string())
}

impl Table {
    fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().enumerate();
        if lines.next().map(|(_, l)| l) != Some(HEADER) {
            return Err(format!("the first line must be {HEADER:?}"));
        }
        let build = lines
            .next()
            .and_then(|(_, l)| l.strip_prefix("# build: "))
            .filter(|b| !b.is_empty())
            .ok_or("the second line must be \"# build: <build>\"")?
            .to_string();
        if lines.next().map(|(_, l)| l) != Some(COLUMNS.join("\t").as_str()) {
            return Err("the third line must name the columns".into());
        }
        let mut rows = Vec::new();
        let mut seen = BTreeSet::new();
        for (n, line) in lines {
            let at = |e: String| format!("line {}: {e}", n + 1);
            let c: Vec<&str> = line.split('\t').collect();
            let [
                keys,
                id,
                kind,
                lang,
                current,
                corrected,
                fault,
                confidence,
                class,
                official,
                source,
                review,
            ] = c.as_slice()
            else {
                return Err(at(format!("{} columns, not {}", c.len(), COLUMNS.len())));
            };
            let keys: Vec<String> = keys.split(' ').map(str::to_string).collect();
            if keys.iter().any(|k| !k.contains(':')) {
                return Err(at("a key is not `source:id`".into()));
            }
            if current == corrected || corrected.is_empty() || current.is_empty() {
                return Err(at(
                    "the corrected text must differ from the current and neither be empty".into(),
                ));
            }
            if !review.is_empty() && corrected.contains(ALTERNATIVES) {
                return Err(at("a second reading picks one spelling, not several".into()));
            }
            if !matches!(*official, "" | "yes") {
                return Err(at(format!("official {official:?} is not empty or yes")));
            }
            if !seen.insert((keys.clone(), (*lang).to_string(), (*current).to_string())) {
                return Err(at(
                    "a second row for the same place, language and text".into()
                ));
            }
            rows.push(Row {
                id: field(id, "id").map_err(at)?,
                kind: field(kind, "kind").map_err(at)?,
                lang: field(lang, "lang").map_err(at)?,
                current: field(current, "current").map_err(at)?,
                corrected: field(corrected, "corrected").map_err(at)?,
                fault: field(fault, "fault").map_err(at)?,
                confidence: Confidence::parse(confidence).map_err(at)?,
                class: field(class, "class").map_err(at)?,
                official: *official == "yes",
                source: field(source, "source").map_err(at)?,
                review: field(review, "review").map_err(at)?,
                keys,
            });
        }
        Ok(Self { build, rows })
    }

    fn render(&self) -> String {
        let mut out = format!(
            "{HEADER}\n# build: {}\n{}\n",
            self.build,
            COLUMNS.join("\t")
        );
        for r in &self.rows {
            r.render(&mut out);
        }
        out
    }
}

/// A country's table, or none.
fn load(root: &Path, cc: &str) -> Result<Option<Table>, String> {
    let path = root.join("data/countries").join(cc).join(FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Table::parse(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// What a row did in a build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Applied,
    Already,
    Held,
    /// Its sources' licence class is one this build may not read (`CLEAN_ROOM.md`, ADR-0030).
    Refused,
    Absent,
    Stale,
}

/// The place holding any of the keys: the one holding most of them.
fn holder(by_key: &BTreeMap<&str, usize>, keys: &[String]) -> Option<usize> {
    let mut count: BTreeMap<usize, usize> = BTreeMap::new();
    for k in keys {
        if let Some(&i) = by_key.get(k.as_str()) {
            *count.entry(i).or_default() += 1;
        }
    }
    count
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

/// Whether a name is in the row's language: its own, or none stated.
fn in_lang(n: &PlaceName, lang: &str) -> bool {
    n.lang.as_deref().is_none_or(|l| l == lang)
}

/// Applies one row to its place.
fn one(r: &Row, p: &mut Place) -> Outcome {
    let has = |p: &Place, t: &str| {
        p.names
            .iter()
            .position(|n| n.text == t && in_lang(n, &r.lang))
    };
    let Some(at) = has(p, &r.current) else {
        return if has(p, &r.corrected).is_some() {
            Outcome::Already
        } else {
            Outcome::Stale
        };
    };
    let reviewed = |kind| PlaceName {
        text: r.corrected.clone(),
        lang: Some(r.lang.clone()),
        kind,
        origin: Origin::Reviewed,
    };
    if at == 0 {
        if r.lang != "en" || r.official {
            // The canonical name itself: replaced, the old spelling kept for search.
            let old = p.names.remove(0);
            p.names
                .retain(|n| !(n.text == r.corrected && in_lang(n, &r.lang)));
            p.names.insert(0, reviewed(None));
            p.names.push(PlaceName {
                kind: Some(NameType::Alt),
                ..old
            });
        } else {
            p.names
                .retain(|n| !(n.text == r.corrected && in_lang(n, &r.lang)));
            p.names
                .insert(1.min(p.names.len()), reviewed(Some(NameType::Official)));
        }
        return Outcome::Applied;
    }
    let kind = p.names.remove(at).kind;
    p.names
        .retain(|n| !(n.text == r.corrected && in_lang(n, &r.lang)));
    p.names.insert(1.min(p.names.len()), reviewed(kind));
    Outcome::Applied
}

/// The tier, run after every other: each row's outcome, and the summary.
pub(super) fn run(
    root: &Path,
    cc: &str,
    build: &str,
    places: &mut [Place],
) -> Result<(String, Vec<String>), String> {
    let Some(table) = load(root, cc)? else {
        return Ok((String::new(), Vec::new()));
    };
    let sources = read_sources(root, cc)?
        .ok_or_else(|| format!("{cc}: {FILE} needs {SOURCES} to credit its sources"))?;
    let by_key: BTreeMap<&str, usize> = places
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            std::iter::once((p.key.as_str(), i))
                .chain(p.sources.iter().map(move |s| (s.as_str(), i)))
        })
        .collect();
    let lite = build == "lite";
    // Lite reads attribution-only classes; main also ODbL; neither reads share-alike,
    // non-commercial or proprietary evidence (ADR-0030).
    let admitted = |class: &str| LITE.contains(&class) || (!lite && class == "odbl");
    let mut plan = Vec::with_capacity(table.rows.len());
    for r in &table.rows {
        let outcome = if r.waits() {
            Err(Outcome::Held)
        } else if !admitted(&r.class) {
            Err(Outcome::Refused)
        } else {
            holder(&by_key, &r.keys).ok_or(Outcome::Absent)
        };
        plan.push(outcome);
    }
    let mut outcomes: BTreeMap<Outcome, Vec<&Row>> = BTreeMap::new();
    for (r, o) in table.rows.iter().zip(plan) {
        let o = match o {
            Ok(i) => places.get_mut(i).map_or(Outcome::Absent, |p| one(r, p)),
            Err(o) => o,
        };
        outcomes.entry(o).or_default().push(r);
    }
    let stale = outcomes.get(&Outcome::Stale).map_or(&[][..], Vec::as_slice);
    if table.build == build && !stale.is_empty() {
        let mut e = format!(
            "G-reviewed: {} reviewed names no longer match the {build} build's names (read them again):",
            stale.len()
        );
        for r in stale.iter().take(20) {
            let _ = write!(
                e,
                "\n  {} {} {:?} -> {:?}",
                r.id, r.lang, r.current, r.corrected
            );
        }
        return Err(e);
    }
    // Every applied name's sources are credited in the pack's notices (D-360).
    let mut credited: Vec<String> = Vec::new();
    for r in outcomes.get(&Outcome::Applied).into_iter().flatten() {
        for c in credits(&sources, &r.source) {
            if !credited.iter().any(|k| k == c) {
                credited.push(c.to_owned());
            }
        }
    }
    Ok((summary(&table, build, &outcomes), credited))
}

fn summary(table: &Table, build: &str, outcomes: &BTreeMap<Outcome, Vec<&Row>>) -> String {
    let n = |o| outcomes.get(&o).map_or(0, Vec::len);
    let mut out = format!(
        "Reviewed names (D-359), {} rows reviewed against {}: {} applied, {} already right, {} held \
         for a second reading, {} from sources this build may not read, {} on places not in this build, {} stale.\n",
        table.rows.len(),
        table.build,
        n(Outcome::Applied),
        n(Outcome::Already),
        n(Outcome::Held),
        n(Outcome::Refused),
        n(Outcome::Absent),
        n(Outcome::Stale),
    );
    let _ = build;
    // The reviewer's error rate before and after, by kind: wrong names they found among those
    // they read, and how many remain wrong after this build.
    let mut kinds: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (o, rows) in outcomes {
        for r in rows {
            let e = kinds.entry(r.kind.as_str()).or_default();
            e.0 += 1;
            if !matches!(o, Outcome::Applied | Outcome::Already) {
                e.1 += 1;
            }
        }
    }
    let _ = writeln!(
        out,
        "| kind | wrong as reviewed | still wrong |\n|---|---:|---:|"
    );
    for (k, (all, left)) in kinds {
        let _ = writeln!(out, "| {k} | {all} | {left} |");
    }
    out
}

/// A source's licence class and whether it is an official list.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    /// A URL prefix, or `judgement` for the reviewer's own reading.
    prefix: String,
    class: String,
    #[serde(default)]
    official: bool,
    /// The notice a pack carries when a name from this source is applied: required for every
    /// class but public domain and CC0 (D-360).
    #[serde(default)]
    credit: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    schema: String,
    /// The licence class of a reviewer's own reading (the review's own licence).
    judgement: String,
    #[serde(rename = "sources")]
    list: Vec<Source>,
}

/// The country's source classification, checked: its schema, and a credit for every source
/// whose class asks for one. `None` when the country has none.
fn read_sources(root: &Path, cc: &str) -> Result<Option<Sources>, String> {
    let path = root.join("data/countries").join(cc).join(SOURCES);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let s: Sources =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if s.schema != "teigeodata/reviewed-sources/v1" {
        return Err(format!(
            "{}: schema must be teigeodata/reviewed-sources/v1",
            path.display()
        ));
    }
    for k in &s.list {
        let free = matches!(k.class.as_str(), "public-domain" | "cc0");
        let credit = k.credit.as_deref().map(str::trim).unwrap_or_default();
        if !free && credit.is_empty() {
            return Err(format!(
                "{}: {} is {}, so it needs a credit",
                path.display(),
                k.prefix,
                k.class
            ));
        }
        if credit.contains('\n') {
            return Err(format!(
                "{}: {}: a credit is one line",
                path.display(),
                k.prefix
            ));
        }
    }
    Ok(Some(s))
}

/// The credits of a row's sources, in order.
fn credits<'a>(s: &'a Sources, source: &str) -> impl Iterator<Item = &'a str> {
    source
        .split(" ; ")
        .map(str::trim)
        .filter_map(|one| s.list.iter().find(|k| one.starts_with(&k.prefix)))
        .filter_map(|k| k.credit.as_deref().map(str::trim))
}

/// The class of a row's sources (` ; `-separated): the first lite-clean one, else `odbl`,
/// else the first;
/// and whether any is an official list. A source no prefix covers is refused, so every source
/// is classified once by a person.
fn classify(s: &Sources, source: &str) -> Result<(String, bool), String> {
    let mut classes = Vec::new();
    let mut official = false;
    for one in source.split(" ; ").map(str::trim) {
        if one == JUDGEMENT {
            classes.push(s.judgement.clone());
            continue;
        }
        let k = s
            .list
            .iter()
            .find(|k| one.starts_with(&k.prefix))
            .ok_or_else(|| format!("no class for the source {one:?} (add it to {SOURCES})"))?;
        classes.push(k.class.clone());
        official |= k.official;
    }
    let class = classes
        .iter()
        .find(|c| LITE.contains(&c.as_str()))
        .or_else(|| classes.iter().find(|c| *c == "odbl"))
        .or_else(|| classes.first())
        .ok_or("a row with no source")?
        .clone();
    Ok((class, official))
}

/// `forge reviewed-import <cc> <reviewer.tsv> --ids <registry.tsv> --build <build>`: the
/// reviewer's table (`place_id kind origin lang current corrected fault confidence source`,
/// keyed by the stable IDs of `<build>`) as the country's reviewed names. Each ID is resolved to
/// the source keys the registry records for it, following retirements; a tombstone or an
/// unknown ID is refused. Rows of other countries' namespaces are left out and counted. A row
/// already in the table keeps its `review`.
pub(crate) fn import(root: &Path, args: &[String]) -> Result<String, String> {
    let usage =
        "usage: forge reviewed-import <cc> <reviewer.tsv> --ids <registry.tsv> --build <build>";
    let (cc, input) = match args {
        [cc, input, ..] => (cc.as_str(), input.as_str()),
        _ => return Err(usage.into()),
    };
    let flag = |name: &str| {
        args.windows(2)
            .find_map(|w| match w {
                [k, v] if k == name => Some(v.clone()),
                _ => None,
            })
            .ok_or_else(|| usage.to_string())
    };
    let (ids_path, build) = (flag("--ids")?, flag("--build")?);
    let registry = ids::Registry::parse(
        &fs::read_to_string(&ids_path).map_err(|e| format!("{ids_path}: {e}"))?,
    )
    .map_err(|e| format!("{ids_path}: {e}"))?;
    let dir = root.join("data/countries").join(cc);
    let sources = read_sources(root, cc)?
        .ok_or_else(|| format!("{}: missing", dir.join(SOURCES).display()))?;
    let text = fs::read_to_string(input).map_err(|e| format!("{input}: {e}"))?;
    let kept: BTreeMap<(String, String, String), String> = load(root, cc)?
        .map(|t| t.rows)
        .unwrap_or_default()
        .into_iter()
        .map(|r| ((r.id, r.lang, r.current), r.review))
        .collect();
    let (rows, abroad) = rows_of(&text, cc, &registry, &sources, &kept)?;
    let table = Table { build, rows };
    let path = dir.join(FILE);
    fs::write(&path, table.render()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!(
        "{}: {} reviewed names of {cc}; {abroad} rows of other countries left out\n",
        path.display(),
        table.rows.len()
    ))
}

/// The reviewer's rows of `cc` as table rows, and the count of rows of other countries.
fn rows_of(
    text: &str,
    cc: &str,
    registry: &ids::Registry,
    sources: &Sources,
    kept: &BTreeMap<(String, String, String), String>,
) -> Result<(Vec<Row>, usize), String> {
    let mut lines = text.lines().enumerate();
    let head = "place_id\tkind\torigin\tlang\tcurrent\tcorrected\tfault\tconfidence\tsource";
    if lines.next().map(|(_, l)| l) != Some(head) {
        return Err(format!("the reviewer's table must start with {head:?}"));
    }
    let prefix = format!("tgd:{cc}/");
    let mut rows = Vec::new();
    let mut abroad = 0;
    for (n, line) in lines {
        let at = |e: String| format!("line {}: {e}", n + 1);
        let c: Vec<&str> = line.split('\t').collect();
        let [
            id,
            kind,
            _origin,
            lang,
            current,
            corrected,
            fault,
            confidence,
            source,
        ] = c.as_slice()
        else {
            return Err(at(format!("{} columns, not 9", c.len())));
        };
        if !id.starts_with(&prefix) {
            abroad += 1;
            continue;
        }
        let serial = PlaceId::from_str(id)
            .map_err(|e| at(format!("{id}: {e}")))?
            .serial();
        let keys = resolve(registry, serial).map_err(|e| at(format!("{id}: {e}")))?;
        let (class, official) = classify(sources, source).map_err(at)?;
        let review = kept
            .get(&(
                (*id).to_string(),
                (*lang).to_string(),
                (*current).to_string(),
            ))
            .cloned()
            .unwrap_or_default();
        rows.push(Row {
            keys,
            id: (*id).to_string(),
            kind: (*kind).to_string(),
            lang: (*lang).to_string(),
            current: (*current).to_string(),
            corrected: (*corrected).to_string(),
            fault: (*fault).to_string(),
            confidence: Confidence::parse(confidence).map_err(at)?,
            class,
            official,
            source: (*source).to_string(),
            review,
        });
    }
    Ok((rows, abroad))
}

/// The source keys of a serial, following retirements to the live serial.
fn resolve(registry: &ids::Registry, serial: u64) -> Result<Vec<String>, String> {
    let mut s = serial;
    // Retirements chain a few steps at most; the bound stops a cycle a bad file might make.
    for _ in 0..16 {
        let e = registry
            .entries
            .get(&s)
            .ok_or_else(|| format!("serial {s} is not in the registry"))?;
        match &e.state {
            ids::State::Live(_) => return Ok(e.keys.iter().cloned().collect()),
            ids::State::To(next) => s = *next,
            ids::State::Gone => return Err(format!("serial {s} is a tombstone")),
        }
    }
    Err(format!("serial {serial}: retirements do not end"))
}

#[cfg(test)]
mod tests {
    use teigeodata_core::PlaceKind;

    use super::*;

    fn place(key: &str, names: &[(&str, Option<&str>)]) -> Place {
        Place {
            key: key.into(),
            kind: PlaceKind::Settlement,
            admin_level: 0,
            flags: 0,
            point: (853_000_000, 277_000_000),
            names: names
                .iter()
                .map(|(t, l)| PlaceName {
                    text: (*t).into(),
                    lang: l.map(str::to_string),
                    kind: None,
                    origin: Origin::Unknown,
                })
                .collect(),
            population: None,
            rank: 0,
            parent: None,
            sources: vec![format!("src:{key}")],
        }
    }

    fn row(
        lang: &str,
        current: &str,
        corrected: &str,
        confidence: Confidence,
        source: &str,
    ) -> Row {
        Row {
            keys: vec!["src:a".into()],
            id: "tgd:np/1".into(),
            kind: "settlement".into(),
            lang: lang.into(),
            current: current.into(),
            corrected: corrected.into(),
            fault: "typo".into(),
            confidence,
            class: "cc-by".into(),
            official: false,
            source: source.into(),
            review: String::new(),
        }
    }

    fn table(build: &str, rows: Vec<Row>) -> String {
        Table {
            build: build.into(),
            rows,
        }
        .render()
    }

    const TEST_SOURCES: &str = "schema: teigeodata/reviewed-sources/v1\njudgement: cc0\nsources:\n  - prefix: \"https://a\"\n    class: government\n    official: true\n    credit: \"A ministry, https://a\"\n";

    fn run_on(text: &str, build: &str, places: &mut [Place]) -> Result<String, String> {
        run_with(text, TEST_SOURCES, build, places).map(|(summary, _)| summary)
    }

    fn run_with(
        text: &str,
        sources: &str,
        build: &str,
        places: &mut [Place],
    ) -> Result<(String, Vec<String>), String> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tg-reviewed-{}-{n}", std::process::id()));
        let cc = dir.join("data/countries/xx");
        fs::create_dir_all(&cc).unwrap();
        fs::write(cc.join(FILE), text).unwrap();
        fs::write(cc.join(SOURCES), sources).unwrap();
        let out = run(&dir, "xx", build, places);
        let _ = fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn a_guarded_name_is_replaced_and_placed_first() {
        let mut ps = vec![place(
            "a",
            &[("Lahachok", None), ("लहचोक", Some("ne")), ("x", Some("ne"))],
        )];
        let t = table(
            "lite",
            vec![row("ne", "लहचोक", "लहाचोक", Confidence::Sure, "https://a")],
        );
        let out = run_on(&t, "lite", &mut ps).unwrap();
        assert!(out.contains("1 applied"), "{out}");
        assert_eq!(ps[0].names[1].text, "लहाचोक");
        assert_eq!(ps[0].names[1].origin, Origin::Reviewed);
        assert!(!ps[0].names.iter().any(|n| n.text == "लहचोक"));
    }

    #[test]
    fn a_stale_guard_fails_the_reviewed_build_only() {
        let rows = vec![row("ne", "गलत", "सही", Confidence::Sure, "https://a")];
        let mut ps = vec![place("a", &[("Sahi", None), ("अर्को", Some("ne"))])];
        let e = run_on(&table("lite", rows.clone()), "lite", &mut ps).unwrap_err();
        assert!(e.starts_with("G-reviewed"), "{e}");
        let out = run_on(&table("lite", rows), "main", &mut ps).unwrap();
        assert!(out.contains("1 stale"), "{out}");
    }

    #[test]
    fn judgement_and_uncertain_rows_wait_for_a_second_reading() {
        let mut held = vec![
            row("ne", "क", "ख", Confidence::Likely, JUDGEMENT),
            row("ne", "ग", "घ", Confidence::Uncertain, "https://a"),
        ];
        held[1].keys = vec!["src:a".into(), "other:1".into()];
        let mut ps = vec![place(
            "a",
            &[("Ka", None), ("क", Some("ne")), ("ग", Some("ne"))],
        )];
        let out = run_on(&table("lite", held.clone()), "lite", &mut ps).unwrap();
        assert!(out.contains("2 held"), "{out}");
        held[0].review = "reader 2026-11-16".into();
        let out = run_on(&table("lite", held), "lite", &mut ps).unwrap();
        assert!(out.contains("1 applied") && out.contains("1 held"), "{out}");
    }

    #[test]
    fn english_replaces_the_canonical_name_only_from_an_official_list() {
        let mut ps = vec![place("a", &[("Chitawan", None)])];
        let r = row("en", "Chitawan", "Chitwan", Confidence::Sure, "https://a");
        run_on(&table("lite", vec![r.clone()]), "lite", &mut ps).unwrap();
        assert_eq!(ps[0].names[0].text, "Chitawan");
        assert_eq!(ps[0].names[1].text, "Chitwan");

        let mut ps = vec![place("a", &[("Chitawan", None)])];
        let official = Row {
            official: true,
            ..r
        };
        run_on(&table("lite", vec![official]), "lite", &mut ps).unwrap();
        assert_eq!(ps[0].names[0].text, "Chitwan");
        assert_eq!(ps[0].names[0].origin, Origin::Reviewed);
        let old = ps[0].names.last().unwrap();
        assert_eq!(
            (old.text.as_str(), old.kind),
            ("Chitawan", Some(NameType::Alt))
        );
    }

    #[test]
    fn builds_skip_rows_from_sources_they_may_not_read_and_count_absent_places() {
        let mut r = row("ne", "क", "ख", Confidence::Sure, "https://a");
        r.class = "share-alike".into();
        let mut ps = vec![place("a", &[("Ka", None), ("क", Some("ne"))])];
        let out = run_on(&table("lite", vec![r.clone()]), "lite", &mut ps).unwrap();
        assert!(
            out.contains("1 from sources this build may not read"),
            "{out}"
        );
        // Main refuses share-alike evidence too, and reads ODbL.
        let out = run_on(&table("lite", vec![r.clone()]), "main", &mut ps).unwrap();
        assert!(
            out.contains("1 from sources this build may not read"),
            "{out}"
        );
        r.class = "odbl".into();
        let out = run_on(&table("lite", vec![r.clone()]), "lite", &mut ps).unwrap();
        assert!(
            out.contains("1 from sources this build may not read"),
            "{out}"
        );
        let mut other = vec![place("b", &[("Kha", None)])];
        let out = run_on(&table("lite", vec![r]), "main", &mut other).unwrap();
        assert!(out.contains("1 on places not in this build"), "{out}");
    }

    #[test]
    fn alternatives_wait_until_one_is_picked() {
        let mut r = row("ne", "क", "ख | ग", Confidence::Sure, "https://a");
        let mut ps = vec![place("a", &[("Ka", None), ("क", Some("ne"))])];
        let out = run_on(&table("lite", vec![r.clone()]), "lite", &mut ps).unwrap();
        assert!(out.contains("1 held"), "{out}");
        r.review = "reader 2026-11-16".into();
        assert!(Table::parse(&table("lite", vec![r])).is_err());
    }

    #[test]
    fn already_right_is_not_stale() {
        let mut ps = vec![place("a", &[("Kha", None), ("ख", Some("ne"))])];
        let t = table(
            "lite",
            vec![row("ne", "क", "ख", Confidence::Sure, "https://a")],
        );
        let out = run_on(&t, "lite", &mut ps).unwrap();
        assert!(out.contains("1 already right"), "{out}");
    }

    #[test]
    fn the_table_round_trips_and_bad_rows_are_refused() {
        let t = table(
            "lite",
            vec![row("ne", "क", "ख", Confidence::Sure, "https://a")],
        );
        assert_eq!(Table::parse(&t).unwrap().render(), t);
        let same = t.replace("\tख\t", "\tक\t");
        assert!(Table::parse(&same).is_err());
        assert!(Table::parse(&t.replacen("# build: lite", "# build:", 1)).is_err());
        let dup = format!("{t}{}", t.lines().last().unwrap());
        assert!(Table::parse(&dup).is_err());
    }

    #[test]
    fn ids_resolve_through_retirements_and_tombstones_are_refused() {
        let reg = ids::Registry::parse(
            "# teigeodata id registry 1\n1\tlite\tgeonames:1 wikidata:Q1\n2\t>1\tgeonames:2\n3\tx\tgeonames:3\n",
        )
        .unwrap();
        assert_eq!(resolve(&reg, 2).unwrap(), ["geonames:1", "wikidata:Q1"]);
        assert!(resolve(&reg, 3).is_err());
        assert!(resolve(&reg, 9).is_err());
    }

    #[test]
    fn sources_are_classified_and_unknown_ones_refused() {
        let s = Sources {
            schema: String::new(),
            judgement: "cc0".into(),
            list: vec![
                Source {
                    prefix: "https://wiki/".into(),
                    class: "share-alike".into(),
                    official: false,
                    credit: None,
                },
                Source {
                    prefix: "https://gov/".into(),
                    class: "cc-by".into(),
                    official: true,
                    credit: Some("Gov".into()),
                },
            ],
        };
        assert_eq!(
            classify(&s, "https://wiki/x ; https://gov/y").unwrap(),
            ("cc-by".into(), true)
        );
        assert_eq!(
            classify(&s, "https://wiki/x").unwrap(),
            ("share-alike".into(), false)
        );
        assert_eq!(classify(&s, JUDGEMENT).unwrap(), ("cc0".into(), false));
        assert!(classify(&s, "https://elsewhere/").is_err());
    }

    #[test]
    fn applied_names_credit_their_sources_and_uncredited_classes_are_refused() {
        let mut ps = vec![place("a", &[("Lahachok", None), ("लहचोक", Some("ne"))])];
        let t = table(
            "lite",
            vec![row(
                "ne",
                "लहचोक",
                "लहाचोक",
                Confidence::Sure,
                "https://a/x",
            )],
        );
        let (_, credits) = run_with(&t, TEST_SOURCES, "lite", &mut ps).unwrap();
        assert_eq!(credits, vec!["A ministry, https://a".to_owned()]);
        // A held row credits nothing.
        let mut ps = vec![place("a", &[("Lahachok", None), ("लहचोक", Some("ne"))])];
        let held = table(
            "lite",
            vec![row(
                "ne",
                "लहचोक",
                "लहाचोक",
                Confidence::Uncertain,
                "https://a/x",
            )],
        );
        let (_, none) = run_with(&held, TEST_SOURCES, "lite", &mut ps).unwrap();
        assert_eq!(none, Vec::<String>::new());
        let bare = TEST_SOURCES.replace("    credit: \"A ministry, https://a\"\n", "");
        let e = run_with(&t, &bare, "lite", &mut ps).unwrap_err();
        assert!(e.contains("needs a credit"), "{e}");
    }
}
