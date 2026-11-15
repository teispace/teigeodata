//! Measured records: a gate's own table, written with `--record` to `site/data/measured/` and
//! stamped with when, where and from which commit it was measured, for the site's measured page
//! (`cargo xtask data-pages`, phase 7.5b, [research](../../docs/02-research/notes/docs-data-pages.md)).
//!
//! The gate keeps one way to show its figures (its Markdown table); the record only adds the
//! stamp, so a number on the site always says how old it is and what it was measured on.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Where the records live, relative to the repository root.
pub(crate) const DIR: &str = "site/data/measured";

/// One gate's figures as the site shows them.
#[derive(Serialize, Deserialize)]
pub(crate) struct Record {
    /// Where the section sits on the page (lower first).
    pub(crate) order: u32,
    pub(crate) title: String,
    /// What is measured and what holds it (a budget, a gate), in a sentence or two of Markdown.
    pub(crate) intro: String,
    /// The command that measured it.
    pub(crate) command: String,
    /// The UTC day it ran.
    pub(crate) date: String,
    /// The commit measured, `+changes` when the tree had uncommitted changes.
    pub(crate) commit: String,
    /// The operating system, architecture and logical cores.
    pub(crate) machine: String,
    /// The one-minute load average when it finished: timings from a busy machine read high.
    pub(crate) load: String,
    /// The gate's own Markdown table.
    pub(crate) table: String,
}

impl Record {
    /// A record of `table`, stamped now.
    pub(crate) fn new(
        root: &Path,
        order: u32,
        title: &str,
        intro: &str,
        command: &str,
        table: &str,
    ) -> Self {
        Self {
            order,
            title: title.to_owned(),
            intro: intro.to_owned(),
            command: command.to_owned(),
            date: output(root, "date", &["-u", "+%Y-%m-%d"]).unwrap_or_default(),
            commit: commit(root),
            machine: machine(root),
            load: load(root),
            table: table.trim().to_owned(),
        }
    }

    /// Writes it to `site/data/measured/<name>.json`; the path written.
    pub(crate) fn write(&self, root: &Path, name: &str) -> Result<String, Vec<String>> {
        let dir = root.join(DIR);
        fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        let path = dir.join(format!("{name}.json"));
        let mut text =
            serde_json::to_string_pretty(self).map_err(|e| vec![format!("{name}: {e}")])?;
        text.push('\n');
        fs::write(&path, text).map_err(|e| vec![format!("{}: {e}", path.display())])?;
        Ok(format!("{DIR}/{name}.json"))
    }
}

fn output(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn commit(root: &Path) -> String {
    let head = output(root, "git", &["rev-parse", "--short=7", "HEAD"]).unwrap_or_default();
    let dirty = output(
        root,
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .is_some_and(|s| !s.is_empty());
    if dirty {
        format!("{head}+changes")
    } else {
        head
    }
}

fn machine(root: &Path) -> String {
    let os = output(root, "uname", &["-sm"]).unwrap_or_default();
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    format!("{os}, {cores} cores")
}

/// The one-minute load average, from `uptime` (the same words on macOS and Linux).
fn load(root: &Path) -> String {
    output(root, "uptime", &[])
        .and_then(|u| {
            u.rsplit_once("load average").and_then(|(_, rest)| {
                rest.trim_start_matches(['s', ':', ' '])
                    .split([',', ' '])
                    .next()
                    .map(str::to_owned)
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::Record;

    #[test]
    fn a_record_round_trips_and_is_stamped() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let r = Record::new(root, 1, "T", "I", "cargo xtask x", "| a |\n|---|\n| 1 |\n");
        assert_eq!(r.date.len(), 10, "{}", r.date);
        assert!(r.machine.contains("cores"), "{}", r.machine);
        assert!(!r.load.is_empty(), "no load average");
        assert!(r.load.parse::<f64>().is_ok(), "{}", r.load);
        let text = serde_json::to_string(&r).unwrap_or_default();
        let back: Record = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back.table, "| a |\n|---|\n| 1 |");
    }
}
