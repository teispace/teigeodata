//! `cargo xtask ids save|restore`: the ID registries kept durably with the data releases
//! (D-261, Q-025).
//!
//! A registry maps each stable ID's serial to the source keys it was given for (ADR-0008); the
//! forge reads the last one to keep every published ID (D-240, D-260). The registries are
//! build output under `data/build/`, so a lost disk or a clean checkout would lose them: every
//! data release therefore carries `ids-<build>.tar.gz`, and `restore` puts it back.
//!
//! - `save <build> <out>`: the world part's registries (`baseline/world/world-ids-<build>/`)
//!   and every pack's (`pack/<cc>/<cc>-ids-<build>.tsv`), paths relative to `data/build/`.
//! - `restore [<tag>] [--build <build>] [--force]`: the archive of release `<tag>`, else of the
//!   newest `data-*` release that has one. A local registry that differs is kept and reported
//!   unless `--force`, so a newer local registry is never overwritten by accident.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every registry file's first line (`teigeodata id registry 1`).
const HEADER: &str = "# teigeodata id registry";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let usage = "usage: cargo xtask ids save <lite|main> <out.tar.gz> | ids restore [<tag>] \
                 [--build <lite|main>] [--force]";
    match args.first().map(String::as_str) {
        Some("save") => match (args.get(1), args.get(2)) {
            (Some(build), Some(out)) => save(root, build, Path::new(out)).map_err(one),
            _ => Err(one(usage.into())),
        },
        Some("restore") => {
            let mut tag = None;
            let mut build = "lite";
            let mut force = false;
            let mut it = args.iter().skip(1);
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--force" => force = true,
                    "--build" => build = it.next().ok_or_else(|| one(usage.into()))?,
                    t if !t.starts_with('-') && tag.is_none() => tag = Some(t.to_string()),
                    _ => return Err(one(usage.into())),
                }
            }
            restore(root, tag.as_deref(), build, force)
        }
        _ => Err(one(usage.into())),
    }
}

fn check_build(build: &str) -> Result<(), String> {
    if matches!(build, "lite" | "main") {
        Ok(())
    } else {
        Err(format!("{build}: not a build (lite or main)"))
    }
}

/// The archive's name for `build`.
pub(crate) fn asset(build: &str) -> String {
    format!("ids-{build}.tar.gz")
}

/// The registry files of `build` under `data/build/`, relative to it, sorted.
fn registries(build_dir: &Path, build: &str) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let world = Path::new("baseline/world").join(format!("world-ids-{build}"));
    if let Ok(entries) = fs::read_dir(build_dir.join(&world)) {
        for e in entries.flatten() {
            let name = e.file_name();
            if Path::new(&name).extension().is_some_and(|x| x == "tsv") {
                out.push(world.join(name));
            }
        }
    }
    if let Ok(packs) = fs::read_dir(build_dir.join("pack")) {
        for cc in packs.flatten() {
            let cc = cc.file_name();
            let rel = Path::new("pack")
                .join(&cc)
                .join(format!("{}-ids-{build}.tsv", cc.to_string_lossy()));
            if build_dir.join(&rel).is_file() {
                out.push(rel);
            }
        }
    }
    for rel in &out {
        let text = fs::read_to_string(build_dir.join(rel))
            .map_err(|e| format!("{}: {e}", rel.display()))?;
        if !text.starts_with(HEADER) {
            return Err(format!("{}: not an ID registry", rel.display()));
        }
    }
    out.sort();
    Ok(out)
}

fn save(root: &Path, build: &str, out: &Path) -> Result<String, String> {
    check_build(build)?;
    let dir = root.join("data/build");
    let files = registries(&dir, build)?;
    if files.is_empty() {
        return Err(format!("no {build} ID registries under {}", dir.display()));
    }
    let out = if out.is_absolute() {
        out.to_path_buf()
    } else {
        root.join(out)
    };
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&out)
        .arg("-C")
        .arg(&dir)
        .args(&files)
        .status()
        .map_err(|e| format!("tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar failed: {status}"));
    }
    let bytes = fs::metadata(&out).map_err(|e| e.to_string())?.len();
    Ok(format!(
        "ids: {} {build} registries → {} ({bytes} B)",
        files.len(),
        out.display()
    ))
}

