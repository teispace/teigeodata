//! Postal rules for `data/countries/<cc>/postal.yaml`, schema `teigeodata/postal/v1` (D-105),
//! and the codes files it names (`teigeodata/postal-codes/v1`).
//!
//! The rules and every code are checked by the pack writer itself (`postal::encode`), so a
//! rule the reader would refuse, or a code no template accepts, fails here first.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use teigeodata_format::postal::{self, Code, CodeKind, Rule};

const SCHEMA: &str = "teigeodata/postal/v1";
const HEADER: &str = "code\tpcode\tparts\tname";

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
    authority: String,
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
    expect: Expect,
    verify: Verify,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    units: usize,
    parts: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Verify {
    format: String,
    code_column: usize,
    parts_column: usize,
}

/// Days since 1970-01-01 of an ISO date `YYYY-MM-DD` (proleptic Gregorian).
pub(crate) fn days(date: &str) -> Option<u32> {
    let mut it = date.split('-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > *month_days.get(usize::try_from(m - 1).ok()?)? {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u32::try_from(era * 146_097 + doe - 719_468).ok()
}

/// Checks one country's postal file; `ids` collects scheme IDs across countries (they are
/// unique worldwide). Returns the number of codes checked and the problems.
pub(crate) fn check(
    dir: &Path,
    text: &str,
    pinned: &BTreeSet<String>,
    ids: &mut BTreeSet<String>,
) -> (usize, Vec<String>) {
    let mut p = Vec::new();
    let file: File = match serde_saphyr::from_str(text) {
        Ok(f) => f,
        Err(e) => return (0, vec![format!("not a postal file: {e}")]),
    };
    if file.schema != SCHEMA {
        p.push(format!("schema must be `{SCHEMA}`"));
    }
    let mut rules = Vec::new();
    for s in &file.schemes {
        let at = format!("scheme `{}`", s.id);
        if !ids.insert(s.id.clone()) {
            p.push(format!(
                "{at}: the ID is used twice (scheme IDs are unique worldwide)"
            ));
        }
        if s.authority.trim().is_empty() {
            p.push(format!("{at}: authority is empty"));
        }
        if let Some(src) = &s.source
            && !pinned.contains(src)
        {
            p.push(format!("{at}: source `{src}` is not in data/sources.lock"));
        }
        let mut date = |d: &Option<String>, field: &str| match d {
            Some(d) => days(d).or_else(|| {
                p.push(format!("{at}: {field} `{d}` is not a date YYYY-MM-DD"));
                None
            }),
            None => None,
        };
        let rule = Rule {
            name: s.id.clone(),
            valid_from: date(&s.valid_from, "valid_from"),
            valid_to: date(&s.valid_to, "valid_to"),
            upper: s.upper,
            admin_aligned: s.admin_aligned,
            ignore: s.ignore.clone(),
            prefixes: s.prefixes.clone(),
            display: s.display.clone(),
            templates: s.templates.clone(),
        };
        if let Err(e) = postal::encode(std::slice::from_ref(&rule), &[], 1, false) {
            p.push(format!("{at}: the pack writer refuses the rule: {e}"));
        }
        rules.push(rule);
    }
    let mut checked = 0;
    for c in &file.codes {
        let at = format!("codes `{}`", c.file);
        if !rules.iter().any(|r| r.name == c.scheme) {
            p.push(format!("{at}: scheme `{}` is not defined here", c.scheme));
            continue;
        }
        for src in [&c.source, &c.admin_source] {
            if !pinned.contains(src) {
                p.push(format!("{at}: source `{src}` is not in data/sources.lock"));
            }
        }
        if c.part_digits > 4 {
            p.push(format!("{at}: part_digits is above 4"));
        }
        if c.verify.format != "html-table"
            || c.verify.code_column == 0
            || c.verify.parts_column == 0
        {
            p.push(format!(
                "{at}: verify needs format `html-table` and columns counted from 1"
            ));
        }
        match fs::read_to_string(dir.join(&c.file)) {
            Ok(rows) => {
                let (n, found) = check_codes(&rows, c, &rules);
                checked += n;
                p.extend(found.into_iter().map(|e| format!("{at}: {e}")));
            }
            Err(e) => p.push(format!("{at}: {e}")),
        }
    }
    (checked, p)
}

/// The rows of a codes file: header, unique codes and units, part counts that fit, the
/// expected totals, and every code and part code accepted by the scheme's rule.
fn check_codes(text: &str, c: &Codes, rules: &[Rule]) -> (usize, Vec<String>) {
    let mut p = Vec::new();
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    if lines.next() != Some(HEADER) {
        return (0, vec![format!("the header must be `{HEADER}`")]);
    }
    let (mut seen_codes, mut seen_units) = (BTreeSet::new(), BTreeSet::new());
    let mut codes = Vec::new();
    let mut parts_total = 0;
    let limit = 10_u32.pow(c.part_digits);
    for (i, line) in lines.enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        let [code, pcode, parts, name] = f.as_slice() else {
            p.push(format!("row {}: not 4 fields", i + 1));
            continue;
        };
        if !seen_codes.insert(*code) {
            p.push(format!("row {}: code {code} twice", i + 1));
        }
        if pcode.is_empty() || !seen_units.insert(*pcode) {
            p.push(format!("row {}: unit `{pcode}` empty or twice", i + 1));
        }
        if name.trim().is_empty() {
            p.push(format!("row {}: the name is empty", i + 1));
        }
        let Ok(parts) = parts.parse::<u32>() else {
            p.push(format!("row {}: parts `{parts}` is not a number", i + 1));
            continue;
        };
        if parts >= limit {
            p.push(format!(
                "row {}: {parts} parts do not fit {} digits",
                i + 1,
                c.part_digits
            ));
            continue;
        }
        let place = u32::try_from(codes.len()).unwrap_or(u32::MAX);
        let mut push = |code: String, kind| {
            codes.push(Code {
                scheme: c.scheme.clone(),
                code,
                place,
                kind,
                valid_from: None,
                valid_to: None,
                redirect: None,
            });
        };
        push((*code).to_string(), CodeKind::Area);
        for w in 1..=parts {
            push(
                format!("{code}{w:0width$}", width = c.part_digits as usize),
                CodeKind::Part,
            );
        }
        parts_total += parts as usize;
    }
    if seen_units.len() != c.expect.units || parts_total != c.expect.parts {
        p.push(format!(
            "{} units and {parts_total} parts; expected {} and {}",
            seen_units.len(),
            c.expect.units,
            c.expect.parts
        ));
    }
    let places = u32::try_from(codes.len()).unwrap_or(u32::MAX);
    if let Err(e) = postal::encode(rules, &codes, places, false) {
        p.push(format!("the pack writer refuses the codes: {e}"));
    }
    (codes.len(), p)
}

