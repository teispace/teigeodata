//! `cargo xtask release-pin <dist>`: pins a release's prebuilt files into the packages that
//! download them (phase 5.10, D-304). Run by the `release` workflow on the files it built:
//!
//! - `bindings/dart/lib/src/hook/prebuilt_hashes.dart`: the SHA-256 of each target's library, which
//!   the Dart hook checks before it uses a downloaded one (D-289);
//! - `bindings/go/internal/prebuilt/prebuilt.go`: the same map for the Go loader (D-297);
//! - `Package.swift` at the root: the Swift package as released, its `binaryTarget` naming the
//!   `XCFramework`'s URL and `SwiftPM` checksum (D-287).
//!
//! Every target the Dart hook or the Go loader can ask for must be in `<dist>`, so a release
//! never ships a package that pins fewer libraries than it was built for. `--check` compares
//! instead of writing (the pinned files match `<dist>`).

use std::fmt::Write as _;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Every target with a prebuilt library: the Dart hook's (`Target.of`) and the Go loader's
/// (`prebuilt.Triple`) together.
pub(crate) const PREBUILT_TARGETS: [&str; 14] = [
    "aarch64-apple-darwin",
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
    "aarch64-linux-android",
    "aarch64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "armv7-linux-androideabi",
    "x86_64-apple-darwin",
    "x86_64-apple-ios",
    "x86_64-linux-android",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
];

