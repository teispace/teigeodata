//! The postal codes of a build (phase 3 step 3.5, D-090, D-105), from
//! `data/countries/<cc>/postal.yaml` and the codes files it names.
//!
//! A codes file gives one row per admin unit: its code and its number of parts (Nepal: a local
//! level's 5-digit code and its wards, each the code and a 2-digit number). Each row becomes an
//! area record of the unit's place (`admin:<pcode>`) and one part record per part. Before any
//! of it is used, the file is checked against its pinned source table: the same codes, each
//! with the same number of parts. A build that may not read the source (the lite build, for a
//! source with no stated licence) leaves the codes out and says so.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use teigeodata_format::postal::{CodeKind, Rule, ascii};

use crate::fetch::store_path;
use crate::lock::Lock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    schemes: Vec<Scheme>,
    #[serde(default)]
    codes: Vec<Codes>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scheme {
    id: String,
    #[allow(
        dead_code,
        reason = "documentation in the data file; checked by check-data"
    )]
    authority: String,
    #[allow(dead_code, reason = "the scheme's evidence; checked by check-data")]
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_to: Option<String>,
    #[serde(default)]
    admin_aligned: bool,
    #[serde(default)]
    upper: bool,
    #[serde(default)]
    ignore: String,
    #[serde(default)]
    prefixes: Vec<String>,
    #[serde(default)]
    display: String,
    templates: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Codes {
    scheme: String,
    file: String,
    source: String,
    admin_source: String,
    #[serde(default)]
    part_digits: u32,
    #[allow(dead_code, reason = "the totals check-data holds the file to")]
    expect: serde::de::IgnoredAny,
    verify: Verify,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Verify {
    format: String,
    code_column: usize,
    parts_column: usize,
}

/// One code of the build: the place key it links to, its scheme, code and kind.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) unit: String,
    pub(crate) scheme: String,
    pub(crate) code: String,
    pub(crate) kind: CodeKind,
}

/// The postal codes a build carries: the rules of the schemes that have codes, the codes, and
/// notes for the summary.
#[derive(Debug, Default)]
pub(crate) struct Postal {
    pub(crate) rules: Vec<Rule>,
    pub(crate) entries: Vec<Entry>,
    pub(crate) notes: Vec<String>,
}

/// Days since 1970-01-01 of `YYYY-MM-DD`.
fn days(date: &str) -> Option<u32> {
    let mut it = date.split('-').map(str::parse::<i64>);
    let (y, m, d) = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    u32::try_from(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468).ok()
}

/// The postal codes of country `cc` for a build on boundary source `admin`; `allowed` is the
/// build's licence gate. `None` when the country has no postal file.
pub(crate) fn load(
    root: &Path,
    cc: &str,
    lock: &Lock,
    admin: &str,
    allowed: &mut dyn FnMut(&str) -> Result<bool, String>,
) -> Result<Option<Postal>, String> {
    let dir = root.join("data/countries").join(cc);
    let path = dir.join("postal.yaml");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    let e = |m: String| format!("{}: {m}", path.display());
    let file: File = serde_saphyr::from_str(&text).map_err(|x| e(x.to_string()))?;
    if file.schema != "teigeodata/postal/v1" {
        return Err(e("not schema teigeodata/postal/v1".into()));
    }
    let mut out = Postal::default();
    for c in &file.codes {
        if !allowed(&c.source)? {
            out.notes.push(format!(
                "postal codes `{}` left out: this build may not read {}",
                c.file, c.source
            ));
            continue;
        }
        if c.admin_source != admin {
            out.notes.push(format!(
                "postal codes `{}` left out: they name units of {}, not {admin}",
                c.file, c.admin_source
            ));
            continue;
        }
        let rows = read_rows(&dir.join(&c.file))?;
        let source = lock.get(&c.source)?;
        let table = fs::read(store_path(root, source))
            .map_err(|x| format!("{}: {x}; run forge fetch {}", c.source, c.source))?;
        verify(&rows, &table, &c.verify).map_err(|m| format!("{}: {m}", c.file))?;
        let width = c.part_digits as usize;
        for (code, pcode, parts) in &rows {
            let unit = format!("admin:{pcode}");
            let mut push = |code: String, kind| {
                out.entries.push(Entry {
                    unit: unit.clone(),
                    scheme: c.scheme.clone(),
                    code,
                    kind,
                });
            };
            push(code.clone(), CodeKind::Area);
            for w in 1..=*parts {
                push(format!("{code}{w:0width$}"), CodeKind::Part);
            }
        }
    }
    for s in &file.schemes {
        if !out.entries.iter().any(|x| x.scheme == s.id) {
            continue;
        }
        let date = |d: &Option<String>| match d {
            Some(d) => days(d)
                .map(Some)
                .ok_or_else(|| e(format!("scheme {}: bad date {d}", s.id))),
            None => Ok(None),
        };
        out.rules.push(Rule {
            name: s.id.clone(),
            valid_from: date(&s.valid_from)?,
            valid_to: date(&s.valid_to)?,
            upper: s.upper,
            admin_aligned: s.admin_aligned,
            ignore: s.ignore.clone(),
            prefixes: s.prefixes.clone(),
            display: s.display.clone(),
            templates: s.templates.clone(),
        });
    }
    Ok(Some(out))
}

