//! `cargo xtask mirror <id>…`: uploads the stored bytes of pinned sources to our content-addressed
//! mirror (D-043, D-127), GitHub releases of this repository holding one asset per file named by
//! its SHA-256. A source's `mirror_url` (`…/releases/download/<tag>/{sha256}`) names its release
//! and then serves the pinned bytes to every build, CI included, after the upstream moves. A
//! tileset uploads every pinned tile. Assets already on a release are skipped; the release notes
//! list the notices of the sources it holds (their licences ask for them with the data).
//!
//! Releases on this repository are immutable once published (D-387): the first mirror release,
//! `sources`, takes no new asset. A newly pinned source names a new release, `sources-<date>`;
//! the command creates it as a draft, uploads, then publishes it, after which it is fixed too.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// The first mirror release; later ones are `sources-<date>` (D-387).
const FIRST: &str = "sources";

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

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let check = args.iter().any(|a| a == "--check");
    let ids: Vec<String> = args.iter().filter(|a| *a != "--check").cloned().collect();
    let text =
        fs::read_to_string(root.join("data/sources.lock")).map_err(|e| one(e.to_string()))?;
    let lock: Lock = toml::from_str(&text).map_err(|e| one(format!("data/sources.lock: {e}")))?;
    let chosen = choose(&lock, &ids).map_err(one)?;
    let mut files = Vec::new();
    for s in &chosen {
        files.extend(files_of(root, s).map_err(one)?);
    }
    if check {
        let mut on = BTreeSet::new();
        for (tag, _) in by_release(&chosen, &files).map_err(one)? {
            on.extend(
                listed(root, &tag)
                    .map_err(one)?
                    .map(|r| r.assets)
                    .unwrap_or_default(),
            );
        }
        return verify(&files, &on);
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
    let mut existing = BTreeSet::new();
    let mut todo: Vec<PathBuf> = Vec::new();
    for (tag, group) in by_release(&chosen, &files).map_err(one)? {
        let all: Vec<&Source> = lock
            .sources
            .iter()
            .filter(|s| tag_of(s).ok().flatten().as_deref() == Some(tag.as_str()))
            .collect();
        let release = release(root, &tag, &notes(&all)).map_err(one)?;
        let new: Vec<PathBuf> = group
            .iter()
            .filter(|f| !release.assets.contains(&f.sha256))
            .map(|f| store.join(&f.sha256))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !new.is_empty() && release.immutable {
            return Err(one(format!(
                "the release `{tag}` is published and immutable: point the new sources' \
                 mirror_url at a new release, `{FIRST}-<date>` (D-387)"
            )));
        }
        for batch in new.chunks(BATCH) {
            let mut args = vec!["release", "upload", tag.as_str()];
            let paths: Vec<String> = batch.iter().map(|p| p.display().to_string()).collect();
            args.extend(paths.iter().map(String::as_str));
            gh(root, &args).map_err(one)?;
        }
        if release.draft {
            gh(root, &["release", "edit", &tag, "--draft=false"]).map_err(one)?;
        }
        existing.extend(release.assets);
        todo.extend(new);
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

/// `--check`: every pinned file of the chosen sources is on the mirror release. A source that
/// names a mirror but was never uploaded builds only while its upstream keeps the file; this
/// finds it before the upstream deletes it (two sources failed a build that way, 2026-11-15).
fn verify(files: &[File], on_release: &BTreeSet<String>) -> Result<String, Vec<String>> {
    let missing: Vec<String> = files
        .iter()
        .filter(|f| !on_release.contains(&f.sha256))
        .map(|f| {
            format!(
                "{}: {} ({} B) is not on its mirror release: run `cargo xtask mirror {}`",
                f.id, f.sha256, f.size, f.id
            )
        })
        .collect();
    if missing.is_empty() {
        Ok(format!(
            "mirror ok: {} files on the mirror releases",
            files.len()
        ))
    } else {
        Err(missing)
    }
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

/// The release a source's `mirror_url` names (`…/releases/download/<tag>/{sha256}`), if any.
fn tag_of(s: &Source) -> Result<Option<String>, String> {
    let Some(url) = s.mirror_url.as_deref().filter(|u| !u.is_empty()) else {
        return Ok(None);
    };
    let tag = url
        .strip_suffix("/{sha256}")
        .and_then(|u| u.rsplit_once("/releases/download/"))
        .map(|(_, t)| t)
        .filter(|t| !t.is_empty() && !t.contains('/'))
        .ok_or_else(|| {
            format!(
                "{}: mirror_url is not …/releases/download/<tag>/{{sha256}}",
                s.id
            )
        })?;
    Ok(Some(tag.to_string()))
}

/// The files grouped by the release their source's `mirror_url` names; a chosen source with
/// none goes to the first release, as before D-387.
fn by_release<'f>(
    chosen: &[&Source],
    files: &'f [File],
) -> Result<std::collections::BTreeMap<String, Vec<&'f File>>, String> {
    let mut out = std::collections::BTreeMap::<String, Vec<&File>>::new();
    for s in chosen {
        let tag = tag_of(s)?.unwrap_or_else(|| FIRST.to_string());
        let group = out.entry(tag).or_default();
        group.extend(files.iter().filter(|f| f.id == s.id));
    }
    Ok(out)
}

/// A mirror release as GitHub has it.
struct Release {
    assets: BTreeSet<String>,
    draft: bool,
    immutable: bool,
}

/// The release `tag`, or None when it does not exist.
fn listed(root: &Path, tag: &str) -> Result<Option<Release>, String> {
    let out = Command::new("gh")
        .args([
            "release",
            "view",
            tag,
            "--json",
            "assets,isDraft,isImmutable",
            "-q",
            "[.isDraft, .isImmutable] + [.assets[].name] | .[]",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let draft = lines.next() == Some("true");
    let immutable = lines.next() == Some("true");
    Ok(Some(Release {
        assets: lines.map(str::to_string).collect(),
        draft,
        immutable,
    }))
}

/// The release `tag` with `notes`: created as a draft when missing (so assets can be added
/// before it is published and fixed), its notes updated while it is still a draft.
fn release(root: &Path, tag: &str, notes: &str) -> Result<Release, String> {
    if let Some(r) = listed(root, tag)? {
        if r.draft {
            gh(root, &["release", "edit", tag, "--notes", notes])?;
        }
        return Ok(r);
    }
    gh(
        root,
        &[
            "release",
            "create",
            tag,
            "--draft",
            "--prerelease",
            "--title",
            "Source mirror",
            "--notes",
            notes,
        ],
    )?;
    Ok(Release {
        assets: BTreeSet::new(),
        draft: true,
        immutable: false,
    })
}

pub(crate) fn gh(root: &Path, args: &[&str]) -> Result<(), String> {
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
    use std::collections::BTreeSet;

    use super::{File, Lock, choose, notes, tag_of, tiles, verify};

    #[test]
    fn check_names_each_file_missing_from_the_release() {
        let f = |id: &str, h: &str| File {
            id: id.to_owned(),
            sha256: h.to_owned(),
            size: 3,
        };
        let files = [f("a", "11"), f("b", "22"), f("b", "33")];
        let on: BTreeSet<String> = ["11".to_owned(), "33".to_owned()].into();
        let e = verify(&files, &on).unwrap_err();
        assert_eq!(e.len(), 1);
        assert!(e[0].starts_with("b: 22 "), "{e:?}");
        let all: BTreeSet<String> = ["11", "22", "33"].map(str::to_owned).into();
        assert!(verify(&files, &all).is_ok());
    }

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

    #[test]
    fn each_source_names_its_mirror_release() {
        let lock: Lock = toml::from_str(&LOCK.replace(
            "https://m/{sha256}",
            "https://github.com/o/r/releases/download/sources-2026.11.23/{sha256}",
        ))
        .unwrap();
        assert_eq!(
            tag_of(&lock.sources[0]).unwrap().as_deref(),
            Some("sources-2026.11.23")
        );
        assert_eq!(tag_of(&lock.sources[1]).unwrap(), None);
        let bad: Lock = toml::from_str(LOCK).unwrap();
        assert!(tag_of(&bad.sources[0]).is_err());
    }
}
