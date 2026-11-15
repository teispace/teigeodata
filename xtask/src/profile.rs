//! `cargo xtask profile`: phase 8.3's sampling profiles of the Q and L suites
//! ([research](../../docs/02-research/notes/profiling.md), D-338).
//!
//! Builds the engine's `profile` example in release with line tables, records each suite with
//! Instruments' Time Profiler (`xctrace`, part of Xcode on macOS), exports the samples, and
//! prints for each suite the items with the most self time (the leaf frame of each sample)
//! and the most time under the nearest teigeodata frame (where in our code the time goes,
//! whatever library it ends in). Traces stay in `target/profile/`; they name the machine, so
//! they are never committed.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Every suite of the `profile` example, in the order of the benchmarks page.
const SUITES: [&str; 8] = [
    "reverse", "admin", "tz", "search", "get", "radius", "open", "first",
];

/// The features the `profile` example needs.
const FEATURES: &str = "elevation,admin,names,search,timezone,timezone-index,admin-index";

/// How many items each table lists.
const TOP: usize = 8;

struct Options {
    packs: Vec<PathBuf>,
    suites: Vec<String>,
    secs: u32,
}

fn options(args: &[String]) -> Result<Options, Vec<String>> {
    let mut o = Options {
        packs: Vec::new(),
        suites: Vec::new(),
        secs: 8,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--suite" => o.suites.push(
                it.next()
                    .cloned()
                    .ok_or_else(|| vec!["--suite needs a value".to_owned()])?,
            ),
            "--secs" => {
                o.secs = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| vec!["--secs takes whole seconds".to_owned()])?;
            }
            p if !p.starts_with("--") => o.packs.push(p.into()),
            other => return Err(vec![format!("unknown option {other}")]),
        }
    }
    if o.packs.is_empty() {
        return Err(vec![
            "usage: cargo xtask profile <pack.tgd>... [--suite <name>]... [--secs <n>]".into(),
        ]);
    }
    if let Some(bad) = o.suites.iter().find(|s| !SUITES.contains(&s.as_str())) {
        return Err(vec![format!(
            "unknown suite {bad}: one of {}",
            SUITES.join(", ")
        )]);
    }
    if o.suites.is_empty() {
        o.suites = SUITES.iter().map(|s| (*s).to_owned()).collect();
    }
    Ok(o)
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let o = options(args)?;
    if Command::new("xctrace").arg("version").output().is_err() {
        return Err(vec![
            "xctrace not found: profiles need macOS with Xcode's command line tools".into(),
        ]);
    }
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .env("CARGO_PROFILE_RELEASE_DEBUG", "line-tables-only")
        .args([
            "build",
            "--release",
            "-p",
            "teigeodata-engine",
            "--example",
            "profile",
            "--features",
            FEATURES,
        ])
        .status()
        .map_err(|e| vec![format!("cargo: {e}")])?;
    if !status.success() {
        return Err(vec!["building the profile example failed".into()]);
    }
    let exe = root.join("target/release/examples/profile");
    let out = root.join("target/profile");
    std::fs::create_dir_all(&out).map_err(|e| vec![format!("{}: {e}", out.display())])?;
    let mut report = String::new();
    for pack in &o.packs {
        let stem = pack
            .file_stem()
            .map_or_else(|| "pack".into(), |s| s.to_string_lossy().into_owned());
        let _ = writeln!(report, "## {}\n", pack.display());
        for suite in &o.suites {
            let trace = out.join(format!("{stem}-{suite}.trace"));
            let summary = out.join(format!("{stem}-{suite}.txt"));
            record(&exe, pack, suite, o.secs, &trace, &summary)?;
            let stdout = std::fs::read_to_string(&summary).unwrap_or_default();
            let calls = stdout
                .lines()
                .find(|l| l.starts_with(&format!("{suite}:")))
                .unwrap_or("(no summary line)")
                .to_owned();
            let xml = Command::new("xctrace")
                .args(["export", "--input"])
                .arg(&trace)
                .args([
                    "--xpath",
                    r#"/trace-toc/run[@number="1"]/data/table[@schema="time-profile"]"#,
                ])
                .output()
                .map_err(|e| vec![format!("xctrace export: {e}")])?;
            let samples = parse(&String::from_utf8_lossy(&xml.stdout));
            let _ = writeln!(report, "### {suite}\n\n`{calls}`\n");
            report.push_str(&table(&samples));
        }
    }
    Ok(report)
}

