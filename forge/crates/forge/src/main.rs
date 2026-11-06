//! `forge`: the teigeodata data pipeline (ADR-0035). Stages land here as their spikes and
//! phases need them; each stage reads pinned inputs and writes content-addressed outputs.
//!
//! ```text
//! forge fetch [--pin] [--verify] [<source-id>…]   download and verify pinned sources
//! forge read <reader> <source-id> [<source-id>…]  turn fetched bytes into record tables
//! forge pip <boundary-source> <level> <points>    assign read points to admin units
//! forge path <source-id>                          print a fetched source's store path
//! ```
//!
//! Run it from anywhere inside the repository: `cargo run --release --manifest-path
//! forge/Cargo.toml -- fetch`.
#![allow(
    clippy::print_stdout,
    reason = "a command-line tool: `path` prints its answer to stdout"
)]

mod fetch;
mod geom;
mod lock;
mod pip;
mod read;
mod record;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(root) = repo_root() else {
        eprintln_safe("forge: run inside the teigeodata repository (no data/sources.lock found)");
        return ExitCode::FAILURE;
    };
    let result = match args.first().map(String::as_str) {
        Some("fetch") => {
            let flags: Vec<&String> = args
                .iter()
                .skip(1)
                .filter(|a| a.starts_with("--"))
                .collect();
            if let Some(bad) = flags
                .iter()
                .find(|f| !matches!(f.as_str(), "--pin" | "--verify"))
            {
                Err(format!("unknown flag `{bad}`"))
            } else {
                let ids: Vec<String> = args
                    .iter()
                    .skip(1)
                    .filter(|a| !a.starts_with("--"))
                    .cloned()
                    .collect();
                let pin = flags.iter().any(|f| f.as_str() == "--pin");
                let verify = flags.iter().any(|f| f.as_str() == "--verify");
                fetch::run(&root, &ids, pin, verify)
            }
        }
        Some("read") => match (args.get(1), args.get(2..)) {
            (Some(reader), Some(ids)) if !ids.is_empty() => {
                read::run(&root, reader, ids).map(|summary| eprintln_safe(&summary))
            }
            _ => Err("usage: forge read <geonames|osm|wikidata> <source-id> [<source-id>…]".into()),
        },
        Some("pip") => match (
            args.get(1),
            args.get(2).map(|l| l.parse::<u8>()),
            args.get(3),
        ) {
            (Some(b), Some(Ok(level)), Some(p)) => {
                pip::run(&root, b, level, p).map(|summary| eprintln_safe(&summary))
            }
            _ => Err("usage: forge pip <boundary-source> <level> <points-source>".into()),
        },
        Some("path") => match args.get(1) {
            Some(id) => path(&root, id),
            None => Err("usage: forge path <source-id>".to_string()),
        },
        _ => Err(
            "usage: forge fetch [--pin] [--verify] [<source-id>…] | forge path <source-id>"
                .to_string(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln_safe(&format!("forge: {e}"));
            ExitCode::FAILURE
        }
    }
}

fn path(root: &Path, id: &str) -> Result<(), String> {
    let l = lock::Lock::read(&root.join("data/sources.lock"))?;
    let p = fetch::store_path(root, l.get(id)?);
    if !p.is_file() {
        return Err(format!("{id} is not fetched yet: forge fetch {id}"));
    }
    println!("{}", p.display());
    Ok(())
}

fn repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("data/sources.lock").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn eprintln_safe(msg: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "{msg}");
}
