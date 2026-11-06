//! `cargo xtask check-docs`: the checkable rules of `docs/CONVENTIONS.md` §8.
//!
//! - every Markdown file under `docs/` has frontmatter with a title, a known status and a valid
//!   `updated` date (templates under `docs/11-agents/templates/` are exempt from the values);
//! - every relative link in `docs/`, `.github/` and the root Markdown files resolves to a file,
//!   and a `#anchor` into a Markdown file names one of its headings;
//! - research notes carry `owner` and `revisit` and the template's nine sections;
//! - ADR file names follow `ADR-NNNN-slug.md` and each ADR is linked from the decision log.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const STATUSES: [&str; 6] = [
    "research",
    "draft",
    "proposed",
    "accepted",
    "living",
    "superseded",
];
const TEMPLATES: &str = "docs/11-agents/templates";
const NOTES: &str = "docs/02-research/notes";
const ADRS: &str = "docs/03-decisions/adr";
const LOG: &str = "docs/03-decisions/log.md";

/// Runs every docs check under `root`.
pub(crate) fn check(root: &Path) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    let files = markdown_files(root);
    for file in &files {
        let rel = relative(root, file);
        let Ok(text) = fs::read_to_string(file) else {
            problems.push(format!("{rel}: cannot read"));
            continue;
        };
        if rel.starts_with("docs/") {
            check_frontmatter(&rel, &text, &mut problems);
        }
        if rel.starts_with(NOTES) {
            check_research_note(&rel, &text, &mut problems);
        }
        // Templates hold placeholder links (`URL`) by design.
        if !rel.starts_with(TEMPLATES) {
            check_links(root, file, &rel, &text, &mut problems);
        }
    }
    check_adrs(root, &mut problems);
    if problems.is_empty() {
        Ok(format!("docs ok: {} Markdown files checked", files.len()))
    } else {
        Err(problems)
    }
}

/// Markdown files under `docs/` and `.github/`, and at the root (not in build output).
fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in ["docs", ".github"] {
        walk(&root.join(dir), &mut out);
    }
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && is_markdown(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if is_markdown(&path) {
            out.push(path);
        }
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md")
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The `key: value` lines between the opening and closing `---`, or `None` without frontmatter.
fn frontmatter(text: &str) -> Option<Vec<(String, String)>> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut fields = Vec::new();
    for line in lines {
        if line.trim_end() == "---" {
            return Some(fields);
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"').to_string();
            fields.push((key.trim().to_string(), value));
        }
    }
    None
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn check_frontmatter(rel: &str, text: &str, problems: &mut Vec<String>) {
    let Some(fields) = frontmatter(text) else {
        problems.push(format!(
            "{rel}: missing frontmatter (docs/CONVENTIONS.md §3)"
        ));
        return;
    };
    if rel.starts_with(TEMPLATES) {
        return;
    }
    if field(&fields, "title").is_none_or(str::is_empty) {
        problems.push(format!("{rel}: frontmatter has no title"));
    }
    match field(&fields, "status") {
        Some(s) if STATUSES.contains(&s) => {}
        Some(s) => problems.push(format!("{rel}: unknown status `{s}`")),
        None => problems.push(format!("{rel}: frontmatter has no status")),
    }
    match field(&fields, "updated") {
        Some(d) if is_date(d) => {}
        Some(d) => problems.push(format!("{rel}: `updated: {d}` is not a YYYY-MM-DD date")),
        None => problems.push(format!("{rel}: frontmatter has no updated date")),
    }
}

/// `YYYY-MM-DD` with a plausible month and day.
fn is_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return false;
    };
    let digits = |p: &str, n: usize| p.len() == n && p.bytes().all(|b| b.is_ascii_digit());
    if !(digits(y, 4) && digits(m, 2) && digits(d, 2)) {
        return false;
    }
    let (Ok(m), Ok(d)) = (m.parse::<u8>(), d.parse::<u8>()) else {
        return false;
    };
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

