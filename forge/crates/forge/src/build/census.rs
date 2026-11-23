//! Census population of admin units (D-386): a country's census table, joined to its units
//! through an official list's crosswalk (D-232), replaces the population the grid gave them.
//!
//! A table follows its list row by row (NSO's municipal table lists the 753 local levels in
//! the CBS list's order), so each row is checked by name: its name, unit-type words cut, must be
//! the list row's name or the unit's name since a rename, else the build stops. A unit's
//! census count replaces its population; the units below it keep their grid shares, scaled so
//! they add up to the count exactly (they stay modelled); the units above it are re-summed from
//! their children, so every level adds up. The table is read from the pinned xlsx itself
//! ([`sheet`]), so the build reads the published bytes, not a hand-made copy.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use teigeodata_core::PlaceKind;
use unicode_normalization::UnicodeNormalization;

use super::{Place, official};
use crate::lock::Lock;

const SCHEMA: &str = "teigeodata/census/v1";

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CensusFile {
    schema: String,
    tables: Vec<Table>,
}

/// One census table.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    /// The pinned source (an xlsx workbook).
    source: String,
    /// The census year, for the report.
    year: u16,
    /// The official list (its source id in `official-names.yaml`) the table follows row by row.
    list: String,
    /// The worksheet's part in the workbook (`xl/worksheets/sheet1.xml`).
    sheet: String,
    /// The first data row (1-based, as the spreadsheet numbers them).
    first_row: u32,
    /// The column of the unit's name, in the list's language.
    name: String,
    /// The column of the population.
    population: String,
    /// Rows (numbered from 1 in the table's data) whose name is far from their list row's,
    /// each checked by a person to be the same unit, with how.
    #[serde(default)]
    checked: BTreeMap<u32, String>,
}

/// What the census tier did.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    pub(super) report: String,
    /// Units whose count is the census's or a sum of census counts.
    pub(super) stated: BTreeSet<String>,
    /// Units below a census unit whose grid share was scaled to its count.
    pub(super) modelled: BTreeSet<String>,
}

impl Outcome {
    /// The modelled set the pack marks (D-351): the grid's, less the units the census counts or
    /// sums, plus the scaled shares below them.
    pub(super) fn mark(&self, grid: BTreeSet<String>) -> BTreeSet<String> {
        grid.into_iter()
            .filter(|k| !self.stated.contains(k))
            .chain(self.modelled.iter().cloned())
            .collect()
    }
}

fn read_file(root: &Path, cc: &str) -> Result<Option<CensusFile>, String> {
    let path = root.join("data/countries").join(cc).join("census.yaml");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let f: CensusFile =
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if f.schema != SCHEMA {
        return Err(format!("{}: schema is not {SCHEMA}", path.display()));
    }
    Ok(Some(f))
}

/// The sources the country's census tables read (for the licence gate and `make`'s fetch).
pub(crate) fn sources(root: &Path, cc: &str) -> Result<Vec<String>, String> {
    Ok(read_file(root, cc)?
        .map(|f| f.tables.into_iter().map(|t| t.source).collect())
        .unwrap_or_default())
}

/// Gives the units of each table the build may read (`allowed`) their census population.
pub(super) fn run(
    root: &Path,
    lock: &Lock,
    (cc, allowed): (&str, &[String]),
    places: &mut [Place],
) -> Result<Outcome, String> {
    let mut out = Outcome::default();
    let Some(f) = read_file(root, cc)? else {
        return Ok(out);
    };
    let index: HashMap<&str, usize> = places
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            std::iter::once(p.key.as_str())
                .chain(p.sources.iter().map(String::as_str))
                .map(move |k| (k, i))
        })
        .collect();
    let mut counts: Vec<(usize, i64)> = Vec::new();
    for t in &f.tables {
        if !allowed.contains(&t.source) {
            let _ = writeln!(
                out.report,
                "Census of {}: not read in this build.",
                t.source
            );
            continue;
        }
        let (list, strip) = official::rows(root, lock, cc, &t.list)?
            .ok_or_else(|| format!("{}: no official list {} to follow", t.source, t.list))?;
        let cells = sheet(&lock.path(root, &t.source)?, &t.sheet)?;
        let table = columns(&cells, t.first_row, &t.name, &t.population)?;
        let joined = join(&t.source, (&list, &strip), &table, &t.checked)?;
        let mut unjoined = 0;
        for (key, n) in joined.rows {
            match key.and_then(|k| index.get(k.as_str())) {
                Some(&i) => counts.push((i, n)),
                None => unjoined += 1,
            }
        }
        let _ = writeln!(
            out.report,
            "Census {} ({}): {} rows, every one checked by name against {} ({} near spellings, {} checked by hand); {} units took its count, {unjoined} rows have no unit in the build.",
            t.year,
            t.source,
            table.len(),
            t.list,
            joined.near,
            t.checked.len(),
            table.len() - unjoined,
        );
    }
    if !counts.is_empty() {
        apply(places, &counts, &mut out);
    }
    Ok(out)
}