/// The rows of a codes file: (code, pcode, parts).
fn read_rows(path: &Path) -> Result<Vec<(String, String, u32)>, String> {
    let text = fs::read_to_string(path).map_err(|x| format!("{}: {x}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    if lines.next() != Some("code\tpcode\tparts\tname") {
        return Err(format!("{}: not a postal codes file", path.display()));
    }
    lines
        .map(|l| {
            let mut f = l.split('\t');
            match (f.next(), f.next(), f.next().map(str::parse::<u32>)) {
                (Some(c), Some(p), Some(Ok(n))) => Ok((c.to_string(), p.to_string(), n)),
                _ => Err(format!("{}: bad row `{l}`", path.display())),
            }
        })
        .collect()
}

/// The cells of every row of every HTML table in `html`, as text: tags dropped, the common
/// entities decoded, spaces collapsed.
fn html_rows(html: &str) -> Vec<Vec<String>> {
    let lower = html.to_ascii_lowercase();
    let mut rows = Vec::new();
    let mut at = 0;
    while let Some(tr) = lower.get(at..).and_then(|s| s.find("<tr")) {
        let start = at + tr;
        let end = lower
            .get(start..)
            .and_then(|s| s.find("</tr"))
            .map_or(lower.len(), |e| start + e);
        let mut cells = Vec::new();
        let mut c = start;
        loop {
            let next = ["<td", "<th"]
                .iter()
                .filter_map(|t| lower.get(c..end).and_then(|s| s.find(t)))
                .min();
            let Some(open) = next else { break };
            let open = c + open;
            let body = lower
                .get(open..end)
                .and_then(|s| s.find('>'))
                .map_or(end, |g| open + g + 1);
            let close = ["</td", "</th", "<td", "<th"]
                .iter()
                .filter_map(|t| lower.get(body..end).and_then(|s| s.find(t)))
                .min()
                .map_or(end, |x| body + x);
            cells.push(text_of(html.get(body..close).unwrap_or("")));
            c = close.max(open + 1);
        }
        rows.push(cells);
        at = end.max(start + 1);
    }
    rows
}

/// The text of an HTML fragment.
fn text_of(fragment: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in fragment.chars() {
        match ch {
            '<' => tag = true,
            '>' => {
                tag = false;
                out.push(' ');
            }
            _ if !tag => out.push(ch),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A cell's number, in any script's digits.
fn number(cell: &str) -> Option<u32> {
    let digits: String = cell
        .chars()
        .map(|c| ascii(c).map(char::from))
        .collect::<Option<String>>()?;
    digits.trim().parse().ok()
}

/// The codes file against the source table: every code of the table is in the file with the
/// same number of parts, and the file has no other code.
fn verify(rows: &[(String, String, u32)], table: &[u8], v: &Verify) -> Result<(), String> {
    if v.format != "html-table" {
        return Err(format!("unknown verify format {}", v.format));
    }
    let html = String::from_utf8_lossy(table);
    let mut source: BTreeMap<String, u32> = BTreeMap::new();
    for cells in html_rows(&html) {
        let (Some(code), Some(parts)) = (
            cells.get(v.code_column.wrapping_sub(1)),
            cells.get(v.parts_column.wrapping_sub(1)),
        ) else {
            continue;
        };
        let (Some(code), Some(parts)) =
            (number(code).map(|_| code.trim().to_string()), number(parts))
        else {
            continue;
        };
        if source.insert(code.clone(), parts).is_some() {
            return Err(format!("the source table has code {code} twice"));
        }
    }
    let file: BTreeMap<&str, u32> = rows.iter().map(|r| (r.0.as_str(), r.2)).collect();
    let mut problems = Vec::new();
    for (code, parts) in &source {
        match file.get(code.as_str()) {
            Some(p) if p == parts => {}
            Some(p) => problems.push(format!("{code}: {p} parts here, {parts} in the source")),
            None => problems.push(format!("{code}: in the source, not here")),
        }
    }
    for code in file.keys() {
        if !source.contains_key(*code) {
            problems.push(format!("{code}: here, not in the source"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        let n = problems.len();
        problems.truncate(5);
        Err(format!(
            "{n} differences from the source table: {}",
            problems.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Verify, days, html_rows, verify};

    const HTML: &str = "<table><tr><th>S.N.</th><th>Code</th><th>Wards</th></tr>\
        <tr><td>1</td><td> 30608 </td><td>३२</td></tr>\
        <TR><TD>2</TD><TD><b>30609</b></TD><TD>10&nbsp;</TD></TR>\
        <tr><td></td><td>office only</td><td></td></tr></table>";

    fn v() -> Verify {
        Verify {
            format: "html-table".into(),
            code_column: 2,
            parts_column: 3,
        }
    }

    #[test]
    fn reads_tables_and_checks_codes() {
        let rows = html_rows(HTML);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2], ["2", "30609", "10"]);
        let good = vec![
            ("30608".to_string(), "U1".to_string(), 32),
            ("30609".to_string(), "U2".to_string(), 10),
        ];
        assert_eq!(verify(&good, HTML.as_bytes(), &v()), Ok(()));
        let mut wrong = good.clone();
        wrong[0].2 = 33;
        assert!(verify(&wrong, HTML.as_bytes(), &v()).is_err());
        assert!(verify(&good[..1], HTML.as_bytes(), &v()).is_err());
        let mut extra = good;
        extra.push(("30610".into(), "U3".into(), 1));
        assert!(verify(&extra, HTML.as_bytes(), &v()).is_err());
        assert_eq!(days("2025-09-08"), Some(20_339));
        assert_eq!(days("1970-01-01"), Some(0));
    }
}
