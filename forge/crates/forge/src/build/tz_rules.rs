//! Zone rules (D-196, TG-03, [research note §5.5]): the pack's timezones' rules from the same
//! tzdb release as its zone outlines.
//!
//! The release is the version of the build's timezone source (`tbb-2026f` or `tzdb-2026f`):
//! its data `tzdb-<version>` and code `tzcode-<version>` are pinned in `data/sources.lock`.
//! `zic` is built from that tzcode with the system C compiler, so the output never depends on
//! the host's `zic` (macOS ships tzcode 2022g, whose slim output differs from 2026f's in 40 of
//! 598 files), and run with `-b slim` on the data files in the Makefile's order, `backward`
//! last. Each zone the pack's timezone areas name is read back with the format crate's strict
//! TZif parser in the pack stage; a zone without a file fails the build (G-tzrules). The
//! scratch tree is removed afterwards.
//!
//! [research note §5.5]: ../../../../../docs/02-research/notes/tz-rules.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use teigeodata_format::tz_rules::{self as rules, TzRules};

use super::zones::Zones;
use crate::lock::Lock;

/// tzdb's data files, in the order of its Makefile's `TDATA` (`backward` last).
const DATA_FILES: [&str; 10] = [
    "africa",
    "antarctica",
    "asia",
    "australasia",
    "europe",
    "northamerica",
    "southamerica",
    "etcetera",
    "factory",
    "backward",
];

/// The compiled rules of a build: the tzdb release and each zone's slim TZif bytes.
#[derive(Debug)]
pub(crate) struct Compiled {
    pub(crate) version: String,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
}

/// The tzdb release a timezone source belongs to, and the IDs of its pinned data and code.
pub(crate) fn sources(lock: &Lock, tz: &str) -> Result<(String, [String; 2]), String> {
    let version = lock
        .get(tz)?
        .version
        .clone()
        .ok_or_else(|| format!("{tz}: no version in data/sources.lock"))?;
    let ids = [format!("tzdb-{version}"), format!("tzcode-{version}")];
    for id in &ids {
        lock.get(id).map_err(|e| {
            format!("G-tzrules: {e}: the rules come from the outlines' tzdb release")
        })?;
    }
    Ok((version, ids))
}

/// The zone name of a timezone place's key (`tz:Asia/Kathmandu`).
pub(super) fn zone_name(key: &str) -> Option<&str> {
    key.strip_prefix("tz:")
}

/// Compiles the rules of `zones` from the release of timezone source `tz`; `None` without
/// zones.
///
/// # Errors
/// A source missing or not matching its pin, a failed compile, or a zone `zic` wrote no file
/// for (G-tzrules).
pub(super) fn compile(
    root: &Path,
    lock: &Lock,
    tz: Option<&str>,
    zones: &Zones,
) -> Result<Option<Compiled>, String> {
    let (Some(tz), false) = (tz, zones.places.is_empty()) else {
        return Ok(None);
    };
    let names: Vec<&str> = zones
        .places
        .iter()
        .map(|p| zone_name(&p.key).ok_or_else(|| format!("{}: not a zone key", p.key)))
        .collect::<Result<_, _>>()?;
    compile_names(root, lock, tz, &names).map(Some)
}