/// Records one suite into `trace`, the driver's summary line into `summary`. xctrace now and
/// then stays idle after its target exits, past its own `--time-limit` and deaf to SIGINT
/// (three times in four full runs, 2026-11-15), so each recording is watched: one that
/// outlives its suite by [`GRACE`] is killed and tried once more.
fn record(
    exe: &Path,
    pack: &Path,
    suite: &str,
    secs: u32,
    trace: &Path,
    summary: &Path,
) -> Result<(), Vec<String>> {
    let limit = Duration::from_secs(u64::from(secs)) + GRACE;
    for attempt in 1..=2 {
        if trace.exists() {
            std::fs::remove_dir_all(trace)
                .map_err(|e| vec![format!("{}: {e}", trace.display())])?;
        }
        let mut child = Command::new("xctrace")
            .args(["record", "--template", "Time Profiler", "--no-prompt"])
            .arg("--target-stdout")
            .arg(summary)
            .arg("--output")
            .arg(trace)
            .arg("--launch")
            .arg("--")
            .arg(exe)
            .arg(pack)
            .arg(suite)
            .arg(secs.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| vec![format!("xctrace record: {e}")])?;
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if start.elapsed() > limit => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(250)),
                Err(e) => return Err(vec![format!("xctrace record: {e}")]),
            }
        };
        match status {
            Some(s) if s.success() && trace.exists() => return Ok(()),
            Some(s) => return Err(vec![format!("xctrace record {suite} failed: {s}")]),
            None => {
                eprintln!("profile: xctrace hung recording {suite} (attempt {attempt}); killed");
            }
        }
    }
    Err(vec![format!("xctrace hung twice recording {suite}")])
}

/// How long a recording may outlive its suite (start-up, the probes, saving the trace).
const GRACE: Duration = Duration::from_secs(90);

/// One sample: its weight in nanoseconds and its frames' names, the leaf first.
struct Sample {
    weight: u64,
    frames: Vec<String>,
}

/// The value of attribute `name` in the tag that starts at `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let end = tag.find('>').unwrap_or(tag.len());
    let head = tag.get(..end)?;
    let key = format!(" {name}=\"");
    let at = head.find(&key)? + key.len();
    let rest = head.get(at..)?;
    rest.get(..rest.find('"')?)
}

