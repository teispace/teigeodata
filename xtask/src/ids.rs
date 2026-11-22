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
//! - `bridge <cc> <lite.tsv> <main.tsv> <out.tsv>`: lite's IDs to main's (TG-30, Q-038). The
//!   two builds keep their own registries, so one place has two IDs; the bridge maps each live
//!   lite serial to the live main serial sharing most of its source keys, the smaller key set
//!   breaking a tie. A tie left, or no shared key, maps to nothing and says which. A retired
//!   lite ID is not listed: a lite pack's REDIRECTS give its successor first.
//! - `bridges <ids-lite.tar.gz> <ids-main.tar.gz> <out.tar.gz>`: `bridge` for every namespace
//!   both lines number (a country pack's registry, else the world part's; `world` is namespace
//!   0), from two `save` archives; every data release ships it (D-368).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every registry file's first line (`teigeodata id registry 1`).
const HEADER: &str = "# teigeodata id registry";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let usage = "usage: cargo xtask ids save <lite|main> <out.tar.gz> | ids restore [<tag>] \
                 [--build <lite|main>] [--force] | ids bridge <cc> <lite.tsv> <main.tsv> <out.tsv> \
                 | ids bridges <ids-lite.tar.gz> <ids-main.tar.gz> <out.tar.gz>";
    match args.first().map(String::as_str) {
        Some("save") => match (args.get(1), args.get(2)) {
            (Some(build), Some(out)) => save(root, build, Path::new(out)).map_err(one),
            _ => Err(one(usage.into())),
        },
        Some("bridge") => match (args.get(1), args.get(2), args.get(3), args.get(4)) {
            (Some(cc), Some(lite), Some(main), Some(out)) => {
                bridge(cc, Path::new(lite), Path::new(main), Path::new(out)).map_err(one)
            }
            _ => Err(one(usage.into())),
        },
        Some("bridges") => match (args.get(1), args.get(2), args.get(3)) {
            (Some(lite), Some(main), Some(out)) => {
                bridges(Path::new(lite), Path::new(main), Path::new(out)).map_err(one)
            }
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

/// The bridge file's first line.
const BRIDGE_HEADER: &str = "# teigeodata id bridge 1";

/// A registry's live serials (held by some build now) and their source keys.
fn live_serials(text: &str) -> Result<Vec<(u64, BTreeSet<&str>)>, String> {
    let mut lines = text.lines().enumerate();
    if !lines.next().is_some_and(|(_, l)| l.starts_with(HEADER)) {
        return Err(format!("not an ID registry (first line `{HEADER} 1`)"));
    }
    let mut out = Vec::new();
    for (i, line) in lines {
        let mut cols = line.split('\t');
        let (Some(serial), Some(state), Some(keys)) = (cols.next(), cols.next(), cols.next())
        else {
            return Err(format!("line {}: needs three tab-separated columns", i + 1));
        };
        if state == "x" || state.starts_with('>') {
            continue;
        }
        let serial = serial
            .parse()
            .map_err(|_| format!("line {}: bad serial", i + 1))?;
        out.push((serial, keys.split(' ').filter(|k| !k.is_empty()).collect()));
    }
    Ok(out)
}

/// What a lite serial maps to.
#[derive(Debug, PartialEq, Eq)]
enum Bridged {
    /// The main serial, with the keys the two share.
    To(u64, usize),
    /// Several main serials share as many keys, with key sets of one size.
    Tie,
    /// No main serial shares a key.
    None,
}

/// Each lite serial's main serial ([`Bridged`]), in lite's order.
fn bridge_serials(
    lite: &[(u64, BTreeSet<&str>)],
    main: &[(u64, BTreeSet<&str>)],
) -> Vec<(u64, Bridged)> {
    let mut by_key: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, (_, keys)) in main.iter().enumerate() {
        for k in keys {
            by_key.entry(k).or_default().push(i);
        }
    }
    lite.iter()
        .map(|(serial, keys)| {
            let mut shared: BTreeMap<usize, usize> = BTreeMap::new();
            for k in keys {
                for &i in by_key.get(k).into_iter().flatten() {
                    *shared.entry(i).or_default() += 1;
                }
            }
            // Most shared keys first, then the smaller key set (fewer keys the lite place lacks).
            let rank = |(&i, &n): (&usize, &usize)| {
                (
                    n,
                    core::cmp::Reverse(main.get(i).map_or(usize::MAX, |m| m.1.len())),
                )
            };
            let best = shared.iter().map(rank).max();
            let at_best: Vec<usize> = shared
                .iter()
                .filter(|e| Some(rank(*e)) == best)
                .map(|(&i, _)| i)
                .collect();
            let to = match (at_best.as_slice(), best) {
                ([i], Some((n, _))) => main.get(*i).map_or(Bridged::None, |m| Bridged::To(m.0, n)),
                ([], _) => Bridged::None,
                _ => Bridged::Tie,
            };
            (*serial, to)
        })
        .collect()
}

/// The namespace a registry file names: a country code, or `world` (namespace 0).
fn namespace(cc: &str) -> Result<teigeodata_core::Namespace, String> {
    if cc == "world" {
        return Ok(teigeodata_core::Namespace::WORLD);
    }
    <[u8; 2]>::try_from(cc.as_bytes())
        .ok()
        .and_then(teigeodata_core::Namespace::from_country)
        .ok_or_else(|| format!("{cc}: not a two-letter country code or `world`"))
}

/// Each namespace's registry in one line's extracted archive (`dir`), by namespace: a country
/// pack's (`pack/<cc>/<cc>-ids-<build>.tsv`) over the world part's
/// (`baseline/world/world-ids-<build>/<cc>.tsv`), since the world part numbers a pack country
/// through the pack's registry.
fn line_registries(dir: &Path, build: &str) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut out = BTreeMap::new();
    let world = dir.join(format!("baseline/world/world-ids-{build}"));
    if world.is_dir() {
        for e in fs::read_dir(&world).map_err(|e| format!("{}: {e}", world.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if let Some(cc) = p
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".tsv"))
            {
                out.insert(cc.to_string(), p.clone());
            }
        }
    }
    let packs = dir.join("pack");
    if packs.is_dir() {
        for e in fs::read_dir(&packs).map_err(|e| format!("{}: {e}", packs.display()))? {
            let d = e.map_err(|e| e.to_string())?.path();
            let Some(cc) = d.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            let f = d.join(format!("{cc}-ids-{build}.tsv"));
            if f.is_file() {
                out.insert(cc, f);
            }
        }
    }
    Ok(out)
}

