//! `cargo xtask fuzz-coverage <dir>`: the fuzzing campaign's coverage gate (phase 8.1, D-331).
//!
//! `fuzz-campaign.yml` runs each libFuzzer target for hours, replays its minimised corpus under
//! `cargo fuzz coverage` and writes `llvm-cov export -summary-only` JSON as `<target>.json`.
//! This task reads every such file in `<dir>` and checks that **every section kind's decoder
//! was reached** by at least one target: its module has covered lines. It prints a Markdown
//! report (one row per kind with the best target's line and function coverage, then each
//! target's totals) for the job summary and the phase file.
//!
//! Coverage is per source file, so kinds that share a layout share a row's figures: `SEARCH`
//! and `SUBSTRING` (`search.rs`), `REGION` and `ADMIN_REGION` (`region.rs`), `ARCS` and
//! `ARCS_OVERVIEW` (`arcs.rs`), `DEM` and `DEM_COARSE` (`dem.rs`). The best single target is reported, not a union
//! across targets: a union needs every binary's profile, and one target reaching a decoder is
//! what the gate asks.
//!
//! The kinds come from the reader itself (`Kind::name`), so a kind added to the format without
//! a decoder entry here fails `every_known_kind_has_a_decoder`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::Value;

/// Where each kind's decoder lives, relative to `crates/teigeodata-format/src/`.
const DECODERS: &[(&str, &str)] = &[
    ("IDMAP", "idmap.rs"),
    ("PLACES", "places.rs"),
    ("HIERARCHY", "hierarchy.rs"),
    ("NAMES", "names.rs"),
    ("SEARCH", "search.rs"),
    ("PLACE_INDEX", "place_index.rs"),
    ("ARCS", "arcs.rs"),
    ("POLYGONS", "polygons.rs"),
    ("POLY_INDEX", "poly_index.rs"),
    ("REGION", "region.rs"),
    ("POSTAL", "postal.rs"),
    ("COMPONENTS", "components.rs"),
    ("DEM", "dem.rs"),
    ("BLOBS", "blobs.rs"),
    ("BLOCK_HASHES", "blocks.rs"),
    ("REDIRECTS", "redirects.rs"),
    ("ADMIN_REGION", "region.rs"),
    ("ARCS_OVERVIEW", "arcs.rs"),
    ("ELEVATION", "elevation.rs"),
    ("PLACE_TZ", "place_tz.rs"),
    ("ISO_CODES", "iso_codes.rs"),
    ("TZ_RULES", "tz_rules.rs"),
    ("SUBSTRING", "search.rs"),
    ("DEM_COARSE", "dem.rs"),
];

const FORMAT_SRC: &str = "crates/teigeodata-format/src/";

/// Covered and total counts of one file in one target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    lines: (u64, u64),
    functions: (u64, u64),
}

/// One target's summary: per file (path from the workspace root) and the totals.
#[derive(Debug, Default)]
struct Summary {
    files: BTreeMap<String, Counts>,
    totals: Counts,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let [dir] = args else {
        return Err(vec![
            "usage: cargo xtask fuzz-coverage <dir of <target>.json>".into(),
        ]);
    };
    let dir = root.join(dir);
    let mut summaries = BTreeMap::new();
    let entries = fs::read_dir(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let target = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = fs::read_to_string(&path).map_err(|e| vec![format!("{target}: {e}")])?;
            let summary = parse(&text).map_err(|e| vec![format!("{target}: {e}")])?;
            summaries.insert(target, summary);
        }
    }
    let missing: Vec<String> = DECODERS
        .iter()
        .map(|(_, file)| *file)
        .filter(|file| !root.join(FORMAT_SRC).join(file).is_file())
        .map(|file| format!("decoder {FORMAT_SRC}{file} does not exist; update DECODERS"))
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    gate(&summaries)
}

/// Reads `llvm-cov export -summary-only` JSON. Paths are cut to the workspace root (the part
/// from `crates/`), so runner checkouts and local ones compare equal.
fn parse(text: &str) -> Result<Summary, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let data = json
        .pointer("/data/0")
        .ok_or("no `data[0]`: not an llvm-cov export")?;
    let n = |v: &Value, at: &str| v.pointer(at).and_then(Value::as_u64).unwrap_or(0);
    let counts = |v: Option<&Value>| {
        v.map_or_else(Counts::default, |v| Counts {
            lines: (n(v, "/lines/covered"), n(v, "/lines/count")),
            functions: (n(v, "/functions/covered"), n(v, "/functions/count")),
        })
    };
    let mut summary = Summary {
        totals: counts(data.get("totals")),
        ..Summary::default()
    };
    let files = data
        .get("files")
        .and_then(Value::as_array)
        .ok_or("no `files`")?;
    for file in files {
        let name = file
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(at) = name.find("crates/") {
            let path = name.get(at..).unwrap_or_default().to_owned();
            summary.files.insert(path, counts(file.get("summary")));
        }
    }
    Ok(summary)
}

fn percent((covered, count): (u64, u64)) -> String {
    if count == 0 {
        return "–".into();
    }
    #[expect(clippy::cast_precision_loss, reason = "a percentage for a report")]
    let p = covered as f64 * 100.0 / count as f64;
    format!("{p:.1} %")
}