fn check_research_note(rel: &str, text: &str, problems: &mut Vec<String>) {
    if let Some(fields) = frontmatter(text) {
        for key in ["owner", "revisit"] {
            if field(&fields, key).is_none() {
                problems.push(format!("{rel}: research note has no `{key}`"));
            }
        }
        if let Some(d) = field(&fields, "revisit").filter(|d| !is_date(d)) {
            problems.push(format!("{rel}: `revisit: {d}` is not a YYYY-MM-DD date"));
        }
    }
    for n in 1..=9 {
        let prefix = format!("## {n}.");
        if !text.lines().any(|l| l.starts_with(&prefix)) {
            problems.push(format!(
                "{rel}: research note lacks section `{prefix}` (template)"
            ));
        }
    }
}

/// Relative link targets in `text`, outside fenced code blocks and inline code.
fn link_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        for marker in ["```", "~~~"] {
            if trimmed.starts_with(marker) {
                fence = match fence {
                    Some(open) if open == marker => None,
                    Some(open) => Some(open),
                    None => Some(marker),
                };
            }
        }
        if fence.is_some() || trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            continue;
        }
        let visible = strip_inline_code(line);
        let mut rest = visible.as_str();
        while let Some(start) = rest.find("](") {
            let after = rest.get(start + 2..).unwrap_or_default();
            let Some(end) = after.find(')') else { break };
            let raw = after.get(..end).unwrap_or_default();
            let target = raw.split_whitespace().next().unwrap_or_default();
            let target = target.trim_start_matches('<').trim_end_matches('>');
            if !target.is_empty() {
                targets.push(target.to_string());
            }
            rest = after.get(end..).unwrap_or_default();
        }
    }
    targets
}

fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for c in line.chars() {
        if c == '`' {
            in_code = !in_code;
        } else if !in_code {
            out.push(c);
        }
    }
    out
}

fn is_external(target: &str) -> bool {
    ["http://", "https://", "mailto:", "tel:"]
        .iter()
        .any(|p| target.starts_with(p))
}

fn check_links(root: &Path, file: &Path, rel: &str, text: &str, problems: &mut Vec<String>) {
    let dir = file.parent().unwrap_or(root);
    for target in link_targets(text) {
        if is_external(&target) {
            continue;
        }
        let (path_part, anchor) = match target.split_once('#') {
            Some((p, a)) => (p, Some(a)),
            None => (target.as_str(), None),
        };
        let resolved = if path_part.is_empty() {
            file.to_path_buf()
        } else {
            dir.join(path_part)
        };
        let resolved = normalize(&resolved);
        if !resolved.exists() {
            problems.push(format!("{rel}: broken link `{target}`"));
            continue;
        }
        if let Some(anchor) = anchor.filter(|a| !a.is_empty())
            && is_markdown(&resolved)
        {
            let headings = fs::read_to_string(&resolved)
                .map(|t| anchors(&t))
                .unwrap_or_default();
            if !headings.contains(anchor) {
                problems.push(format!(
                    "{rel}: link `{target}` names no heading `#{anchor}`"
                ));
            }
        }
    }
}

/// Removes `.` and resolves `..` lexically, so `a/../b` checks `b`.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// GitHub-style heading anchors of a Markdown text.
fn anchors(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !trimmed.starts_with('#') {
            continue;
        }
        let heading = trimmed.trim_start_matches('#');
        if !heading.starts_with(' ') {
            continue;
        }
        out.insert(slug(heading.trim()));
    }
    out
}

fn slug(heading: &str) -> String {
    let text = strip_inline_code_keep(heading);
    text.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else if c == ' ' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// Drops backticks but keeps their content, as GitHub does for anchors.
fn strip_inline_code_keep(s: &str) -> String {
    s.chars().filter(|&c| c != '`').collect()
}

fn check_adrs(root: &Path, problems: &mut Vec<String>) {
    let log = fs::read_to_string(root.join(LOG)).unwrap_or_default();
    let Ok(entries) = fs::read_dir(root.join(ADRS)) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_adr_name(&name) {
            problems.push(format!("{ADRS}/{name}: not named ADR-NNNN-slug.md"));
            continue;
        }
        if !log.contains(&format!("(adr/{name})")) {
            problems.push(format!("{ADRS}/{name}: not linked from {LOG}"));
        }
    }
}