/// `ids bridges`: the bridge of every namespace both lines number, from two `ids save`
/// archives, into one archive of `<ns>-ids-bridge.tsv` files (D-368).
fn bridges(lite: &Path, main: &Path, out: &Path) -> Result<String, String> {
    let tmp = std::env::temp_dir().join(format!("xtask-bridges-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let untar = |archive: &Path, to: &Path| -> Result<(), String> {
        fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(to)
            .status()
            .map_err(|e| format!("tar: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{}: tar failed: {status}", archive.display()))
        }
    };
    let run = || -> Result<String, String> {
        let (l, m, o) = (tmp.join("lite"), tmp.join("main"), tmp.join("out"));
        untar(lite, &l)?;
        untar(main, &m)?;
        let (lr, mr) = (line_registries(&l, "lite")?, line_registries(&m, "main")?);
        fs::create_dir_all(&o).map_err(|e| format!("{}: {e}", o.display()))?;
        let mut files = Vec::new();
        let mut lines = Vec::new();
        for (cc, lf) in &lr {
            let Some(mf) = mr.get(cc) else { continue };
            let name = format!("{cc}-ids-bridge.tsv");
            lines.push(bridge(cc, lf, mf, &o.join(&name))?);
            files.push(name);
        }
        if files.is_empty() {
            return Err("no namespace is numbered by both lines".into());
        }
        let out = if out.is_absolute() {
            out.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(out)
        };
        let status = Command::new("tar")
            .env("COPYFILE_DISABLE", "1")
            .arg("-czf")
            .arg(&out)
            .arg("-C")
            .arg(&o)
            .args(&files)
            .status()
            .map_err(|e| format!("tar: {e}"))?;
        if !status.success() {
            return Err(format!("tar failed: {status}"));
        }
        let only_lite = lr.keys().filter(|k| !mr.contains_key(*k)).count();
        let only_main = mr.keys().filter(|k| !lr.contains_key(*k)).count();
        Ok(format!(
            "ids: {} bridges → {} ({only_lite} namespaces only in lite, {only_main} only in main)\n{}",
            files.len(),
            out.display(),
            lines
                .iter()
                .map(|l| l.rsplit('/').next().unwrap_or(l).to_string())
                .collect::<Vec<_>>()
                .join("\n")
        ))
    };
    let r = run();
    let _ = fs::remove_dir_all(&tmp);
    r
}