/// Compiles the rules of the zones `names` from the release of timezone source `tz`: one code
/// path for a pack's areas and an embedded layer's (D-224).
///
/// # Errors
/// As [`compile`].
pub(super) fn compile_names(
    root: &Path,
    lock: &Lock,
    tz: &str,
    names: &[&str],
) -> Result<Compiled, String> {
    let (version, [data, code]) = sources(lock, tz)?;
    let scratch = root.join("data/build/tz-rules").join(&version);
    let result = run(root, lock, (&data, &code), &version, &scratch, names);
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn run(
    root: &Path,
    lock: &Lock,
    (data, code): (&str, &str),
    version: &str,
    scratch: &Path,
    names: &[&str],
) -> Result<Compiled, String> {
    let _ = fs::remove_dir_all(scratch);
    let (src, out) = (scratch.join("src"), scratch.join("out"));
    fs::create_dir_all(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    for id in [code, data] {
        let path = lock.path(root, id)?;
        let (sha, _) = crate::fetch::hash_file(&path)?;
        if sha != lock.get(id)?.sha256 {
            return Err(format!("{id}: the stored file does not match its pin"));
        }
        for (name, bytes) in super::zones::tar_entries(&path)? {
            fs::write(src.join(&name), bytes).map_err(|e| format!("{name}: {e}"))?;
        }
    }
    // The headers tzcode's Makefile generates; zic reads no installed paths with `-d`.
    fs::write(
        src.join("version.h"),
        format!(
            "static char const PKGVERSION[]=\"(tzcode) \";\nstatic char const TZVERSION[]=\"{version}\";\nstatic char const REPORT_BUGS_TO[]=\"tz@iana.org\";\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        src.join("tzdir.h"),
        "#ifndef TZDEFAULT\n# define TZDEFAULT \"/etc/localtime\"\n#endif\n#ifndef TZDIR\n# define TZDIR \"/usr/share/zoneinfo\"\n#endif\n",
    )
    .map_err(|e| e.to_string())?;
    let zic = src.join("zic");
    exec(
        Command::new("cc")
            .args(["-O2", "-o", "zic", "zic.c"])
            .current_dir(&src),
        "cc zic.c (tzcode)",
    )?;
    let said = exec(Command::new(&zic).arg("--version"), "zic --version")?;
    if !said.split_whitespace().any(|w| w == version) {
        return Err(format!("zic says {said:?}, not {version}"));
    }
    let mut cmd = Command::new(&zic);
    cmd.args(["-b", "slim", "-d"]).arg(&out);
    cmd.args(DATA_FILES.iter().map(|f| src.join(f)));
    exec(&mut cmd, "zic -b slim")?;
    let mut files = BTreeMap::new();
    for &name in names {
        if !rules::is_zone_name(name) {
            return Err(format!("G-tzrules: {name:?} is not a tzdb zone name"));
        }
        let path: PathBuf = out.join(name);
        let bytes = fs::read(&path)
            .map_err(|_| format!("G-tzrules: tzdb {version} has no rules for the zone {name}"))?;
        files.insert(name.to_string(), bytes);
    }
    Ok(Compiled {
        version: version.to_string(),
        files,
    })
}

/// Runs `cmd`; its standard output, or an error naming `what` with its standard error.
fn exec(cmd: &mut Command, what: &str) -> Result<String, String> {
    let o = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{what}: {}: {}",
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The `TZ_RULES` payload of a pack whose timezone areas are `(zone name, PLACES row)`.
pub(super) fn section(compiled: &Compiled, zones: &[(&str, u32)]) -> Result<Vec<u8>, String> {
    let parsed: Vec<(&str, rules::ZoneRules, u32)> = zones
        .iter()
        .map(|&(name, row)| {
            let bytes = compiled
                .files
                .get(name)
                .ok_or_else(|| format!("G-tzrules: no compiled rules for the zone {name}"))?;
            let r = rules::parse_tzif(bytes).map_err(|e| format!("{name}: {e}"))?;
            Ok((name, r, row))
        })
        .collect::<Result<_, String>>()?;
    let inputs: Vec<rules::ZoneInput<'_>> = parsed
        .iter()
        .map(|(name, rules, row)| rules::ZoneInput {
            name,
            rules,
            place: Some(*row),
        })
        .collect();
    rules::encode(&inputs, false).map_err(|e| format!("TZ_RULES: {e}"))
}

/// G-tzrules on a written pack: `TZ_RULES` passes its check, holds exactly the pack's
/// timezone areas (each name linked to its area's row), rebuilds each zone's TZif as `zic`
/// wrote it, and the manifest's `tzdb` is the release compiled.
pub(super) fn check(
    rules: &TzRules<'_>,
    tzdb: Option<&str>,
    zones: &[(&str, u32)],
    compiled: &Compiled,
) -> Result<(), String> {
    rules.check().map_err(|e| format!("G-tzrules: {e}"))?;
    if tzdb != Some(compiled.version.as_str()) {
        return Err(format!(
            "G-tzrules: the manifest's tzdb is {tzdb:?}, the rules are {}",
            compiled.version
        ));
    }
    if rules.len() != zones.len() {
        return Err(format!(
            "G-tzrules: {} zones have rules, the pack has {} timezone areas",
            rules.len(),
            zones.len()
        ));
    }
    for &(name, row) in zones {
        let z = rules
            .zone(name)
            .ok_or_else(|| format!("G-tzrules: the zone {name} has no rules"))?;
        if rules.zone_of_place(row).map(|z| z.name()) != Some(name) {
            return Err(format!("G-tzrules: {name} is not linked to its row {row}"));
        }
        let want = compiled
            .files
            .get(name)
            .ok_or("G-tzrules: a zone not compiled")?;
        if &z.tzif().map_err(|e| e.to_string())? != want {
            return Err(format!(
                "G-tzrules: {name} does not rebuild as zic wrote it"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Compiled, check, section};
    use teigeodata_format::tz_rules::{TypeRow, TzRules, ZoneInput, ZoneRules, encode};
    use teigeodata_format::{Kind, Section};

    /// A footer-only zone's TZif bytes.
    fn tzif(std: &str, offset: i32, abbr: &str) -> Vec<u8> {
        let footer = teigeodata_format::tz_rules::parse_footer(std)
            .ok()
            .flatten();
        let z = ZoneRules {
            version: 2,
            transitions: Vec::new(),
            types: vec![TypeRow {
                offset,
                is_dst: false,
                abbr: abbr.into(),
            }],
            footer,
            first_abbr: 0,
        };
        let one = [ZoneInput {
            name: "A/B",
            rules: &z,
            place: None,
        }];
        let b = encode(&one, false).unwrap_or_default();
        let s = Section::unpacked(Kind::TZ_RULES, 1, &b);
        TzRules::open(&s)
            .ok()
            .and_then(|r| r.zone("A/B").and_then(|z| z.tzif().ok()))
            .unwrap_or_default()
    }

    #[test]
    fn the_gate_passes_a_good_section_and_fails_each_planted_fault() {
        let compiled = Compiled {
            version: "2026f".into(),
            files: [
                (
                    "Asia/Kathmandu".to_string(),
                    tzif("<+0545>-5:45", 20_700, "+0545"),
                ),
                ("Asia/Kolkata".to_string(), tzif("IST-5:30", 19_800, "IST")),
            ]
            .into_iter()
            .collect(),
        };
        let zones = [("Asia/Kathmandu", 4), ("Asia/Kolkata", 9)];
        let run = |zones_written: &[(&str, u32)], tzdb: Option<&str>| {
            let bytes = section(&compiled, zones_written)?;
            let s = Section::unpacked(Kind::TZ_RULES, zones_written.len() as u64, &bytes);
            let r = TzRules::open(&s).map_err(|e| e.to_string())?;
            check(&r, tzdb, &zones, &compiled)
        };
        assert_eq!(run(&zones, Some("2026f")), Ok(()));
        assert!(run(&zones[..1], Some("2026f")).is_err(), "a zone dropped");
        assert!(run(&zones, Some("2026e")).is_err(), "a wrong version");
        assert!(run(&zones, None).is_err(), "no version");
        assert!(
            run(&[("Asia/Kathmandu", 9), ("Asia/Kolkata", 4)], Some("2026f")).is_err(),
            "areas swapped"
        );
        assert!(
            section(&compiled, &[("Asia/Dhaka", 1)]).is_err(),
            "a zone never compiled"
        );
        let mut changed = Compiled {
            version: compiled.version.clone(),
            files: compiled.files.clone(),
        };
        if let Some(f) = changed.files.get_mut("Asia/Kolkata") {
            *f = tzif("IST-5:31", 19_860, "IST");
        }
        let bytes = section(&compiled, &zones).unwrap_or_default();
        let s = Section::unpacked(Kind::TZ_RULES, 2, &bytes);
        let r = TzRules::open(&s).ok();
        assert!(
            r.is_some_and(|r| check(&r, Some("2026f"), &zones, &changed).is_err()),
            "rules that differ from zic's"
        );
    }

    /// The real compile from the pinned tzdata and tzcode 2026f: run with
    /// `cargo test -- --ignored zic_from` once `forge fetch` has stored both.
    #[test]
    #[ignore = "needs tzdb-2026f and tzcode-2026f in the store, and a C compiler"]
    fn zic_from_the_pinned_tzcode_compiles_the_outline_zones() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let lock = crate::lock::Lock::read(&root.join("data/sources.lock"))
            .unwrap_or_else(|e| panic!("{e}"));
        let stored = ["tzdb-2026f", "tzcode-2026f"]
            .iter()
            .all(|id| lock.path(&root, id).is_ok_and(|p| p.exists()));
        assert!(stored, "fetch tzdb-2026f and tzcode-2026f first");
        let place = |key: &str| super::super::Place {
            key: key.into(),
            kind: teigeodata_core::PlaceKind::TimezoneArea,
            admin_level: 0,
            flags: 0,
            point: (0, 0),
            names: Vec::new(),
            population: None,
            rank: 0,
            parent: None,
            sources: Vec::new(),
        };
        let zones = super::Zones {
            places: vec![place("tz:Asia/Kathmandu"), place("tz:America/New_York")],
            rings: Vec::new(),
        };
        let c = super::compile(&root, &lock, Some("tbb-2026f"), &zones);
        let c = c.ok().flatten();
        assert!(c.as_ref().is_some_and(|c| c.version == "2026f"));
        let ktm = c.as_ref().and_then(|c| c.files.get("Asia/Kathmandu"));
        assert_eq!(ktm.map(Vec::len), Some(161), "the slim file of the note");
        assert!(
            !root.join("data/build/tz-rules/2026f").exists(),
            "scratch removed"
        );
        let bad = super::Zones {
            places: vec![place("tz:Asia/Nowhere")],
            rings: Vec::new(),
        };
        let e = super::compile(&root, &lock, Some("tbb-2026f"), &bad);
        assert!(e.is_err_and(|e| e.contains("G-tzrules")));
    }
}