fn is_adr_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ADR-") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".md") else {
        return false;
    };
    let Some((num, slug)) = stem.split_once('-') else {
        return false;
    };
    num.len() == 4
        && num.bytes().all(|b| b.is_ascii_digit())
        && !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir, removed when dropped.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("teigeodata-xtask-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const FM: &str = "---\ntitle: \"T\"\nstatus: draft\nupdated: 2026-11-06\n---\n";

    #[test]
    fn a_clean_tree_passes() {
        let t = Tmp::new("clean");
        t.write(
            "docs/a.md",
            &format!("{FM}# A\n\n## Part two\n\nSee [b](b.md#b-title).\n"),
        );
        t.write(
            "docs/b.md",
            &format!("{FM}# B title\n\nBack to [a](a.md#part-two).\n"),
        );
        t.write(
            "README.md",
            "[docs](docs/a.md) and [web](https://example.com)\n",
        );
        assert!(check(&t.0).is_ok(), "{:?}", check(&t.0));
    }

    #[test]
    fn a_broken_link_fails() {
        let t = Tmp::new("broken");
        t.write("docs/a.md", &format!("{FM}[missing](nope.md)\n"));
        let problems = check(&t.0).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("broken link `nope.md`")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_missing_anchor_fails() {
        let t = Tmp::new("anchor");
        t.write("docs/a.md", &format!("{FM}# A\n[x](#not-here)\n"));
        let problems = check(&t.0).unwrap_err();
        assert!(
            problems.iter().any(|p| p.contains("#not-here")),
            "{problems:?}"
        );
    }

    #[test]
    fn links_in_code_are_ignored() {
        let t = Tmp::new("code");
        t.write(
            "docs/a.md",
            &format!("{FM}```\n[x](nope.md)\n```\n`[y](nope.md)`\n"),
        );
        assert!(check(&t.0).is_ok(), "{:?}", check(&t.0));
    }

    #[test]
    fn bad_frontmatter_fails() {
        let t = Tmp::new("fm");
        t.write("docs/none.md", "# no frontmatter\n");
        t.write(
            "docs/status.md",
            "---\ntitle: x\nstatus: maybe\nupdated: 2026-13-01\n---\n",
        );
        let problems = check(&t.0).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("none.md: missing frontmatter"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("unknown status `maybe`"))
        );
        assert!(problems.iter().any(|p| p.contains("2026-13-01")));
    }

    #[test]
    fn research_notes_need_their_sections() {
        let t = Tmp::new("note");
        t.write(
            "docs/02-research/notes/n.md",
            &format!("{FM}## 1. Question\n"),
        );
        let problems = check(&t.0).unwrap_err();
        assert!(problems.iter().any(|p| p.contains("no `owner`")));
        assert!(problems.iter().any(|p| p.contains("`## 9.`")));
    }

    #[test]
    fn adrs_must_be_named_and_logged() {
        let t = Tmp::new("adr");
        t.write(
            "docs/03-decisions/log.md",
            &format!("{FM}[x](adr/ADR-0001-one.md)\n"),
        );
        t.write("docs/03-decisions/adr/ADR-0001-one.md", FM);
        t.write("docs/03-decisions/adr/ADR-0002-two.md", FM);
        t.write("docs/03-decisions/adr/adr-3.md", FM);
        let problems = check(&t.0).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("ADR-0002-two.md: not linked"))
        );
        assert!(problems.iter().any(|p| p.contains("adr-3.md: not named")));
        assert!(
            !problems.iter().any(|p| p.contains("ADR-0001")),
            "{problems:?}"
        );
    }

    #[test]
    fn slugs_match_github() {
        assert_eq!(
            slug("Fixing data (no Rust needed)"),
            "fixing-data-no-rust-needed"
        );
        assert_eq!(slug("1. Resume protocol"), "1-resume-protocol");
        assert_eq!(slug("`cargo xtask` tasks"), "cargo-xtask-tasks");
    }

    #[test]
    fn dates_are_checked() {
        assert!(is_date("2026-11-06"));
        assert!(!is_date("2026-11-6"));
        assert!(!is_date("YYYY-MM-DD"));
        assert!(!is_date("2026-00-10"));
    }
}