/// A row's name is a spelling of its list row's when it differs in at most this share of the
/// longer name's characters (ङ for ङ्ग, ब for व, a dropped space: NSO's table against CBS's
/// list, 2026-11-23); a farther row must be checked by a person and listed (`checked`).
const NEAR: f64 = 0.34;

/// How the table's rows matched their list's.
#[derive(Debug, Default, PartialEq)]
struct Matched {
    /// The key of each row's unit and its count.
    rows: Vec<(Option<String>, i64)>,
    /// Rows whose name is a near spelling of the list's.
    near: usize,
}

/// The table's rows joined to the list's. Refuses a table whose row count differs from the
/// list's, and a row whose name is far from its list row's unless `checked` lists it (a row
/// checked and no longer far is refused too, so the list stays true).
fn join(
    source: &str,
    (list, strip): (&[official::Row], &[String]),
    table: &[(String, i64)],
    checked: &BTreeMap<u32, String>,
) -> Result<Matched, String> {
    if list.len() != table.len() {
        return Err(format!(
            "{source}: {} rows, its list {}; a table must follow its list row by row",
            table.len(),
            list.len()
        ));
    }
    let mut far = Vec::new();
    let mut stale = Vec::new();
    let mut out = Matched::default();
    for (i, (row, (name, n))) in list.iter().zip(table).enumerate() {
        let at = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let got: Vec<char> = plain(official::cut(name, strip)).chars().collect();
        let distance = row
            .names
            .iter()
            .map(|w| {
                let w: Vec<char> = plain(w).chars().collect();
                #[allow(clippy::cast_precision_loss, reason = "short names")]
                let d = crate::xlit::levenshtein(&got, &w) as f64
                    / got.len().max(w.len()).max(1) as f64;
                d
            })
            .fold(f64::INFINITY, f64::min);
        match (distance, checked.contains_key(&at)) {
            (d, false) if d > NEAR => far.push(format!(
                "row {at}: {name} is not {} ({:.0} % apart)",
                row.names.join(" or "),
                d * 100.0
            )),
            (d, true) if d <= NEAR => stale.push(at.to_string()),
            (_, true) => {}
            (d, _) if d > 0.0 => out.near += 1,
            _ => {}
        }
        out.rows.push((row.key.clone(), *n));
    }
    if !stale.is_empty() {
        return Err(format!(
            "{source}: rows {} are listed as checked but match their list rows; remove them",
            stale.join(", ")
        ));
    }
    if far.is_empty() {
        Ok(out)
    } else {
        Err(format!(
            "{source}: {} rows name another unit than their list row; check each and list it under `checked` if it is the same unit: {}",
            far.len(),
            far.join("; ")
        ))
    }
}

/// A name as the join compares it: NFC, without spaces, punctuation or the danda.
fn plain(s: &str) -> String {
    s.nfc()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation() && *c != '।')
        .collect()
}

