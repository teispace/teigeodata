//! `cargo xtask flutter-check`: the Dart package in a Flutter app (D-289). The example at
//! `bindings/dart/example` keeps only its Dart sources; this makes its platform folders with
//! `flutter create .`, analyses it and runs its integration test on a device, so the build hook
//! is proven to put the library into an app on that platform.

use std::path::Path;
use std::process::Command;

use crate::package_c::command;

const USAGE: &str = "usage: cargo xtask flutter-check [--device <id> | --ios]";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut device, mut ios) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--device" => device = Some(it.next().ok_or_else(|| vec![USAGE.to_owned()])?.clone()),
            "--ios" => ios = true,
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let example = root.join("bindings/dart/example");
    let flutter = if cfg!(windows) {
        "flutter.bat"
    } else {
        "flutter"
    };
    command(
        &example,
        flutter,
        &[
            "create",
            "--platforms=android,ios,linux,macos,windows",
            "--project-name",
            "teigeodata_example",
            "--org",
            "org.teispace",
            ".",
        ],
    )?;
    // `flutter create` adds a widget test for its own template app; the example has its own.
    let _ = std::fs::remove_dir_all(example.join("test"));
    command(&example, flutter, &["pub", "get"])?;
    let dart = if cfg!(windows) { "dart.bat" } else { "dart" };
    command(&example, dart, &["analyze", "--fatal-infos"])?;

    let mut booted = None;
    let device = match (device, ios) {
        (Some(d), _) => d,
        (None, true) => {
            let (id, name, was_booted) = iphone()?;
            eprintln!("flutter-check: {name} ({id})");
            if !was_booted {
                // `boot` returns before the simulator is up; `bootstatus -b` boots and waits.
                command(root, "xcrun", &["simctl", "bootstatus", &id, "-b"])?;
                booted = Some(id.clone());
            }
            id
        }
        (None, false) => host().to_owned(),
    };
    // Streamed, not captured: an app build and a device run are long, and a stall must show
    // where it is in the log.
    let result = stream(
        &example,
        flutter,
        &["test", "integration_test", "-d", &device],
    );
    // A simulator this run booted is shut down again: the machine is shared.
    if let Some(id) = booted {
        let _ = command(root, "xcrun", &["simctl", "shutdown", &id]);
    }
    result?;
    Ok(format!(
        "flutter-check: the example's integration test passed on {device}"
    ))
}

/// The desktop Flutter device of this machine.
fn host() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// Runs `program` with its output going to this process's, so a CI log shows its progress.
fn stream(dir: &Path, program: &str, args: &[&str]) -> Result<(), Vec<String>> {
    let status = Command::new(program)
        .current_dir(dir)
        .args(args)
        .status()
        .map_err(|e| vec![format!("{program}: {e}")])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![format!(
            "{program} {} failed: {status}",
            args.join(" ")
        )])
    }
}

/// How a simulator is preferred: booted, then by iOS version, then a Pro Max.
type Rank = (bool, (u32, u32), bool);

/// The iPhone simulator to test on: a booted one first, then on the newest iOS runtime, a Pro
/// Max (the machine's shared set) before others. Its id, name and whether it is booted.
fn iphone() -> Result<(String, String, bool), Vec<String>> {
    let out = Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "-j"])
        .output()
        .map_err(|e| vec![format!("xcrun: {e}")])?;
    let list: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| vec![format!("simctl list: {e}")])?;
    let mut best: Option<(Rank, String, String)> = None;
    let runtimes = list.get("devices").and_then(serde_json::Value::as_object);
    for (runtime, devices) in runtimes.into_iter().flatten() {
        let Some(version) = ios_version(runtime) else {
            continue;
        };
        for d in devices.as_array().into_iter().flatten() {
            let field = |k: &str| d.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
            let (name, id) = (field("name"), field("udid"));
            if !name.starts_with("iPhone") || id.is_empty() {
                continue;
            }
            let rank = (
                field("state") == "Booted",
                version,
                name.contains("Pro Max"),
            );
            if best.as_ref().is_none_or(|(r, _, _)| rank > *r) {
                best = Some((rank, id.to_owned(), name.to_owned()));
            }
        }
    }
    best.map(|((booted, _, _), id, name)| (id, name, booted))
        .ok_or_else(|| vec!["no iPhone simulator available".into()])
}

/// The iOS version of a simulator runtime identifier, such as
/// `com.apple.CoreSimulator.SimRuntime.iOS-27-2` or `…iOS-27-2-iPhone19-4`.
fn ios_version(runtime: &str) -> Option<(u32, u32)> {
    let rest = runtime.rsplit_once("SimRuntime.iOS-")?.1;
    let mut parts = rest.split('-');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::ios_version;

    #[test]
    fn runtime_versions_are_read() {
        assert_eq!(
            ios_version("com.apple.CoreSimulator.SimRuntime.iOS-27-2"),
            Some((27, 2))
        );
        assert_eq!(
            ios_version("com.apple.CoreSimulator.SimRuntime.iOS-27-2-iPhone19-4"),
            Some((27, 2))
        );
        assert_eq!(
            ios_version("com.apple.CoreSimulator.SimRuntime.watchOS-12-0"),
            None
        );
    }
}
