//! `cargo xtask mirror <id>…`: uploads the stored bytes of pinned sources to our content-addressed
//! mirror (D-043, D-127), the GitHub release `sources`, one asset per file named by its SHA-256.
//! A source's `mirror_url` (`…/releases/download/sources/{sha256}`) then serves the pinned
//! bytes to every build, CI included, after the upstream moves. A tileset uploads every pinned
//! tile. Assets already on the release are skipped; the release notes list the notices of the
//! sources it holds (their licences ask for them with the data).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// The release that holds the mirror.
pub(crate) const TAG: &str = "sources";

/// Files per `gh release upload` call.
const BATCH: usize = 16;

#[derive(Debug, Deserialize)]
struct Lock {
    #[serde(default, rename = "source")]
    sources: Vec<Source>,
}

#[derive(Debug, Deserialize)]
struct Source {
    id: String,
    kind: String,
    sha256: String,
    size: u64,
    #[serde(default)]
    tiles: Option<String>,
    #[serde(default)]
    mirror_url: Option<String>,
    attribution: String,
}

/// One file to mirror: its source, SHA-256 and size.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct File {
    pub(crate) id: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
}

pub(crate) fn run(root: &Path, ids: &[String]) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let text =
        fs::read_to_string(root.join("data/sources.lock")).map_err(|e| one(e.to_string()))?;
    let lock: Lock = toml::from_str(&text).map_err(|e| one(format!("data/sources.lock: {e}")))?;
    let chosen = choose(&lock, ids).map_err(one)?;
    let mut files = Vec::new();
    for s in &chosen {
        files.extend(files_of(root, s).map_err(one)?);
    }
    let store = root.join("data/cache/store/sha256");
    let mut problems = Vec::new();
    for f in &files {
        match fs::metadata(store.join(&f.sha256)) {
            Ok(m) if m.len() == f.size => {}
            _ => problems.push(format!(
                "{}: {} is not in the store at its pinned size: run `forge fetch {}` first",
                f.id, f.sha256, f.id
            )),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let existing = assets(root).map_err(one)?;
    let todo: Vec<PathBuf> = files
        .iter()
        .filter(|f| !existing.contains(&f.sha256))
        .map(|f| store.join(&f.sha256))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let all: Vec<&Source> = lock
        .sources
        .iter()
        .filter(|s| s.mirror_url.is_some() || chosen.iter().any(|c| c.id == s.id))
        .collect();
    gh(root, &["release", "edit", TAG, "--notes", &notes(&all)]).map_err(one)?;
    for batch in todo.chunks(BATCH) {
        let mut args = vec!["release", "upload", TAG];
        let paths: Vec<String> = batch.iter().map(|p| p.display().to_string()).collect();
        args.extend(paths.iter().map(String::as_str));
        gh(root, &args).map_err(one)?;
    }
    let bytes: u64 = files
        .iter()
        .filter(|f| !existing.contains(&f.sha256))
        .map(|f| f.size)
        .sum();
    Ok(format!(
        "mirror: {} files of {} sources, {} uploaded ({} MB), {} already there",
        files.len(),
        chosen.len(),
        todo.len(),
        bytes.div_ceil(1_000_000),
        files.len() - todo.len()
    ))
}

/// The sources named by `ids`, or every source with a `mirror_url` when none are named.
fn choose<'a>(lock: &'a Lock, ids: &[String]) -> Result<Vec<&'a Source>, String> {
    if ids.is_empty() {
        return Ok(lock
            .sources
            .iter()
            .filter(|s| s.mirror_url.is_some())
            .collect());
    }
    ids.iter()
        .map(|id| {
            lock.sources
                .iter()
                .find(|s| &s.id == id)
                .ok_or_else(|| format!("no source `{id}` in data/sources.lock"))
        })
        .collect()
}

/// The files of a source: its own bytes, or every pinned tile of a tileset.
fn files_of(root: &Path, s: &Source) -> Result<Vec<File>, String> {
    if s.kind != "tileset" {
        return Ok(vec![File {
            id: s.id.clone(),
            sha256: s.sha256.clone(),
            size: s.size,
        }]);
    }
    let rel = s
        .tiles
        .as_deref()
        .ok_or_else(|| format!("{}: a tileset needs a tiles manifest", s.id))?;
    let text = fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    tiles(&s.id, &text)
}