fn restore(
    root: &Path,
    tag: Option<&str>,
    build: &str,
    force: bool,
) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    check_build(build).map_err(one)?;
    let name = asset(build);
    let tag = match tag {
        Some(t) => t.to_string(),
        None => newest_with(root, &name).map_err(one)?,
    };
    let dir = root.join("data/build");
    let scratch = dir.join(".ids-restore");
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).map_err(|e| one(e.to_string()))?;
    let result = (|| {
        crate::mirror::gh(
            root,
            &[
                "release",
                "download",
                &tag,
                "-p",
                &name,
                "-D",
                &scratch.display().to_string(),
            ],
        )
        .map_err(one)?;
        let unpacked = scratch.join("unpacked");
        fs::create_dir_all(&unpacked).map_err(|e| one(e.to_string()))?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(scratch.join(&name))
            .arg("-C")
            .arg(&unpacked)
            .status()
            .map_err(|e| one(format!("tar: {e}")))?;
        if !status.success() {
            return Err(one(format!("tar failed: {status}")));
        }
        place(&unpacked, &dir, build, force)
    })();
    let _ = fs::remove_dir_all(&scratch);
    result.map(|s| format!("ids: {tag}: {s}"))
}

/// Copies the registries of `from` to `to`: a missing one is written, an equal one left, a
/// differing one kept (a problem) unless `force`.
fn place(from: &Path, to: &Path, build: &str, force: bool) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let files = registries(from, build).map_err(one)?;
    if files.is_empty() {
        return Err(one(format!("the archive holds no {build} registry")));
    }
    let (mut written, mut same) = (0, 0);
    let mut problems = Vec::new();
    for rel in &files {
        let bytes = fs::read(from.join(rel)).map_err(|e| one(e.to_string()))?;
        let dest = to.join(rel);
        match fs::read(&dest) {
            Ok(local) if local == bytes => same += 1,
            Ok(_) if !force => problems.push(format!(
                "{}: differs from the release's; kept (pass --force to replace it)",
                dest.display()
            )),
            _ => {
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p).map_err(|e| one(e.to_string()))?;
                }
                fs::write(&dest, &bytes).map_err(|e| one(format!("{}: {e}", dest.display())))?;
                written += 1;
            }
        }
    }
    if problems.is_empty() {
        Ok(format!(
            "{} {build} registries: {written} written, {same} already equal",
            files.len()
        ))
    } else {
        Err(problems)
    }
}

/// The newest `data-*` release that carries `asset`.
fn newest_with(root: &Path, asset: &str) -> Result<String, String> {
    let out = Command::new("gh")
        .args([
            "release",
            "list",
            "--limit",
            "100",
            "--json",
            "tagName,createdAt",
            "-q",
            "sort_by(.createdAt) | reverse | .[].tagName",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Err(format!("gh release list failed: {}", out.status));
    }
    for tag in String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|t| t.starts_with("data-"))
    {
        let names = Command::new("gh")
            .args([
                "release",
                "view",
                tag,
                "--json",
                "assets",
                "-q",
                ".assets[].name",
            ])
            .current_dir(root)
            .output()
            .map_err(|e| format!("gh: {e}"))?;
        if String::from_utf8_lossy(&names.stdout)
            .lines()
            .any(|n| n == asset)
        {
            return Ok(tag.to_string());
        }
    }
    Err(format!("no data release carries {asset}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::{place, registries};
    use std::fs;
    use std::path::Path;

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    #[test]
    fn registries_restore_missing_keep_differing_unless_forced() {
        let base = std::env::temp_dir().join(format!("xtask-ids-{}", std::process::id()));
        let (from, to) = (base.join("from"), base.join("to"));
        let reg = "# teigeodata id registry 1\n1\tlite\tgeonames:1\n";
        write(&from, "baseline/world/world-ids-lite/in.tsv", reg);
        write(&from, "pack/np/np-ids-lite.tsv", reg);
        write(&from, "pack/np/np-ids-main.tsv", reg);
        assert_eq!(registries(&from, "lite").unwrap().len(), 2);
        // A newer local registry stays.
        let newer = "# teigeodata id registry 1\n1\tlite\tgeonames:1\n2\tlite\tgeonames:2\n";
        write(&to, "pack/np/np-ids-lite.tsv", newer);
        let err = place(&from, &to, "lite", false).unwrap_err();
        assert!(err.iter().any(|e| e.contains("differs")), "{err:?}");
        assert_eq!(
            fs::read_to_string(to.join("pack/np/np-ids-lite.tsv")).unwrap(),
            newer
        );
        // The missing one was written all the same.
        assert!(to.join("baseline/world/world-ids-lite/in.tsv").is_file());
        let ok = place(&from, &to, "lite", true).unwrap();
        assert!(ok.contains("1 written, 1 already equal"), "{ok}");
        // A file that is not a registry is refused.
        write(&from, "pack/xx/xx-ids-lite.tsv", "nope");
        assert!(registries(&from, "lite").is_err());
        fs::remove_dir_all(&base).unwrap();
    }
}
