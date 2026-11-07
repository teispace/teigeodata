//! `cargo xtask`: every repository task and gate of teigeodata (ADR-0021).
//!
//! The tasks are a binary for people and CI, so printing is their interface.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports on stdout and stderr"
)]

mod bindings;
mod clean;
mod commit;
mod data;
mod docs;
mod golden;
mod lexicon;
mod recipe;
mod sizes;
mod sources;
mod workspace;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
cargo xtask <task>

Tasks:
  bindings [--check]
                    Extract the IDL from crates/teigeodata-api into bindings/idl.json and
                    generate every binding layer from it; --check fails on any drift.
  check-docs        Check frontmatter, statuses, dates, links, research-note sections and
                    the ADR index across docs/ and the root Markdown files.
  check-commit <f>  Check a commit message file (Conventional Commits, scopes, DCO
                    sign-off, no AI attribution); run by the commit-msg hook.
  check-data        Check every data/countries/<cc>/ folder: profile.yaml (admin ladder,
                    unit types, source codes), lexicon.yaml (IDs, NFC, scripts, variant
                    collisions, evidence) and golden query files; and data/sources.lock
                    (pins, licence classes, query templates).
  check-workspace   Check that the forge, bindings/ and the C ABI crate keep the main lint
                    table (boundary crates may only lower unsafe_code to deny).
  clean [--all]     Free disk: incremental caches and scratch of both workspaces; --all
                    also runs `cargo clean` in both. Fetched sources are kept.
  sizes [--native|--wasm]
                    Z2: the bytes each feature set of core and format adds to a linked
                    library, native and wasm32, raw and gzipped (tools/size-probe).
  help              Show this message.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let task = args.first().map_or("help", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    match task {
        "bindings" => report(bindings::run(&root, rest.iter().any(|a| a == "--check"))),
        "check-docs" => report(docs::check(&root)),
        "check-commit" => {
            if let Some(file) = rest.first() {
                report(commit::check_file(&root, Path::new(file)))
            } else {
                eprintln!("usage: cargo xtask check-commit <message-file>");
                ExitCode::FAILURE
            }
        }
        "check-data" => report(data::check(&root)),
        "check-workspace" => report(workspace::check(&root)),
        "clean" => report(clean::run(&root, rest.iter().any(|a| a == "--all"))),
        "sizes" => {
            let only = |f: &str| rest.iter().any(|a| a == f);
            let (native, wasm) = (!only("--wasm"), !only("--native"));
            report(sizes::run(&root, wasm, native))
        }
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown task `{other}`\n\n{HELP}");
            ExitCode::FAILURE
        }
    }
}

/// Prints a task's problems and turns them into the exit code.
fn report(result: Result<String, Vec<String>>) -> ExitCode {
    match result {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for problem in &problems {
                eprintln!("error: {problem}");
            }
            eprintln!("{} problem(s)", problems.len());
            ExitCode::FAILURE
        }
    }
}

/// The workspace root: the parent of this crate's manifest directory.
fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map_or_else(|| manifest.to_path_buf(), Path::to_path_buf)
}
