//! `cargo xtask check-pack <pack.tgd> [--lite]`: the licence gate on a built pack (G-lic,
//! D-126). The pack opens with every section's hash verified; its manifest states the build's
//! licence (`odbl` main, `cc-by-4.0` lite, ADR-0002) and carries the sources' notices; in a
//! lite pack every notice belongs only to sources whose licence class the lite build may read
//! (`CLEAN_ROOM.md` rule 2). A notice no pinned source states is refused: the pack cannot say
//! where it came from.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use teigeodata_format::{Pack, Tag};

use crate::sources::LITE;

#[derive(Deserialize)]
struct Lock {
    #[serde(default, rename = "source")]
    sources: Vec<Source>,
}

#[derive(Deserialize)]
struct Source {
    id: String,
    licence_class: String,
    attribution: String,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let lite = args.iter().any(|a| a == "--lite");
    let path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or_else(|| one("usage: cargo xtask check-pack <pack.tgd> [--lite]".into()))?;
    let bytes = fs::read(path).map_err(|e| one(format!("{path}: {e}")))?;
    let pack = Pack::open_verified(&bytes).map_err(|e| one(format!("{path}: {e}")))?;
    let lock_text =
        fs::read_to_string(root.join("data/sources.lock")).map_err(|e| one(e.to_string()))?;
    let lock: Lock = toml::from_str(&lock_text).map_err(|e| one(format!("sources.lock: {e}")))?;
    let m = pack.manifest();
    let problems = check(m.text(Tag::LICENCE), m.attribution(), lite, &lock);
    if !problems.is_empty() {
        return Err(problems
            .into_iter()
            .map(|p| format!("{path}: {p}"))
            .collect());
    }
    let notices = m.attribution().map_or(0, |a| a.lines().count());
    Ok(format!(
        "{}pack ok: {path} ({} build), {notices} notices, sections verified",
        sizes(&pack, bytes.len()),
        if lite { "lite" } else { "main" }
    ))
}

/// Each section's stored bytes, items and share of the file, largest first: what a component
/// split would download.
fn sizes(pack: &Pack<'_>, total: usize) -> String {
    use std::fmt::Write as _;
    let mut rows: Vec<(String, usize, u64)> = pack
        .sections()
        .map(|s| (s.kind().to_string(), s.bytes().len(), s.item_count()))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut out = String::from("| Section | Bytes | Items | Share |\n|---|--:|--:|--:|\n");
    let mut sum = 0;
    for (kind, n, items) in rows {
        sum += n;
        #[allow(clippy::cast_precision_loss, reason = "byte counts far below 2^52")]
        let share = n as f64 * 100.0 / total.max(1) as f64;
        let _ = writeln!(out, "| {kind} | {n} | {items} | {share:.1}% |");
    }
    let _ = writeln!(
        out,
        "| head and padding | {} | | |\n",
        total.saturating_sub(sum)
    );
    out
}

/// The licence and notices of a pack against its build: the problems found.
fn check(licence: Option<&str>, attribution: Option<&str>, lite: bool, lock: &Lock) -> Vec<String> {
    let mut problems = Vec::new();
    let want = if lite { "cc-by-4.0" } else { "odbl" };
    if licence != Some(want) {
        problems.push(format!(
            "licence {licence:?}, a {} build states {want}",
            if lite { "lite" } else { "main" }
        ));
    }
    let Some(text) = attribution else {
        problems.push("no attribution: every pack carries its sources' notices (D-126)".into());
        return problems;
    };
    for line in text.lines() {
        let owners: Vec<&Source> = lock
            .sources
            .iter()
            .filter(|s| s.attribution == line)
            .collect();
        if owners.is_empty() {
            problems.push(format!("notice {line:?} is stated by no pinned source"));
            continue;
        }
        if lite {
            for s in owners
                .iter()
                .filter(|s| !LITE.contains(&s.licence_class.as_str()))
            {
                problems.push(format!(
                    "a lite pack carries the notice of {} ({}), which the lite build may not read",
                    s.id, s.licence_class
                ));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::{Lock, check};

    const LOCK: &str = r#"
[[source]]
id = "osm"
licence_class = "odbl"
attribution = "© OpenStreetMap contributors"

[[source]]
id = "gn"
licence_class = "cc-by"
attribution = "GeoNames, CC BY 4.0"
"#;

    #[test]
    fn a_lite_pack_carries_only_lite_notices() {
        let lock: Lock = toml::from_str(LOCK).unwrap();
        let both = Some("© OpenStreetMap contributors\nGeoNames, CC BY 4.0");
        let none: [String; 0] = [];
        assert_eq!(check(Some("odbl"), both, false, &lock), none);
        assert_eq!(
            check(Some("cc-by-4.0"), Some("GeoNames, CC BY 4.0"), true, &lock),
            none
        );
        let p = check(Some("cc-by-4.0"), both, true, &lock);
        assert_eq!(p.len(), 1);
        assert!(p[0].contains("osm (odbl)"), "{p:?}");
        // The licence must match the build, every notice must be a pinned source's, and a
        // pack without notices fails.
        assert!(
            check(Some("odbl"), Some("GeoNames, CC BY 4.0"), true, &lock)[0].contains("licence")
        );
        assert!(
            check(Some("odbl"), Some("Someone else"), false, &lock)[0].contains("no pinned source")
        );
        assert!(check(Some("odbl"), None, false, &lock)[0].contains("no attribution"));
    }

    /// The lite classes here and in the forge's licence gate are one list.
    #[test]
    fn the_forge_gates_lite_with_the_same_classes() {
        let root = super::super::workspace_root();
        let src =
            std::fs::read_to_string(root.join("forge/crates/forge/src/build/mod.rs")).unwrap();
        let start = src.find("pub(crate) const LITE").unwrap();
        let list = &src[start..start + src[start..].find("];").unwrap()];
        let forge: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(forge, super::LITE);
    }
}