/// The samples of the export. Instruments writes each frame, backtrace and weight once with
/// an `id` and refers to it after with `ref`, so the parser keeps a table of each.
fn parse(xml: &str) -> Vec<Sample> {
    let mut frames: HashMap<&str, String> = HashMap::new();
    let mut stacks: HashMap<&str, Vec<String>> = HashMap::new();
    let mut weights: HashMap<&str, u64> = HashMap::new();
    let mut out = Vec::new();
    for row in xml.split("<row>").skip(1) {
        let row = row.split("</row>").next().unwrap_or(row);
        // The sample's weight: the first `<weight`.
        let weight = row
            .find("<weight")
            .and_then(|i| row.get(i..))
            .map_or(0, |w| {
                if let Some(r) = attr(w, "ref") {
                    weights.get(r).copied().unwrap_or(0)
                } else {
                    let v = w
                        .split_once('>')
                        .and_then(|(_, b)| b.split_once('<'))
                        .and_then(|(v, _)| v.parse().ok())
                        .unwrap_or(0);
                    if let Some(id) = attr(w, "id") {
                        weights.insert(id, v);
                    }
                    v
                }
            });
        let Some(at) = row.find("<tagged-backtrace") else {
            continue;
        };
        let Some(bt) = row.get(at..) else { continue };
        if let Some(r) = attr(bt, "ref") {
            if let Some(s) = stacks.get(r) {
                out.push(Sample {
                    weight,
                    frames: s.clone(),
                });
            }
            continue;
        }
        let mut names = Vec::new();
        for f in bt.split("<frame").skip(1) {
            let name = if let Some(r) = attr(f, "ref") {
                frames.get(r).cloned()
            } else {
                let n = attr(f, "name").map_or_else(
                    || attr(f, "addr").unwrap_or("?").to_owned(),
                    |n| demangle(&unescape(n)),
                );
                if let Some(id) = attr(f, "id") {
                    frames.insert(id, n.clone());
                }
                Some(n)
            };
            names.extend(name);
        }
        if let Some(id) = attr(bt, "id") {
            stacks.insert(id, names.clone());
        }
        out.push(Sample {
            weight,
            frames: names,
        });
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A Rust symbol without its hash; any other symbol as it is.
fn demangle(s: &str) -> String {
    format!("{:#}", rustc_demangle::demangle(s))
}

/// The frame a sample's time is charged to in our code: the leaf-most teigeodata frame that
/// is not the driver itself.
fn ours(frames: &[String]) -> Option<&str> {
    frames
        .iter()
        .map(String::as_str)
        .find(|f| f.starts_with("teigeodata_") || f.starts_with("<teigeodata_"))
}

/// Nanoseconds as `f64`: sample weights sum to seconds of profile, far below 2^52 ns.
#[allow(clippy::cast_precision_loss, reason = "exact below 2^52 ns, 52 days")]
const fn ns(v: u64) -> f64 {
    v as f64
}

/// The self-time and our-frame tables for one suite, as Markdown.
fn table(samples: &[Sample]) -> String {
    let total: u64 = samples.iter().map(|s| s.weight).sum();
    let mut leaf: HashMap<&str, u64> = HashMap::new();
    let mut under: HashMap<&str, u64> = HashMap::new();
    for s in samples {
        let l = s.frames.first().map_or("?", String::as_str);
        *leaf.entry(l).or_default() += s.weight;
        *under
            .entry(ours(&s.frames).unwrap_or("(outside teigeodata)"))
            .or_default() += s.weight;
    }
    let mut text = String::new();
    let _ = writeln!(
        text,
        "{} samples, {:.2} s.\n",
        samples.len(),
        ns(total) / 1e9
    );
    for (title, map) in [("Self time (leaf frame)", leaf), ("Under our frame", under)] {
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let _ = writeln!(text, "| {title} | % |\n|---|---:|");
        for (name, w) in v.into_iter().take(TOP) {
            let pct = if total == 0 {
                0.0
            } else {
                ns(w) * 100.0 / ns(total)
            };
            let _ = writeln!(text, "| `{}` | {pct:.1} |", name.replace('|', "\\|"));
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<trace-query-result><node><schema/>
<row><sample-time id="1">1</sample-time><weight id="9" fmt="1.00 ms">1000000</weight><sentinel/></row>
<row><weight ref="9"/><tagged-backtrace id="11"><frame id="12" name="_RNvNtNtCs1_4libm4math3cos3cos" addr="0x1"><binary/></frame><frame id="13" name="_RNvMs_NtCs1_17teigeodata_engine6engineNtB4_6Engine7reverse" addr="0x2"/><frame id="14" name="main" addr="0x3"/></tagged-backtrace></row>
<row><weight ref="9"/><tagged-backtrace ref="11"/></row>
<row><weight id="20">2000000</weight><tagged-backtrace id="21"><frame ref="13"/><frame ref="14"/></tagged-backtrace></row>
</node></trace-query-result>"#;

    #[test]
    fn samples_follow_their_refs() {
        let s = parse(XML);
        assert_eq!(s.len(), 3, "the row without a backtrace is skipped");
        assert_eq!(s[0].weight, 1_000_000);
        assert_eq!(
            s[1].frames, s[0].frames,
            "a backtrace ref repeats its frames"
        );
        assert_eq!(s[0].frames[0], "libm::math::cos::cos");
        assert_eq!(s[2].weight, 2_000_000);
        assert_eq!(
            s[2].frames[0], s[0].frames[1],
            "a frame ref repeats its name"
        );
    }

    #[test]
    fn time_is_charged_to_our_frame() {
        let t = table(&parse(XML));
        assert!(t.contains("| `libm::math::cos::cos` | 50.0 |"), "{t}");
        assert!(
            t.contains("| `<teigeodata_engine::engine::Engine>::reverse` | 100.0 |"),
            "{t}"
        );
    }
}
