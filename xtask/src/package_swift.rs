//! `cargo xtask package-swift`: the `XCFramework` the Swift package ships (phase 5.3, D-287).
//!
//! Builds `teigeodata-c`'s static library (`dist` profile) for iOS, the iOS simulators (arm64
//! and `x86_64`) and macOS (arm64 and `x86_64`), joins each platform's slices with `lipo`, and
//! makes `bindings/swift/CTeiGeodata.xcframework` with the header and a module map naming the
//! module `CTeiGeodata`. Then zips it to `target/apple/CTeiGeodata.xcframework.zip` and prints
//! the checksum a release's `binaryTarget` names.
//!
//! `--test` runs the Swift parity checks against it: `swift test` on macOS and
//! `xcodebuild test` on an iOS simulator (`--simulator <name>`, else the first iPhone
//! available).

use std::path::Path;
use std::process::Command;

use crate::package_c::command;

/// The deployment targets, the same as `bindings/swift/Package.swift`'s platforms.
const IOS_MIN: &str = "15.0";
const MACOS_MIN: &str = "12.0";

/// Each platform of the `XCFramework` and the Rust targets in its fat library.
const PLATFORMS: &[(&str, &[&str])] = &[
    ("ios", &["aarch64-apple-ios"]),
    (
        "ios-simulator",
        &["aarch64-apple-ios-sim", "x86_64-apple-ios"],
    ),
    ("macos", &["aarch64-apple-darwin", "x86_64-apple-darwin"]),
];

const MODULE_MAP: &str = "module CTeiGeodata {\n    header \"teigeodata.h\"\n    export *\n}\n";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let io = |what: &Path, e: std::io::Error| vec![format!("{}: {e}", what.display())];

    let targets: Vec<&str> = PLATFORMS
        .iter()
        .flat_map(|(_, t)| t.iter().copied())
        .collect();
    let mut build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    build
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "--profile",
            "dist",
            "-p",
            "teigeodata-c",
        ])
        .env("IPHONEOS_DEPLOYMENT_TARGET", IOS_MIN)
        .env("MACOSX_DEPLOYMENT_TARGET", MACOS_MIN);
    for t in &targets {
        build.args(["--target", t]);
    }
    let status = build.status().map_err(|e| vec![format!("cargo: {e}")])?;
    if !status.success() {
        return Err(vec![
            "building teigeodata-c for Apple targets failed".into(),
        ]);
    }

    let apple = root.join("target/apple");
    let framework = assemble(root, &apple)?;

    let zip = apple.join("CTeiGeodata.xcframework.zip");
    command(
        root,
        "ditto",
        &[
            "-c",
            "-k",
            "--sequesterRsrc",
            "--keepParent",
            &framework.display().to_string(),
            &zip.display().to_string(),
        ],
    )?;
    let checksum = output(
        &root.join("bindings/swift"),
        "swift",
        &["package", "compute-checksum", &zip.display().to_string()],
    )?;
    let kb = std::fs::metadata(&zip).map_or(0, |m| m.len()) / 1024;
    let mut summary = vec![format!(
        "{}: {kb} KB, SwiftPM checksum {}",
        zip.display(),
        checksum.trim()
    )];
    std::fs::write(
        apple.join("CTeiGeodata.xcframework.zip.checksum"),
        checksum.trim(),
    )
    .map_err(|e| io(&apple, e))?;

    if has("--test") {
        summary.push(test(root, value("--simulator"))?);
    }
    Ok(summary.join("\n"))
}

/// Joins each platform's slices and makes `bindings/swift/CTeiGeodata.xcframework`.
fn assemble(root: &Path, apple: &Path) -> Result<std::path::PathBuf, Vec<String>> {
    let io = |what: &Path, e: std::io::Error| vec![format!("{}: {e}", what.display())];
    let _ = std::fs::remove_dir_all(apple);
    let headers = apple.join("headers");
    std::fs::create_dir_all(&headers).map_err(|e| io(&headers, e))?;
    std::fs::copy(
        root.join("bindings/c/teigeodata.h"),
        headers.join("teigeodata.h"),
    )
    .map_err(|e| io(&headers, e))?;
    std::fs::write(headers.join("module.modulemap"), MODULE_MAP).map_err(|e| io(&headers, e))?;

    let mut create: Vec<String> = vec!["-create-xcframework".into()];
    for (platform, slices) in PLATFORMS {
        let dir = apple.join(platform);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let fat = dir.join("libteigeodata.a");
        let mut lipo = vec!["-create".to_owned()];
        for t in *slices {
            lipo.push(
                root.join("target")
                    .join(t)
                    .join("dist/libteigeodata.a")
                    .display()
                    .to_string(),
            );
        }
        lipo.extend(["-output".into(), fat.display().to_string()]);
        let refs: Vec<&str> = lipo.iter().map(String::as_str).collect();
        command(root, "lipo", &refs)?;
        create.extend([
            "-library".into(),
            fat.display().to_string(),
            "-headers".into(),
            headers.display().to_string(),
        ]);
    }
    let framework = root.join("bindings/swift/CTeiGeodata.xcframework");
    let _ = std::fs::remove_dir_all(&framework);
    create.extend(["-output".into(), framework.display().to_string()]);
    let refs: Vec<&str> = create.iter().map(String::as_str).collect();
    command(root, "xcodebuild", &refs)?;
    Ok(framework)
}

/// The parity checks against the `XCFramework` on macOS and on an iOS simulator.
fn test(root: &Path, simulator: Option<String>) -> Result<String, Vec<String>> {
    let swift = root.join("bindings/swift");
    let env = [("TEIGEODATA_XCFRAMEWORK", "1")];
    run_env(&swift, "swift", &["test", "-c", "release"], &env)?;
    let name = match simulator {
        Some(name) => name,
        None => first_iphone()?,
    };
    let destination = format!("platform=iOS Simulator,name={name}");
    run_env(
        &swift,
        "xcodebuild",
        &[
            "test",
            "-scheme",
            "TeiGeodata-Package",
            "-destination",
            &destination,
            "-derivedDataPath",
            &root.join("target/apple/derived").display().to_string(),
            "-quiet",
        ],
        &env,
    )?;
    Ok(format!(
        "parity passed against the XCFramework: macOS (swift test) and {name} (xcodebuild test)"
    ))
}

fn first_iphone() -> Result<String, Vec<String>> {
    let text = output(
        Path::new("."),
        "xcrun",
        &["simctl", "list", "devices", "available"],
    )?;
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("iPhone"))
        .and_then(|l| l.split(" (").next())
        .map(str::to_owned)
        .ok_or_else(|| vec!["no iPhone simulator available".into()])
}

fn run_env(
    dir: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<(), Vec<String>> {
    let mut cmd = Command::new(program);
    cmd.current_dir(dir).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| vec![format!("{program}: {e}")])?;
    if out.status.success() {
        Ok(())
    } else {
        let tail = |b: &[u8]| {
            let text = String::from_utf8_lossy(b);
            let lines: Vec<&str> = text.lines().collect();
            let skip = lines.len().saturating_sub(40);
            lines.into_iter().skip(skip).collect::<Vec<_>>().join("\n")
        };
        Err(vec![format!(
            "{program} {} failed:\n{}\n{}",
            args.join(" "),
            tail(&out.stdout),
            tail(&out.stderr)
        )])
    }
}

fn output(dir: &Path, program: &str, args: &[&str]) -> Result<String, Vec<String>> {
    let out = Command::new(program)
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| vec![format!("{program}: {e}")])?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(vec![format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        )])
    }
}
