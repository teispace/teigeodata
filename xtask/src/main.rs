//! `cargo xtask`: every repository task and gate of teigeodata (ADR-0021).
//!
//! The tasks are a binary for people and CI, so printing is their interface.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports on stdout and stderr"
)]

mod abi;
mod accuracy;
mod bindings;
mod breakdown;
mod clean;
mod commit;
mod corrections;
mod data;
mod data_pages;
mod docs;
mod docs_examples;
mod flutter_check;
mod fuzz_coverage;
mod golden;
mod ids;
mod iso3166;
mod lexicon;
mod load;
mod maven;
mod measured;
mod mirror;
mod pack;
mod package_c;
mod package_dotnet;
mod package_jvm;
mod package_node;
mod package_swift;
mod package_wasm;
mod parity;
mod phone;
mod playground;
mod postal;
mod profile;
mod recipe;
mod release;
mod romanise;
mod scaling;
mod sizes;
mod sources;
mod split;
mod tuf;
mod unsafe_audit;
mod version;
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
  flutter-check [--device <id> | --ios]
                    The Dart package in a Flutter app (D-289): makes the platform folders of
                    bindings/dart/example, analyses it and runs its integration test on the
                    host's desktop, the given device, or (--ios) the first iPhone simulator,
                    booted for the run and shut down after it.
  package-c [--target <triple>] [--lite] [--out <dir>] [--smoke] [--prebuilt]
                    The C library as it ships (D-286): builds teigeodata-c (dist profile),
                    stages the header, shared and static libraries, pkg-config and CMake
                    files, checks the exports and archives it under target/package/;
                    --smoke builds and runs bindings/c/parity.c through pkg-config and
                    CMake against the staged package; --prebuilt also writes the library
                    alone as the release asset Dart and Go download (an Android or iOS
                    target builds only that, D-304).
  release-pin <dist> [--check]
                    Pins a release's files (D-304): the libraries' SHA-256 into the Dart
                    hook and the Go loader, the XCFramework's URL and checksum into the
                    root Package.swift; --check only compares.
  package-jvm [--android] [--desktop [--natives <dir>] [--test]] [--android-test]
                    The JVM binding as it ships (D-288): --android builds the JNI library
                    for arm64-v8a, armeabi-v7a and x86_64 (cargo-ndk, API 24), checks 16 KB
                    alignment and makes target/jvm/teigeodata-<v>.aar; --desktop makes the
                    JAR with the host's library and those under --natives, --test runs the
                    parity checks from it; --android-test runs them on a running emulator.
  package-dotnet [--natives <dir>] [--test]
                    The NuGet package Teispace.TeiGeodata (D-298) under target/dotnet/, with
                    each RID's library in runtimes/<rid>/native (<dir>/<rid>/<file>, or this
                    machine's alone); --test runs the parity program from the package under
                    the JIT and as NativeAOT.
  package-node [--addons <dir>] [--test]
                    The Node.js packages (D-294, napi-rs layout) under target/node/: the
                    main package with its loader, and a platform package per prebuilt
                    addon; --addons takes teigeodata.<suffix>.node for every platform,
                    else this machine's addon is built and packaged; --test installs them
                    as npm would and runs the parity checks.
  package-wasm [--variant full|lookup] [--test] [--record]
                    The WebAssembly npm packages (D-293): each variant (a set of
                    components, D-291) built, through wasm-bindgen --target web and
                    wasm-opt -Oz, with the per-host entries, under target/wasm/<variant>/;
                    fails when the gzipped module is over the variant's budget; --test runs
                    the parity checks from the package on Node.js, Bun and Deno; --record
                    writes the sizes for the site's measured page (site/data/measured/).
  package-swift [--test] [--simulator <name>]
                    The XCFramework the Swift package ships (D-287): teigeodata-c's static
                    library for iOS, the iOS simulators and macOS, joined, as
                    bindings/swift/CTeiGeodata.xcframework and a zip with its SwiftPM
                    checksum under target/apple/; --test runs the Swift parity checks
                    against it on macOS and on an iOS simulator.
  parity-mutants [--lang <a,b>] [--leaf <prefix>] [--jobs <n>] [--sample] [--no-build] [--list]
                    Plants each value of bindings/parity.json in turn and requires every
                    language's parity suite (python, node, go, jvm, swift, dart, c, cpp) to
                    fail on it (5.9); reports the values a suite does not check. --sample
                    plants only the first value of each section.
  pack-sizes <pack.tgd> [--langs ne,en]
                    TG-11: where a pack's bytes go: each component on disk and on the
                    wire and per place, each column, names by language (and the names
                    section cut to --langs), outlines by admin level and level of detail.
  check-workspace   Check that the forge, bindings/ and the C ABI crate keep the main lint
                    table (boundary crates may only lower unsafe_code to deny), and that every
                    file and lockfile names the workspace's version (bump --check).
  bump <major.minor.patch> | bump --check
                    Write the release version everywhere it is written by hand (the
                    workspaces, the bindings' packages, .NET, Go, Dart, Node) and refresh
                    every lockfile's workspace entries; --check fails on any that differs.
  mirror [<id>…] [--check]
                    Upload the stored bytes of pinned sources (every source with a
                    mirror_url when none are named) to the content-addressed release
                    `sources`, one asset per file named by its SHA-256 (D-127); needs `gh`.
                    --check uploads nothing and fails on each pinned file not there.
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
  data-pages [--check] [--fetch] [--pack <path>] [--tgd <path>]
                    Phase 7.5: writes the site's coverage page and a page per country folder
                    (site/content/docs/data/) from data/countries/ and the pack site/data.json
                    pins, read with `tgd export` and `tgd inspect` (target/release/tgd);
                    --check fails on any difference.
  docs-examples [--check|--bless] [--lang <a,b>] [--pack <path>] [--fetch]
                    Phase 7: runs each language's tour (site/examples/<lang>/) on the pack
                    site/data.json pins (--fetch downloads it to target/docs-data/ and checks
                    its SHA-256) and compares what it prints with site/examples/tour.out;
                    --check also checks the per-section files the pages include, --bless
                    writes them from one language. Languages not built here are skipped
                    and named.
  fuzz-coverage <dir>
                    The fuzzing campaign's gate (phase 8.1): reads each target's llvm-cov
                    summary (<target>.json) and fails unless every section kind's decoder was
                    reached; prints the Markdown report.
  unsafe-audit [--check]
                    Phase 8.2: lists every unsafe block and impl of the shipped sources with
                    its SAFETY argument and the tests that reach it, in
                    docs/06-engineering/unsafe-inventory.md; fails on a block without an
                    argument or a file not reviewed; --check fails when the page is stale.
  load <pack.tgd>… [--query <name>] [--secs <n>] [--tgd <path>] [--only <route>]… [--check <budget.toml>] [--record]
                    Phase 6.4 (D-320): builds tgd, serves each pack on loopback and drives
                    a route mix (search at 1, 16 and 64 connections, reverse, get, near,
                    hierarchy, boundary, Nominatim, MCP) with oha, twice, sampling the
                    footprint; --check holds it to tools/load/budget.toml's per-pack
                    floors and ceilings (needs oha and curl); --record (every route) writes
                    the tables for the site's measured page.
  accuracy <cc> <pack.tgd> [--registry <tsv>] [--places <tsv>] [--write]
                    Phase 3.7 (D-339): the country's ACCURACY.md, measured on the shipped
                    pack against tests/golden/<cc>-places.csv (horizontal by stratum, the
                    vertical part from position, the two labelling passes' agreement);
                    --write writes data/countries/<cc>/ACCURACY.md.
  profile <pack.tgd>… [--suite <name>]… [--secs <n>]
                    Phase 8.3 (D-338): records each Q and L suite of the engine's profile
                    example with Instruments' Time Profiler (macOS, xctrace) and prints the
                    items with the most self time and the most time under each teigeodata
                    frame; traces stay in target/profile/.
  scaling <pack.tgd> [--secs <n>] [--record]
                    Phase 8.3 (D-338): throughput of one Shared engine at 1, 2, 4 and 8
                    threads on a query mix; fails when 4 threads reach under 1.5× one
                    thread (a lock on the query path); --record writes the table for the
                    site's measured page.
  phone <layer.tgd> [--factor <x>] [--record]
                    P1, G-phone: runs the engine's phone-class bench on a layer (open, first
                    queries, search a keystroke at a time, point queries, heap) scaled to a
                    low-end phone; --record writes its table for the site's measured page.
  playground [--fetch] [--no-build]
                    Phase 7.6: writes what the site's playground loads to
                    site/public/playground/ (git-ignored): the full WebAssembly package built
                    from this commit, the pinned pack (SHA-256 checked; --fetch downloads it)
                    and manifest.json.
  sizes [--native|--wasm] [--record]
                    Z2: the bytes each feature set of core and format adds to a linked
                    library, native and wasm32, raw and gzipped (tools/size-probe); --record
                    writes them for the site's measured page.
  sizes --tgd       The tgd binary with neither, either and both servers (features serve
                    and mcp), stripped, raw and gzipped (phase 6.4, D-320).
  help              Show this message.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let task = args.first().map_or("help", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    match task {
        "abi" => report(abi::run(&root, rest)),
        "accuracy" => report(accuracy::run(&root, rest)),
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
        "release-pin" => report(release::run(&root, rest)),
        "flutter-check" => report(flutter_check::run(&root, rest)),
        "package-jvm" => report(package_jvm::run(&root, rest)),
        "package-dotnet" => report(package_dotnet::run(&root, rest)),
        "package-node" => report(package_node::run(&root, rest)),
        "parity-mutants" => report(parity::run(&root, rest)),
        "package-wasm" => report(package_wasm::run(&root, rest)),
        "package-swift" => report(package_swift::run(&root, rest)),
        "pack-sizes" => report(breakdown::run(rest)),
        "check-workspace" => report(workspace::check(&root)),
        "bump" => report(version::run(&root, rest)),
        "mirror" => report(mirror::run(&root, rest)),
        "ids" => report(ids::run(&root, rest)),
        "tuf" => report(tuf::run(rest)),
        "clean" => report(clean::run(&root, rest.iter().any(|a| a == "--all"))),
        "load" => report(load::run(&root, rest)),
        "profile" => report(profile::run(&root, rest)),
        "scaling" => report(scaling::run(&root, rest)),
        "fuzz-coverage" => report(fuzz_coverage::run(&root, rest)),
        "unsafe-audit" => report(unsafe_audit::run(&root, rest)),
        "phone" => report(phone::run(&root, rest)),
        "playground" => report(playground::run(&root, rest)),
        "docs-examples" => report(docs_examples::run(&root, rest)),
        "data-pages" => report(data_pages::run(&root, rest)),
        "sizes" if rest.iter().any(|a| a == "--tgd") => {
            report(sizes::tgd(&root, &root.join("target/sizes-tgd")))
        }
        "sizes" => {
            let only = |f: &str| rest.iter().any(|a| a == f);
            let (native, wasm) = (!only("--wasm"), !only("--native"));
            report(sizes::run(&root, wasm, native).and_then(|table| {
                if !only("--record") {
                    return Ok(table);
                }
                if !(native && wasm) {
                    return Err(vec!["--record measures both targets".into()]);
                }
                let written = measured::Record::new(
                    &root,
                    30,
                    "Library bytes per feature set",
                    "What each feature set of the runtime crates adds to a linked library \
                     (`tools/size-probe`, a baseline that any host library links subtracted), \
                     natively in the release profile and for wasm32 in the size profile.",
                    "cargo xtask sizes --record",
                    &table,
                )
                .write(&root, "library")?;
                Ok(format!("{table}\nrecorded in {written}"))
            }))
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
