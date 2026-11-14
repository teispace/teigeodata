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
            if !was_booted {
                command(root, "xcrun", &["simctl", "boot", &id])?;
                booted = Some(id.clone());
            }
            eprintln!("flutter-check: {name} ({id})");
            id
        }
        (None, false) => host().to_owned(),
    };
    let result = command(
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

/// The first available iPhone simulator: its id, name and whether it is already booted.
fn iphone() -> Result<(String, String, bool), Vec<String>> {
    let out = Command::new("xcrun")
        .args(["simctl", "list", "devices", "available"])
        .output()
        .map_err(|e| vec![format!("xcrun: {e}")])?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("iPhone"))
        .find_map(|l| {
            let (name, rest) = l.split_once(" (")?;
            let (id, rest) = rest.split_once(')')?;
            Some((id.to_owned(), name.to_owned(), rest.contains("Booted")))
        })
        .ok_or_else(|| vec!["no iPhone simulator available".into()])
}