const DART: &str = "bindings/dart/lib/src/hook/prebuilt_hashes.dart";
const GO: &str = "bindings/go/internal/prebuilt/prebuilt.go";
const SWIFT: &str = "Package.swift";
const RELEASES: &str = "https://github.com/teispace/teigeodata/releases/download";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let check = args.iter().any(|a| a == "--check");
    let Some(dist) = args.iter().find(|a| !a.starts_with("--")) else {
        return Err(vec![
            "usage: cargo xtask release-pin <dist> [--check]".into(),
        ]);
    };
    let dist = Path::new(dist);
    let version = crate::package_c::version(root)?;
    let mut hashes = Vec::with_capacity(PREBUILT_TARGETS.len());
    let mut missing = Vec::new();
    for triple in PREBUILT_TARGETS {
        let name = crate::package_c::prebuilt_name(&version, triple);
        match std::fs::read(dist.join(&name)) {
            Ok(bytes) => hashes.push((triple, hex(&Sha256::digest(&bytes)))),
            Err(_) => missing.push(format!("{}: no {name}", dist.display())),
        }
    }
    let checksum_file = dist.join("CTeiGeodata.xcframework.zip.checksum");
    let checksum = std::fs::read_to_string(&checksum_file)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    if checksum.len() != 64 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        missing.push(format!(
            "{}: not a SwiftPM checksum",
            checksum_file.display()
        ));
    }
    if !missing.is_empty() {
        return Err(missing);
    }

    let go_path = root.join(GO);
    let go_now = std::fs::read_to_string(&go_path).map_err(|e| vec![format!("{GO}: {e}")])?;
    let files = [
        (DART, dart(&hashes)),
        (GO, go(&go_now, &hashes)?),
        (SWIFT, swift(&version, &checksum)),
    ];
    let mut report = Vec::new();
    let mut stale = Vec::new();
    for (path, text) in files {
        let at = root.join(path);
        let now = std::fs::read_to_string(&at).unwrap_or_default();
        if check {
            if now != text {
                stale.push(format!("{path} does not pin {}", dist.display()));
            }
        } else if now != text {
            std::fs::write(&at, text).map_err(|e| vec![format!("{path}: {e}")])?;
            report.push(format!("wrote {path}"));
        }
    }
    if !stale.is_empty() {
        return Err(stale);
    }
    report.push(format!(
        "{} libraries and the XCFramework of {version} pinned",
        hashes.len()
    ));
    Ok(report.join("\n"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn dart(hashes: &[(&str, String)]) -> String {
    let mut s = String::from(
        "// The prebuilt libraries of this package's release (its pubspec version), by Rust target\n\
         // triple. Written by the release pipeline (`cargo xtask release-pin`) from the files it\n\
         // uploads; empty in a checkout between releases, where the hook builds from source.\n\
         \n\
         /// SHA-256 of each target's library file, lowercase hex.\n\
         const prebuiltSha256 = <String, String>{\n",
    );
    for (triple, hash) in hashes {
        let _ = writeln!(s, "  '{triple}': '{hash}',");
    }
    s.push_str("};\n");
    s
}

/// `prebuilt.go` with its `SHA256` map replaced, keys aligned as `gofmt` aligns them.
fn go(now: &str, hashes: &[(&str, String)]) -> Result<String, Vec<String>> {
    let start = now
        .find("var SHA256 = map[string]string{")
        .ok_or_else(|| vec![format!("{GO}: no `var SHA256 = map[string]string{{`")])?;
    let end = now[start..]
        .find("}\n")
        .map(|i| start + i + 2)
        .ok_or_else(|| vec![format!("{GO}: the SHA256 map does not end")])?;
    let width = hashes.iter().map(|(t, _)| t.len()).max().unwrap_or(0);
    let mut map = String::from("var SHA256 = map[string]string{\n");
    for (triple, hash) in hashes {
        let pad = " ".repeat(width - triple.len());
        let _ = writeln!(map, "\t\"{triple}\": {pad}\"{hash}\",");
    }
    map.push_str("}\n");
    Ok(format!("{}{map}{}", &now[..start], &now[end..]))
}

fn swift(version: &str, checksum: &str) -> String {
    format!(
        "// swift-tools-version:6.0\n\
         // The Swift binding of teigeodata as released (D-287, D-304): its C library is the\n\
         // XCFramework of the release this commit is tagged with. Written by the release pipeline\n\
         // (`cargo xtask release-pin`); to work on the binding, use bindings/swift/Package.swift.\n\
         import PackageDescription\n\
         \n\
         let package = Package(\n\
         \x20   name: \"TeiGeodata\",\n\
         \x20   platforms: [.iOS(.v15), .macOS(.v12)],\n\
         \x20   products: [.library(name: \"TeiGeodata\", targets: [\"TeiGeodata\"])],\n\
         \x20   targets: [\n\
         \x20       .binaryTarget(\n\
         \x20           name: \"CTeiGeodata\",\n\
         \x20           url: \"{RELEASES}/v{version}/CTeiGeodata.xcframework.zip\",\n\
         \x20           checksum: \"{checksum}\"\n\
         \x20       ),\n\
         \x20       .target(\n\
         \x20           name: \"TeiGeodata\",\n\
         \x20           dependencies: [\"CTeiGeodata\"],\n\
         \x20           path: \"bindings/swift/Sources/TeiGeodata\",\n\
         \x20           resources: [.copy(\"PrivacyInfo.xcprivacy\")]\n\
         \x20       ),\n\
         \x20   ]\n\
         )\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_go_map_is_replaced_whole_and_aligned() {
        let before = "package prebuilt\n\nvar SHA256 = map[string]string{}\n\nfunc F() {}\n";
        let hashes = [("a-b", "11".to_owned()), ("a-b-c-d", "22".to_owned())];
        let once = go(before, &hashes).unwrap_or_default();
        assert_eq!(
            once,
            "package prebuilt\n\nvar SHA256 = map[string]string{\n\
             \t\"a-b\":     \"11\",\n\t\"a-b-c-d\": \"22\",\n}\n\nfunc F() {}\n"
        );
        // A filled map is replaced, not appended to.
        let again = go(&once, &[("x", "33".to_owned())]).unwrap_or_default();
        assert_eq!(
            again,
            "package prebuilt\n\nvar SHA256 = map[string]string{\n\t\"x\": \"33\",\n}\n\nfunc F() {}\n"
        );
    }

    #[test]
    fn every_target_the_hook_and_loader_name_is_pinned() {
        let dart = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../bindings/dart/lib/src/hook/target.dart"),
        )
        .unwrap_or_default();
        // Dart has no musl target (Flutter's Linux is glibc); Go has.
        for triple in PREBUILT_TARGETS.iter().filter(|t| !t.ends_with("-musl")) {
            assert!(
                dart.contains(&format!("'{triple}'")),
                "{triple} is not a Dart target"
            );
        }
        // One `(OS.…, Architecture.…)` case per Dart target: a new one must be pinned too.
        assert_eq!(dart.matches("(OS.").count(), PREBUILT_TARGETS.len() - 2);
        let go = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(GO))
            .unwrap_or_default();
        for os in [
            "-apple-darwin",
            "-unknown-linux-musl",
            "-unknown-linux-gnu",
            "-pc-windows-msvc",
        ] {
            assert!(
                go.contains(&format!("\"{os}\"")),
                "the Go loader has no {os}"
            );
        }
    }

    #[test]
    fn the_swift_manifest_names_the_release() {
        let s = swift("1.2.3", &"ab".repeat(32));
        assert!(s.contains("/v1.2.3/CTeiGeodata.xcframework.zip\""));
        assert!(s.contains(&format!("checksum: \"{}\"", "ab".repeat(32))));
    }
}