/// The report, or the kinds no target reached.
fn gate(summaries: &BTreeMap<String, Summary>) -> Result<String, Vec<String>> {
    if summaries.is_empty() {
        return Err(vec![
            "no coverage summaries: did every campaign job fail?".into(),
        ]);
    }
    let mut out = String::from(
        "### Fuzzing campaign: section decoders reached\n\n\
         | Kind | Decoder | Lines | Functions | Best target |\n|---|---|---|---|---|\n",
    );
    let mut problems = Vec::new();
    for (kind, file) in DECODERS {
        let path = format!("{FORMAT_SRC}{file}");
        let best = summaries
            .iter()
            .filter_map(|(target, s)| s.files.get(&path).map(|c| (target, *c)))
            .max_by_key(|(_, c)| c.lines.0);
        match best {
            Some((target, c)) if c.lines.0 > 0 => {
                let _ = writeln!(
                    out,
                    "| {kind} | `{file}` | {} ({}/{}) | {} | {target} |",
                    percent(c.lines),
                    c.lines.0,
                    c.lines.1,
                    percent(c.functions),
                );
            }
            _ => {
                let _ = writeln!(out, "| {kind} | `{file}` | **not reached** | – | – |");
                problems.push(format!("no fuzz target reached {kind}'s decoder ({path})"));
            }
        }
    }
    out.push_str("\n| Target | Lines | Functions |\n|---|---|---|\n");
    for (target, s) in summaries {
        let _ = writeln!(
            out,
            "| {target} | {} | {} |",
            percent(s.totals.lines),
            percent(s.totals.functions),
        );
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        eprintln!("{out}");
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teigeodata_format::Kind;

    /// Every kind this reader knows, by name.
    fn known_kinds() -> Vec<&'static str> {
        (0..=u8::MAX)
            .filter_map(|k| Kind(u32::from(k)).name())
            .collect()
    }

    fn export(files: &[(&str, u64, u64)]) -> String {
        let files: Vec<Value> = files
            .iter()
            .map(|(name, covered, count)| {
                serde_json::json!({
                    "filename": format!("/home/runner/work/geodata/geodata/{name}"),
                    "summary": {
                        "lines": {"covered": covered, "count": count},
                        "functions": {"covered": u64::from(*covered > 0), "count": 1},
                    }
                })
            })
            .collect();
        serde_json::json!({
            "data": [{"files": files, "totals": {"lines": {"covered": 1, "count": 2},
                "functions": {"covered": 1, "count": 2}}}],
            "type": "llvm.coverage.json.export",
        })
        .to_string()
    }

    fn all_reached() -> Vec<(String, u64, u64)> {
        DECODERS
            .iter()
            .map(|(_, f)| (format!("{FORMAT_SRC}{f}"), 10, 20))
            .collect()
    }

    fn summaries(files: &[(String, u64, u64)]) -> BTreeMap<String, Summary> {
        let files: Vec<(&str, u64, u64)> =
            files.iter().map(|(n, c, t)| (n.as_str(), *c, *t)).collect();
        BTreeMap::from([("engine".to_owned(), parse(&export(&files)).unwrap())])
    }

    #[test]
    fn every_known_kind_has_a_decoder() {
        let mapped: Vec<&str> = DECODERS.iter().map(|(k, _)| *k).collect();
        for kind in known_kinds() {
            assert!(mapped.contains(&kind), "{kind} has no decoder in DECODERS");
        }
        assert_eq!(
            mapped.len(),
            known_kinds().len(),
            "DECODERS names a kind the reader lacks"
        );
    }

    #[test]
    fn every_decoder_exists() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for (kind, file) in DECODERS {
            assert!(root.join(FORMAT_SRC).join(file).is_file(), "{kind}: {file}");
        }
    }

    #[test]
    fn passes_when_every_decoder_is_reached() {
        let report = gate(&summaries(&all_reached())).unwrap();
        assert!(report.contains("| DEM_COARSE | `dem.rs` | 50.0 % (10/20) | 100.0 % | engine |"));
        assert!(report.contains("| engine | 50.0 % | 50.0 % |"));
    }

    /// The planted fault: one decoder with no covered line fails the gate and is named.
    #[test]
    fn fails_on_a_decoder_no_target_reached() {
        let mut files = all_reached();
        let blobs = files
            .iter_mut()
            .find(|f| f.0.ends_with("blobs.rs"))
            .unwrap();
        blobs.1 = 0;
        let problems = gate(&summaries(&files)).unwrap_err();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("BLOBS"));

        let missing: Vec<_> = all_reached()
            .into_iter()
            .filter(|f| !f.0.ends_with("tz_rules.rs"))
            .collect();
        let problems = gate(&summaries(&missing)).unwrap_err();
        assert!(problems[0].contains("TZ_RULES"), "{problems:?}");
    }

    #[test]
    fn the_best_target_counts() {
        let mut low = all_reached();
        for f in &mut low {
            f.1 = 0;
        }
        let mut s = summaries(&low);
        s.insert(
            "open".into(),
            parse(&export(&[(&format!("{FORMAT_SRC}blobs.rs"), 3, 9)])).unwrap(),
        );
        let problems = gate(&s).unwrap_err();
        assert!(
            !problems.iter().any(|p| p.contains("BLOBS")),
            "{problems:?}"
        );
    }

    #[test]
    fn refuses_no_summaries_and_bad_json() {
        assert!(gate(&BTreeMap::new()).is_err());
        assert!(parse("{}").is_err());
        assert!(parse("not json").is_err());
    }
}