#[cfg(test)]
#[allow(
    clippy::assert_is_empty,
    reason = "tests: the problems list is printed on failure"
)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::{check, days};

    const GOOD: &str = r#"schema: teigeodata/postal/v1
schemes:
  - id: zz
    authority: Test Post
    valid_from: 2025-09-08
    admin_aligned: true
    ignore: " -"
    templates: ["999", "99999"]
"#;

    #[test]
    fn days_are_civil() {
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(days("2025-09-08"), Some(20_339));
        assert_eq!(days("2000-02-29"), Some(11_016));
        for bad in [
            "2025-02-29",
            "2025-13-01",
            "2025-9-8",
            "25-09-08",
            "2025-09-08-1",
        ] {
            assert_eq!(days(bad), None, "{bad}");
        }
    }

    #[test]
    fn accepts_good_and_refuses_each_violation() {
        let pinned = BTreeSet::new();
        let run = |t: &str| check(Path::new("."), t, &pinned, &mut BTreeSet::new()).1;
        assert!(run(GOOD).is_empty(), "{:?}", run(GOOD));
        for (from, to) in [
            ("teigeodata/postal/v1", "teigeodata/postal/v2"),
            ("id: zz", "id: ZZ"),
            ("[\"999\", \"99999\"]", "[]"),
            ("[\"999\", \"99999\"]", "[\"9\\\\\"]"),
            ("2025-09-08", "2025-02-30"),
            ("ignore: \" -\"", "ignore: \"\u{e9}\""),
            ("    templates", "    source: nowhere\n    templates"),
        ] {
            let bad = GOOD.replace(from, to);
            assert_ne!(bad, GOOD, "{to}");
            assert!(!run(&bad).is_empty(), "accepted: {to}");
        }
        // Scheme IDs are unique across countries.
        let mut ids = BTreeSet::new();
        assert!(check(Path::new("."), GOOD, &pinned, &mut ids).1.is_empty());
        assert!(!check(Path::new("."), GOOD, &pinned, &mut ids).1.is_empty());
    }

    #[test]
    fn checks_the_codes_file() {
        let dir = std::env::temp_dir().join(format!("tgd-postal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let pinned: BTreeSet<String> = ["s".to_string(), "a".to_string()].into();
        let file = format!(
            "{GOOD}codes:\n  - scheme: zz\n    file: c.tsv\n    source: s\n    admin_source: a\n    part_digits: 2\n    expect: {{ units: 2, parts: 3 }}\n    verify: {{ format: html-table, code_column: 1, parts_column: 2 }}\n"
        );
        let run = |rows: &str| {
            std::fs::write(dir.join("c.tsv"), rows).unwrap_or_default();
            check(&dir, &file, &pinned, &mut BTreeSet::new())
        };
        let good = "# a comment\ncode\tpcode\tparts\tname\n101\tU1\t1\tOne\n102\tU2\t2\tTwo\n";
        let (n, p) = run(good);
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(n, 5);
        for bad in [
            good.replace("101\tU1", "101\tU2"),
            good.replace("102\tU2", "101\tU2"),
            good.replace("\t2\tTwo", "\t3\tTwo"),
            good.replace("\t2\tTwo", "\t100\tTwo"),
            good.replace("101\t", "1x1\t"),
            good.replace("code\tpcode", "pcode\tcode"),
            good.replace("\tOne", ""),
        ] {
            assert!(!run(&bad).1.is_empty(), "accepted:\n{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap_or_default();
    }
}