/// The pinned tiles of a manifest (`tile<TAB>sha256|absent<TAB>size`, `#` comments).
fn tiles(id: &str, text: &str) -> Result<Vec<File>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut cells = line.split('\t');
        let (Some(_), Some(hash), Some(size)) = (cells.next(), cells.next(), cells.next()) else {
            return Err(format!(
                "{id}: manifest line {}: expected three cells",
                i + 1
            ));
        };
        if hash == "absent" {
            continue;
        }
        let size = size
            .parse()
            .map_err(|e| format!("{id}: manifest line {}: {e}", i + 1))?;
        out.push(File {
            id: id.to_string(),
            sha256: hash.to_string(),
            size,
        });
    }
    Ok(out)
}

/// The release notes: what the mirror is and the notices of the sources it holds.
fn notes(sources: &[&Source]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for s in sources {
        if !seen.contains(&s.attribution.as_str()) {
            seen.push(&s.attribution);
        }
    }
    let mut out = String::from(
        "Content-addressed mirror of pinned build inputs (D-043, D-127): each asset is named by \
         the SHA-256 of its bytes, as pinned in `data/sources.lock`. Not a release of \
         teigeodata.\n\nNotices of the mirrored sources:\n\n",
    );
    for n in seen {
        out.push_str("- ");
        out.push_str(n);
        out.push('\n');
    }
    out
}

/// The asset names already on the mirror release, creating the release when it is missing.
fn assets(root: &Path) -> Result<BTreeSet<String>, String> {
    let listed = Command::new("gh")
        .args([
            "release",
            "view",
            TAG,
            "--json",
            "assets",
            "-q",
            ".assets[].name",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !listed.status.success() {
        gh(
            root,
            &[
                "release",
                "create",
                TAG,
                "--prerelease",
                "--title",
                "Source mirror",
                "--notes",
                "Content-addressed mirror of pinned build inputs.",
            ],
        )?;
        return Ok(BTreeSet::new());
    }
    Ok(String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn gh(root: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("gh")
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("gh: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "gh {} failed: {status}",
            args.join(" ").chars().take(80).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{File, Lock, choose, notes, tiles};

    const LOCK: &str = r#"
[[source]]
id = "a"
kind = "http"
sha256 = "aa"
size = 1
attribution = "A, CC BY"
mirror_url = "https://m/{sha256}"

[[source]]
id = "b"
kind = "http"
sha256 = "bb"
size = 2
attribution = "A, CC BY"

[[source]]
id = "c"
kind = "tileset"
sha256 = "cc"
size = 3
attribution = "C, CDLA"
"#;

    #[test]
    fn chooses_mirrored_sources_and_lists_tiles() {
        let lock: Lock = toml::from_str(LOCK).unwrap();
        let ids = |v: Vec<&super::Source>| v.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(choose(&lock, &[]).unwrap()), ["a"]);
        assert_eq!(
            ids(choose(&lock, &["c".into(), "b".into()]).unwrap()),
            ["c", "b"]
        );
        assert!(choose(&lock, &["z".into()]).is_err());
        let manifest = "# Tiles of c\nt1\tdd\t10\nt2\tabsent\t0\n\nt3\tee\t20\n";
        assert_eq!(
            tiles("c", manifest).unwrap(),
            [
                File {
                    id: "c".into(),
                    sha256: "dd".into(),
                    size: 10
                },
                File {
                    id: "c".into(),
                    sha256: "ee".into(),
                    size: 20
                },
            ]
        );
        assert!(tiles("c", "t1\tdd\n").is_err());
        assert!(tiles("c", "t1\tdd\tbig\n").is_err());
    }

    #[test]
    fn notes_list_each_notice_once() {
        let lock: Lock = toml::from_str(LOCK).unwrap();
        let all: Vec<&super::Source> = lock.sources.iter().collect();
        let n = notes(&all);
        assert_eq!(n.matches("- A, CC BY\n").count(), 1);
        assert!(n.contains("- C, CDLA\n") && n.contains("SHA-256"));
    }
}