/// Sets each counted unit's population, scales the units below it to the count and re-sums
/// the units above it.
fn apply(places: &mut [Place], counts: &[(usize, i64)], out: &mut Outcome) {
    let mut children: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, p) in places.iter().enumerate() {
        if p.kind == PlaceKind::Admin
            && let Some(parent) = &p.parent
        {
            children.entry(parent.as_str()).or_default().push(i);
        }
    }
    let children: HashMap<String, Vec<usize>> = children
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let key_at: HashMap<String, usize> = places
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.clone(), i))
        .collect();
    let mut grid_off = Vec::new();
    for &(i, n) in counts {
        let Some(p) = places.get_mut(i) else { continue };
        if let Some(b) = p.population.filter(|&b| b > 0) {
            #[allow(clippy::cast_precision_loss, reason = "a ratio of counts")]
            grid_off.push((n as f64 / b as f64 - 1.0).abs());
        }
        p.population = Some(n);
        out.stated.insert(p.key.clone());
        scale(places, &children, i, n, &mut out.modelled);
    }
    // Above: every ancestor of a counted unit, deepest first, is its children's sum.
    let mut above: BTreeSet<(std::cmp::Reverse<u8>, usize)> = BTreeSet::new();
    for &(i, _) in counts {
        let mut at = places.get(i).and_then(|p| p.parent.as_ref());
        while let Some(&j) = at.and_then(|k| key_at.get(k)) {
            let Some(p) = places.get(j) else { break };
            above.insert((std::cmp::Reverse(p.admin_level), j));
            at = p.parent.as_ref();
        }
    }
    let mut summed = 0;
    for &(_, j) in &above {
        let Some(key) = places.get(j).map(|p| p.key.clone()) else {
            continue;
        };
        let kids = children.get(&key).map_or(&[][..], Vec::as_slice);
        let kid = |c: usize| places.get(c);
        let sum: i64 = kids.iter().filter_map(|&c| kid(c)?.population).sum();
        let all_stated = kids
            .iter()
            .all(|&c| kid(c).is_some_and(|p| out.stated.contains(&p.key)));
        if sum > 0
            && let Some(p) = places.get_mut(j)
        {
            p.population = Some(sum);
            summed += 1;
            if all_stated {
                out.stated.insert(key);
            } else {
                out.modelled.insert(key);
            }
        }
    }
    grid_off.sort_by(f64::total_cmp);
    let pct = |q: f64| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "an index into a short list"
        )]
        let at = ((grid_off.len().saturating_sub(1)) as f64 * q).round() as usize;
        grid_off.get(at).map_or(0.0, |v| v * 100.0)
    };
    let _ = writeln!(
        out.report,
        "Census: {} units counted (their population before differed by a median {:.1} %, P90 {:.1} %), {} below them scaled to their counts, {summed} above them re-summed.",
        counts.len(),
        pct(0.5),
        pct(0.9),
        out.modelled.len(),
    );
}

/// Scales the populations of the units below `i` so they add up to `n`, keeping their shares
/// (largest remainder, so the sum is exact), down every level.
fn scale(
    places: &mut [Place],
    children: &HashMap<String, Vec<usize>>,
    i: usize,
    n: i64,
    modelled: &mut BTreeSet<String>,
) {
    let Some(kids) = places.get(i).and_then(|p| children.get(&p.key)) else {
        return;
    };
    let shares: Vec<(usize, i64)> = kids
        .iter()
        .filter_map(|&c| {
            let v = places.get(c)?.population.filter(|&v| v > 0)?;
            Some((c, v))
        })
        .collect();
    let total: i64 = shares.iter().map(|s| s.1).sum();
    if total == 0 {
        return;
    }
    let mut parts: Vec<(usize, i64, i128)> = shares
        .iter()
        .map(|&(c, v)| {
            let exact = i128::from(n) * i128::from(v);
            let floor = exact / i128::from(total);
            let rem = exact % i128::from(total);
            (c, i64::try_from(floor).unwrap_or(i64::MAX), rem)
        })
        .collect();
    let mut left = n - parts.iter().map(|p| p.1).sum::<i64>();
    parts.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    for p in &mut parts {
        if left <= 0 {
            break;
        }
        p.1 += 1;
        left -= 1;
    }
    for (c, v, _) in parts {
        if let Some(p) = places.get_mut(c) {
            p.population = Some(v);
            modelled.insert(p.key.clone());
        }
        scale(places, children, c, v, modelled);
    }
}

/// The cells of worksheet part `part` of the xlsx at `path`: (row, column letters) → text.
fn sheet(path: &Path, part: &str) -> Result<BTreeMap<(u32, String), String>, String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| at(&e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| at(&e))?;
    let mut text = |name: &str| -> Result<Option<String>, String> {
        let mut f = match zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(at(&e)),
        };
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(|e| at(&e))?;
        Ok(Some(s))
    };
    let strings = text("xl/sharedStrings.xml")?
        .map(|s| shared_strings(&s))
        .unwrap_or_default();
    let xml = text(part)?.ok_or_else(|| at(&format!("no {part}")))?;
    cells(&xml, &strings).map_err(|e| at(&e))
}

