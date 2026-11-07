//! `forge`: the teigeodata data pipeline (ADR-0035). Stages land here as their spikes and
//! phases need them; each stage reads pinned inputs and writes content-addressed outputs.
//!
//! ```text
//! forge fetch [--pin] [--verify] [<source-id>…]   download and verify pinned sources
//! forge read <reader> <source-id> [<source-id>…]  turn fetched bytes into record tables
//! forge pip <boundary-source> <level> <points>    assign read points to admin units
//! forge geofix <points> [<boundary-source>]      flag rounded, stacked, snapped points
//! forge links <points> <points> [...]            stated same-as pairs and their distances
//! forge conflate <cc> <points> <points> [...]     match records across sources
//! forge aliases <cc> <anchor> <a+b+...>          carry cluster names to the anchor's records
//! forge dem <tileset> <points> [...]             DEM heights against stated elevations
//! forge dem-ref <dem> <ref> <geoid> [<boundary>]  the DEM against a ground reference
//! forge geoid <source> <lat> <lon> [...]          geoid undulation N at points
//! forge build <cc> --release <v> --admin <b> …    write and check a country's pack
//! forge make <cc> --release <v> [--lite] …       every stage from data/countries/<cc>/build.yaml, cached
//! forge path <source-id>                          print a fetched source's store path
//! ```
//!
//! Run it from anywhere inside the repository: `cargo run --release --manifest-path
//! forge/Cargo.toml -- fetch`.
#![allow(
    clippy::print_stdout,
    reason = "a command-line tool: `path` prints its answer to stdout"
)]

mod aliases;
mod build;
mod conflate;
mod dem;
mod fetch;
mod geofix;
mod geoid;
mod geom;
mod links;
mod lock;
mod make;
mod pip;
mod read;
mod record;
mod sliderule;
mod tiles;

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
        Some("path") => match args.get(1) {
            Some(id) => path(&root, id),
            None => Err("usage: forge path <source-id>".to_string()),
        },
        _ => match stage(&root, &args) {
            Some(r) => r.map(|summary| eprintln_safe(&summary)),
            None => Err(
                "usage: forge fetch [--pin] [--verify] [<source-id>…] | forge path <source-id> | forge <stage> … (see the module docs)"
                    .to_string(),
            ),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln_safe(&format!("forge: {e}"));
            ExitCode::FAILURE
        }
    }
}

/// The stages: each returns its summary. `None` when `args` names no stage.
fn stage(root: &Path, args: &[String]) -> Option<Result<String, String>> {
    Some(match args.first().map(String::as_str)? {
        "build" => build::run(root, args.get(1..).unwrap_or_default()),
        "make" => make::run(root, args.get(1..).unwrap_or_default()),
        "read" => match (args.get(1), args.get(2..)) {
            (Some(reader), Some(ids)) if !ids.is_empty() => read::run(root, reader, ids),
            _ => Err("usage: forge read <geonames|osm|wikidata> <source-id> [<source-id>…]".into()),
        },
        "pip" => match (
            args.get(1),
            args.get(2).map(|l| l.parse::<u8>()),
            args.get(3),
        ) {
            (Some(b), Some(Ok(level)), Some(p)) => pip::run(root, b, level, p),
            _ => Err("usage: forge pip <boundary-source> <level> <points-source>".into()),
        },
        "geofix" => match args.get(1) {
            Some(p) => geofix::run(root, p, args.get(2).map(String::as_str)),
            None => Err("usage: forge geofix <points-source> [<boundary-source>]".into()),
        },
        "links" if args.len() >= 3 => links::run(root, args.get(1..).unwrap_or_default()),
        "links" => Err("usage: forge links <points-source> <points-source> [...]".into()),
        "conflate" if args.len() >= 4 => match (args.get(1), args.get(2..)) {
            (Some(cc), Some(sources)) => conflate::run(root, cc, sources),
            _ => Err("usage: forge conflate <cc> <points-source> <points-source> [...]".into()),
        },
        "conflate" => {
            Err("usage: forge conflate <cc> <points-source> <points-source> [...]".into())
        }
        "aliases" => match (args.get(1), args.get(2), args.get(3)) {
            (Some(cc), Some(anchor), Some(run)) => aliases::run(root, cc, anchor, run),
            _ => Err("usage: forge aliases <cc> <anchor-source> <source+source+...>".into()),
        },
        "dem" if args.len() >= 3 => match (args.get(1), args.get(2..)) {
            (Some(t), Some(sources)) => dem::run(root, t, sources),
            _ => Err("usage: forge dem <tileset-source> <points-source> [...]".into()),
        },
        "dem-ref" => match (args.get(1), args.get(2), args.get(3)) {
            (Some(d), Some(r), Some(g)) => {
                dem::reference::run(root, d, r, g, args.get(4).map(String::as_str))
            }
            _ => {
                Err("usage: forge dem-ref <dem-tileset> <reference-tileset> <geoid-source> [<boundary-source>]".into())
            }
        },
        "geoid" if args.len() >= 4 => geoid::run(root, args.get(1..).unwrap_or_default()),
        "dem" => Err("usage: forge dem <tileset-source> <points-source> [...]".into()),
        _ => return None,
    })
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
