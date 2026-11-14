//! `cargo xtask`: every repository task and gate of teigeodata (ADR-0021).
//!
//! The tasks are a binary for people and CI, so printing is their interface.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports on stdout and stderr"
)]

mod abi;
mod bindings;
mod breakdown;
mod clean;
mod commit;
mod corrections;
mod data;
mod docs;
mod golden;
mod ids;
mod iso3166;
mod lexicon;
mod mirror;
mod pack;
mod package_c;
mod package_swift;
mod postal;
mod recipe;
mod romanise;
mod sizes;
mod sources;
mod split;
mod tuf;
mod wire;
mod workspace;
mod xlit;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
cargo xtask <task>

Tasks:
  abi [--since <tag>] [--release] [--lib <path> [--lite]]
                    The C ABI's gates (D-285): the header's declarations against the last
                    release's (or <tag>'s), naming the lowest next version; --release fails
                    when the workspace's version is lower. --lib checks that a built shared
                    library exports exactly the header's functions (--lite: without the
                    store's).
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
  check-pack <pack.tgd> [--lite] [--dump <dir>]
                    G-lic on a built pack: every section verified, the licence of its
                    build, and in a lite pack only lite-class sources' notices (D-126);
                    each section's size; G-wire: the transport asset's bytes per
                    component add up, and each fits the country's wire_kb (D-135);
                    --dump writes each section's bytes to <dir>.
  package-c [--target <triple>] [--lite] [--out <dir>] [--smoke]
                    The C library as it ships (D-286): builds teigeodata-c (dist profile),
                    stages the header, shared and static libraries, pkg-config and CMake
                    files, checks the exports and archives it under target/package/;
                    --smoke builds and runs bindings/c/parity.c through pkg-config and
                    CMake against the staged package.
  package-swift [--test] [--simulator <name>]
                    The XCFramework the Swift package ships (D-287): teigeodata-c's static
                    library for iOS, the iOS simulators and macOS, joined, as
                    bindings/swift/CTeiGeodata.xcframework and a zip with its SwiftPM
                    checksum under target/apple/; --test runs the Swift parity checks
                    against it on macOS and on an iOS simulator.
  pack-sizes <pack.tgd> [--langs ne,en]
                    TG-11: where a pack's bytes go: each component on disk and on the
                    wire and per place, each column, names by language (and the names
                    section cut to --langs), outlines by admin level and level of detail.
  check-workspace   Check that the forge, bindings/ and the C ABI crate keep the main lint
                    table (boundary crates may only lower unsafe_code to deny).
  mirror [<id>…]    Upload the stored bytes of pinned sources (every source with a
                    mirror_url when none are named) to the content-addressed release
                    `sources`, one asset per file named by its SHA-256 (D-127); needs `gh`.
  ids save <build> <out> | ids restore [<tag>] [--build <build>] [--force]
                    The ID registries kept with the data releases (D-261): save writes
                    the world part's and every pack's into one archive; restore puts a
                    release's back (the newest data release's by default), keeping a
                    local one that differs unless --force; needs `gh`.
  tuf <command>     The catalog repository (D-163): keygen, pubkey, root, sign,
                    release, timestamp, verify; `cargo xtask tuf` lists them.
  clean [--all]     Free disk: incremental caches and scratch of both workspaces (built
                    packs in data/build/ are kept); --all also removes data/build/ and
                    runs `cargo clean` in both. Fetched sources are kept.
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
        "abi" => report(abi::run(&root, rest)),
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
        "check-pack" => report(pack::run(&root, rest)),
        "package-c" => report(package_c::run(&root, rest)),
        "package-swift" => report(package_swift::run(&root, rest)),
        "pack-sizes" => report(breakdown::run(rest)),
        "check-workspace" => report(workspace::check(&root)),
        "mirror" => report(mirror::run(&root, rest)),
        "ids" => report(ids::run(&root, rest)),
        "tuf" => report(tuf::run(rest)),
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
