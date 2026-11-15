//! `cargo xtask phone <layer.tgd> [--record]`: runs the phone-class bench (P1, TG-17, the
//! engine's `phone` example) on a layer and, with `--record`, writes its table for the site's
//! measured page. The bench is the gate (G-phone); this only runs it and keeps what it printed.

use std::path::Path;
use std::process::Command;

use crate::measured::Record;

const USAGE: &str = "usage: cargo xtask phone <layer.tgd> [--factor <x>] [--record]";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut layer, mut factor, mut record) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--record" => record = true,
            "--factor" => factor = Some(it.next().ok_or_else(|| vec![USAGE.to_owned()])?.clone()),
            p if !p.starts_with("--") && layer.is_none() => layer = Some(p.to_owned()),
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let layer = layer.ok_or_else(|| vec![USAGE.to_owned()])?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root).args([
        "run",
        "--quiet",
        "--locked",
        "--release",
        "-p",
        "teigeodata-engine",
        "--all-features",
        "--example",
        "phone",
        "--",
        &layer,
    ]);
    if let Some(f) = &factor {
        cmd.args(["--factor", f]);
    }
    let out = cmd.output().map_err(|e| vec![format!("cargo run: {e}")])?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(vec![format!(
            "the phone bench failed:\n{text}{}",
            String::from_utf8_lossy(&out.stderr)
        )]);
    }
    if record {
        // The bench's own table; the intro carries its factor.
        let table: String = text
            .lines()
            .filter(|l| l.starts_with('|'))
            .collect::<Vec<_>>()
            .join("\n");
        let name = Path::new(&layer)
            .file_name()
            .map_or_else(|| layer.clone(), |n| n.to_string_lossy().into_owned());
        let scale = text
            .split_once(", factor ")
            .and_then(|(_, r)| r.split_whitespace().next())
            .unwrap_or("?");
        let intro = format!(
            "The phone-class bench (P1, G-phone) on `{name}`: open, the first query of each kind, \
             search a keystroke at a time and the point queries, each the minimum of several \
             rounds here and multiplied by {scale} (this machine's core over a low-end \
             phone's), and the heap counted by allocator; the gate fails when an estimate is \
             over its target. A point query is `reverse_in_country` (10 nearest), `admin_at`, \
             `tz_at` and `elevation_at`; the page cache is warm, as on a relaunch."
        );
        let written = Record::new(
            root,
            50,
            "Phone-class estimates",
            &intro,
            &format!("cargo xtask phone {name} --record"),
            &table,
        )
        .write(root, "phone")?;
        return Ok(format!("{text}\nrecorded in {written}"));
    }
    Ok(text)
}