/// The shared strings of `sharedStrings.xml`: each `<si>`'s `<t>` texts, joined (a rich text's
/// runs), NFC.
fn shared_strings(xml: &str) -> Vec<String> {
    xml.split("<si>")
        .skip(1)
        .map(|si| {
            let si = si.split("</si>").next().unwrap_or_default();
            // A phonetic run (`<rPh>`) is a reading aid, not the text.
            let mut base = String::new();
            let mut rest = si;
            while let Some(at) = rest.find("<rPh") {
                base.push_str(&rest[..at]);
                rest = rest[at..]
                    .find("</rPh>")
                    .map_or("", |e| &rest[at + e + 6..]);
            }
            base.push_str(rest);
            let mut s = String::new();
            let mut rest = base.as_str();
            while let Some(at) = rest.find("<t") {
                rest = &rest[at + 2..];
                if !(rest.starts_with('>') || rest.starts_with(' ')) {
                    continue;
                }
                let Some(open) = rest.find('>') else { break };
                let body = &rest[open + 1..];
                let end = body.find("</t>").unwrap_or(body.len());
                s.push_str(&unescape(&body[..end]));
                rest = &body[end..];
            }
            s.nfc().collect()
        })
        .collect()
}

/// The cells of a worksheet's XML: shared strings resolved, inline strings and numbers as
/// written.
fn cells(xml: &str, strings: &[String]) -> Result<BTreeMap<(u32, String), String>, String> {
    let mut out = BTreeMap::new();
    for c in xml.split("<c ").skip(1) {
        let head = &c[..c.find('>').unwrap_or(c.len())];
        let attr = |name: &str| {
            let k = format!(" {name}=\"");
            let h = format!(" {head}");
            h.find(&k).map(|i| {
                let v = &h[i + k.len()..];
                v[..v.find('"').unwrap_or(v.len())].to_string()
            })
        };
        let Some(r) = attr("r") else { continue };
        if head.ends_with('/') {
            continue;
        }
        let split = r.find(|c: char| c.is_ascii_digit()).unwrap_or(r.len());
        let (col, row) = r.split_at(split);
        let row: u32 = row.parse().map_err(|_| format!("cell {r}"))?;
        let body = c.split("</c>").next().unwrap_or_default();
        let inner = |tag: &str| {
            let open = format!("<{tag}");
            body.find(&open).and_then(|i| {
                let b = &body[i..];
                let s = b.find('>')? + 1;
                let e = b.find(&format!("</{tag}>"))?;
                Some(unescape(&b[s..e]))
            })
        };
        let value = match attr("t").as_deref() {
            Some("s") => {
                let i: usize = inner("v")
                    .and_then(|v| v.trim().parse().ok())
                    .ok_or_else(|| format!("cell {r}: no shared string index"))?;
                strings
                    .get(i)
                    .cloned()
                    .ok_or_else(|| format!("cell {r}: shared string {i} is missing"))?
            }
            Some("inlineStr") => inner("t").unwrap_or_default(),
            _ => inner("v").unwrap_or_default(),
        };
        out.insert((row, col.to_string()), value.trim().to_string());
    }
    Ok(out)
}

/// XML's five entities and numeric references.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find(';').unwrap_or(0);
        let ent = &rest[1..end.max(1)];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) if end > 0 => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The table's (name, population) rows from `first` down to the last row with a name; a row
/// with a name and no whole positive population is refused.
fn columns(
    cells: &BTreeMap<(u32, String), String>,
    first: u32,
    name: &str,
    population: &str,
) -> Result<Vec<(String, i64)>, String> {
    let last = cells
        .keys()
        .filter(|(r, c)| *r >= first && c == name)
        .map(|(r, _)| *r)
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    for r in first..=last {
        let get = |c: &str| cells.get(&(r, c.to_string())).map_or("", String::as_str);
        let n = get(name);
        if n.is_empty() {
            return Err(format!("row {r}: no name in column {name}"));
        }
        let v = get(population);
        let p: f64 = v
            .parse()
            .map_err(|_| format!("row {r}: population `{v}` in column {population}"))?;
        if p.fract() != 0.0 || p <= 0.0 || p > 1e10 {
            return Err(format!("row {r}: population {v} is not a whole count"));
        }
        #[allow(clippy::cast_possible_truncation, reason = "checked whole and bounded")]
        out.push((n.to_string(), p as i64));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