/// `ids bridge`: writes the lite-to-main bridge of country `cc` (see the module docs).
fn bridge(cc: &str, lite: &Path, main: &Path, out: &Path) -> Result<String, String> {
    let ns = namespace(cc)?;
    let read = |p: &Path| fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let (lite_text, main_text) = (read(lite)?, read(main)?);
    let at = |p: &Path, e: String| format!("{}: {e}", p.display());
    let lite_rows = live_serials(&lite_text).map_err(|e| at(lite, e))?;
    let main_rows = live_serials(&main_text).map_err(|e| at(main, e))?;
    let id = |serial: u64| {
        teigeodata_core::PlaceId::new(ns, serial)
            .map(|i| i.to_string())
            .ok_or_else(|| format!("serial {serial}: not an ID"))
    };
    let mut text = format!(
        "{BRIDGE_HEADER}: {cc} lite to main (D-368). Columns: lite ID, main ID (empty when none), \
         how: `keys N` shared, `tie`, or `none`.\n"
    );
    let (mut to, mut tie, mut none) = (0, 0, 0);
    for (serial, b) in bridge_serials(&lite_rows, &main_rows) {
        let (main_id, how) = match b {
            Bridged::To(m, n) => {
                to += 1;
                (id(m)?, format!("keys {n}"))
            }
            Bridged::Tie => {
                tie += 1;
                (String::new(), "tie".into())
            }
            Bridged::None => {
                none += 1;
                (String::new(), "none".into())
            }
        };
        let _ = writeln!(text, "{}\t{main_id}\t{how}", id(serial)?);
    }
    fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(format!(
        "{}: {to} lite IDs to a main ID, {tie} ties, {none} with no shared key",
        out.display()
    ))
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
            // Hidden files are not registries: macOS tar once packed an AppleDouble `._x.tsv`
            // beside each (data-2026.11.14.3), which is not text.
            let hidden = name.to_string_lossy().starts_with('.');
            if !hidden && Path::new(&name).extension().is_some_and(|x| x == "tsv") {
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
        // macOS tar otherwise adds an AppleDouble `._` file for each file with extended
        // attributes; GNU tar ignores the variable.
        .env("COPYFILE_DISABLE", "1")
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
    use super::{Bridged, bridge_serials, bridges, live_serials, namespace, place, registries};
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
        // An AppleDouble file beside a registry, as macOS tar packs one: not a registry.
        write(
            &from,
            "baseline/world/world-ids-lite/._in.tsv",
            "\u{0}\u{5}\u{16}\u{7}",
        );
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

    #[test]
    fn the_bridge_takes_most_shared_keys_then_the_smaller_set_and_names_ties() {
        let lite = "# teigeodata id registry 1\n\
            1\tlite\tgeonames:1 wikidata:Q1\n\
            2\tlite\tgeonames:2\n\
            3\tlite\tgeonames:3\n\
            4\tlite\tgeonames:4\n\
            5\t>1\tgeonames:5\n\
            6\tx\tgeonames:6\n\
            7\tlite\tgeonames:7 wikidata:Q7\n";
        let main = "# teigeodata id registry 1\n\
            10\tmain\tgeonames:1 osm:node/1 wikidata:Q1\n\
            11\tmain\tgeonames:2 osm:node/2\n\
            12\tmain\tgeonames:2 osm:node/3 osm:way/3\n\
            13\tmain\tgeonames:3 osm:node/4\n\
            14\tmain\tgeonames:3 osm:node/5\n\
            15\tx\tgeonames:4\n\
            16\tmain\tgeonames:7\n\
            17\tmain\twikidata:Q7 osm:node/7\n";
        let (l, m) = (live_serials(lite).unwrap(), live_serials(main).unwrap());
        assert_eq!(l.len(), 5, "retired and gone lite serials are left out");
        assert_eq!(
            bridge_serials(&l, &m),
            [
                (1, Bridged::To(10, 2)),
                (2, Bridged::To(11, 1)),
                (3, Bridged::Tie),
                (4, Bridged::None),
                (7, Bridged::To(16, 1)),
            ]
        );
        assert!(live_serials("1\tlite\tgeonames:1\n").is_err());
    }

    #[test]
    fn bridges_cover_every_namespace_both_lines_number() {
        let base = std::env::temp_dir().join(format!("xtask-bridges-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let h = "# teigeodata id registry 1\n";
        // np: a pack registry in both lines, and a world file in lite that the pack overrides;
        // world: namespace 0; us: only in lite; tk: only in main.
        write(
            &base,
            "lite/pack/np/np-ids-lite.tsv",
            &format!("{h}1\tlite\tgeonames:1\n"),
        );
        write(
            &base,
            "lite/baseline/world/world-ids-lite/np.tsv",
            &format!("{h}1\tlite\tgeonames:9\n"),
        );
        write(
            &base,
            "lite/baseline/world/world-ids-lite/world.tsv",
            &format!("{h}1\tlite\tne:sea\n"),
        );
        write(
            &base,
            "lite/baseline/world/world-ids-lite/us.tsv",
            &format!("{h}1\tlite\tgeonames:5\n"),
        );
        write(
            &base,
            "main/pack/np/np-ids-main.tsv",
            &format!("{h}7\tmain\tgeonames:1 osm:node/1\n"),
        );
        write(
            &base,
            "main/baseline/world/world-ids-main/world.tsv",
            &format!("{h}3\tmain\tne:sea\n"),
        );
        write(
            &base,
            "main/baseline/world/world-ids-main/tk.tsv",
            &format!("{h}1\tmain\tgeonames:6\n"),
        );
        let tar = |dir: &str, out: &str| {
            let ok = std::process::Command::new("tar")
                .env("COPYFILE_DISABLE", "1")
                .arg("-czf")
                .arg(base.join(out))
                .arg("-C")
                .arg(base.join(dir))
                .arg(".")
                .status()
                .unwrap()
                .success();
            assert!(ok);
        };
        tar("lite", "ids-lite.tar.gz");
        tar("main", "ids-main.tar.gz");
        let out = base.join("bridge.tar.gz");
        let said = bridges(
            &base.join("ids-lite.tar.gz"),
            &base.join("ids-main.tar.gz"),
            &out,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            said.contains("2 bridges") && said.contains("1 namespaces only in lite"),
            "{said}"
        );
        let x = base.join("x");
        fs::create_dir_all(&x).unwrap();
        assert!(
            std::process::Command::new("tar")
                .arg("-xzf")
                .arg(&out)
                .arg("-C")
                .arg(&x)
                .status()
                .unwrap()
                .success()
        );
        let np = fs::read_to_string(x.join("np-ids-bridge.tsv")).unwrap();
        assert!(
            np.lines().any(|l| l == "tgd:np/1\ttgd:np/7\tkeys 1"),
            "{np}"
        );
        let world = fs::read_to_string(x.join("world-ids-bridge.tsv")).unwrap();
        assert!(
            world
                .lines()
                .any(|l| l == "tgd:world/1\ttgd:world/3\tkeys 1"),
            "{world}"
        );
        assert!(!x.join("us-ids-bridge.tsv").exists() && !x.join("tk-ids-bridge.tsv").exists());
        assert!(namespace("xyz").is_err() && namespace("np").is_ok());
        let _ = fs::remove_dir_all(&base);
    }
}
