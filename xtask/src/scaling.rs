//! `cargo xtask scaling <pack.tgd> [--secs <n>] [--record]`: phase 8.3's scaling gate. Runs the
//! engine's `scaling` example (one `Shared` engine, a mix of `get`, `reverse`, `admin_at`,
//! `tz_at` and `search`, at 1, 2, 4 and 8 threads) and fails when four threads do not reach
//! [`FLOOR_AT_4`] times one thread's throughput. With `--record` it writes the table for the
//! site's measured page ([research](../../docs/02-research/notes/profiling.md), D-338).
//!
//! The floor is low on purpose: it is not a promise of linear scaling on a machine shared with
//! other work and with efficiency cores, but a tripwire for a lock or shared write on the query
//! path, which holds every thread count near one thread's throughput.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use crate::measured::Record;

const USAGE: &str = "usage: cargo xtask scaling <pack.tgd> [--secs <n>] [--record]";
const FEATURES: &str = "shared,elevation,admin,names,search,timezone,timezone-index,admin-index";
/// Four threads must reach at least this speed-up over one.
const FLOOR_AT_4: f64 = 1.5;

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut pack, mut secs, mut record) = (None, "6".to_owned(), false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--record" => record = true,
            "--secs" => it
                .next()
                .ok_or_else(|| vec![USAGE.to_owned()])?
                .clone_into(&mut secs),
            p if !p.starts_with("--") && pack.is_none() => pack = Some(p.to_owned()),
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let pack = pack.ok_or_else(|| vec![USAGE.to_owned()])?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .args([
            "run",
            "--quiet",
            "--locked",
            "--release",
            "-p",
            "teigeodata-engine",
            "--features",
            FEATURES,
            "--example",
            "scaling",
            "--",
            &pack,
            &secs,
        ])
        .output()
        .map_err(|e| vec![format!("cargo run: {e}")])?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(vec![format!(
            "the scaling example failed:\n{text}{}",
            String::from_utf8_lossy(&out.stderr)
        )]);
    }
    let at4 = speedup(&text, 4).ok_or_else(|| vec![format!("no row for 4 threads in:\n{text}")])?;
    if at4 < FLOOR_AT_4 {
        return Err(vec![format!(
            "{text}4 threads reach {at4:.2}× one thread, under the floor of {FLOOR_AT_4}×: \
             a lock or shared write on the query path?"
        )]);
    }
    let mut report = format!("{text}4 threads: {at4:.2}× (floor {FLOOR_AT_4}×)\n");
    if record {
        let name = Path::new(&pack)
            .file_name()
            .map_or_else(|| pack.clone(), |n| n.to_string_lossy().into_owned());
        let table: String = text
            .lines()
            .filter(|l| l.starts_with('|'))
            .collect::<Vec<_>>()
            .join("\n");
        let intro = format!(
            "Throughput of one shared engine on `{name}`: each thread takes the current \
             snapshot for every query, as a server's handlers do, and runs `get`, `reverse` \
             (10 nearest), `admin_at`, `tz_at` and `search` (10 hits) in turn. The best of three \
             rounds per thread count. The gate fails when four threads reach less than \
             {FLOOR_AT_4}× one thread, the mark of a lock on the query path; the machine is \
             shared and mixes performance and efficiency cores, so the figures are a floor."
        );
        let written = Record::new(
            root,
            35,
            "Scaling across threads",
            &intro,
            &format!("cargo xtask scaling {name} --record"),
            &table,
        )
        .write(root, "scaling")?;
        let _ = writeln!(report, "recorded in {written}");
    }
    Ok(report)
}

/// The speed-up column of the row for `threads` threads.
fn speedup(table: &str, threads: u32) -> Option<f64> {
    table.lines().find_map(|l| {
        let cells: Vec<&str> = l.split('|').map(str::trim).collect();
        if cells.get(1)?.parse::<u32>().ok()? != threads {
            return None;
        }
        cells.get(3)?.trim_end_matches('×').parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::speedup;

    #[test]
    fn reads_the_speedup_of_a_row() {
        let t = "| Threads | Queries/s | Speed-up | Efficiency |\n|---:|---:|---:|---:|\n\
                 | 1 | 100 | 1.00× | 100% |\n| 4 | 260 | 2.60× | 65% |\n";
        assert_eq!(speedup(t, 4), Some(2.6));
        assert_eq!(speedup(t, 1), Some(1.0));
        assert_eq!(speedup(t, 8), None);
    }
}
