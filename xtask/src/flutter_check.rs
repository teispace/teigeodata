//! `cargo xtask flutter-check`: the Dart package in a Flutter app (D-289). The example at
//! `bindings/dart/example` keeps only its Dart sources; this makes its platform folders with
//! `flutter create .`, analyses it and runs its integration test on a device, so the build hook
//! is proven to put the library into an app on that platform.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

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
    // where it is in the log. On the iOS simulator a run is bounded and tried twice: a GitHub
    // runner's simulator now and then hangs launching the app after a clean build (run
    // 43690656789: built in 143 s, then idle until the step's 30 minutes). The second try
    // starts from a freshly booted simulator and is announced, so a real hang still fails.
    // Before it, the simulator's log of the app is printed, so a hang says where it stopped;
    // on a CI runner, whose simulator is its own, the simulator is also erased, so the second
    // try does not reuse a stuck install (runs 43700637931, 2026-11-15: both tries hung after
    // the build). A local simulator is shared and never erased.
    let args = ["test", "integration_test", "-d", device.as_str()];
    let result = if ios {
        let mut r = stream_within(&example, flutter, &args, DEVICE_RUN);
        if r.is_err() {
            println!(
                "::warning::flutter-check: the iOS run failed or hung; rebooting the simulator and trying once more"
            );
            app_log(&device);
            let _ = command(root, "xcrun", &["simctl", "shutdown", &device]);
            if std::env::var_os("GITHUB_ACTIONS").is_some() {
                command(root, "xcrun", &["simctl", "erase", &device])?;
            }
            command(root, "xcrun", &["simctl", "bootstatus", &device, "-b"])?;
            r = stream_within(&example, flutter, &args, DEVICE_RUN);
            if r.is_err() {
                app_log(&device);
            }
        }
        r
    } else {
        stream(&example, flutter, &args)
    };
    // A simulator this run booted is shut down again: the machine is shared.
    if let Some(id) = booted {
        let _ = command(root, "xcrun", &["simctl", "shutdown", &id]);
    }
    result?;
    Ok(format!(
        "flutter-check: the example's integration test passed on {device}"
    ))
}

/// Prints the last ten minutes of the simulator's log from the app (`Runner`) and from the
/// launch services that start it, at most 200 lines: why a run hung. Best effort.
fn app_log(device: &str) {
    let out = Command::new("xcrun")
        .args([
            "simctl",
            "spawn",
            device,
            "log",
            "show",
            "--last",
            "10m",
            "--style",
            "compact",
            "--predicate",
            r#"process == "Runner" OR (subsystem == "com.apple.FrontBoard" AND eventMessage CONTAINS "Runner")"#,
        ])
        .output();
    let Ok(out) = out else { return };
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    println!(
        "::group::flutter-check: the simulator's log of the app (last {} lines)",
        lines.len().min(200)
    );
    for line in lines.iter().skip(lines.len().saturating_sub(200)) {
        println!("{line}");
    }
    println!("::endgroup::");
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

/// How long one device run may take: a clean iOS build is about 2.5 minutes on a GitHub runner
/// and the test under one.
const DEVICE_RUN: Duration = Duration::from_mins(10);

/// As [`stream`], killed after `limit`.
fn stream_within(
    dir: &Path,
    program: &str,
    args: &[&str],
    limit: Duration,
) -> Result<(), Vec<String>> {
    let mut child = Command::new(program)
        .current_dir(dir)
        .args(args)
        .spawn()
        .map_err(|e| vec![format!("{program}: {e}")])?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(vec![format!(
                    "{program} {} failed: {status}",
                    args.join(" ")
                )]);
            }
            Ok(None) if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(vec![format!(
                    "{program} {} still running after {} s: stopped",
                    args.join(" "),
                    limit.as_secs()
                )]);
            }
            Ok(None) => std::thread::sleep(Duration::from_secs(1)),
            Err(e) => return Err(vec![format!("{program}: {e}")]),
        }
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
    use super::{ios_version, stream_within};

    #[cfg(unix)]
    #[test]
    fn a_device_run_is_stopped_at_its_limit() {
        let dir = std::env::temp_dir();
        let second = std::time::Duration::from_secs(1);
        assert!(stream_within(&dir, "true", &[], second).is_ok());
        assert!(stream_within(&dir, "false", &[], second).is_err());
        let started = std::time::Instant::now();
        let err = stream_within(&dir, "sleep", &["30"], second).unwrap_err();
        assert!(err[0].contains("still running after 1 s"), "{err:?}");
        assert!(started.elapsed().as_secs() < 10, "stopped, not waited for");
    }

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
