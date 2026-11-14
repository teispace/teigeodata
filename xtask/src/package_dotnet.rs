//! `cargo xtask package-dotnet`: the `NuGet` package `Teispace.TeiGeodata` (5.8c, D-298), with
//! the native library of each runtime identifier under `runtimes/<rid>/native/`, written to
//! `target/dotnet/`.
//!
//! With `--natives <dir>` (the release), `<dir>/<rid>/<file>` must exist for every runtime
//! identifier. Without it, this machine's library is built (the `dist` profile) and only its
//! runtime identifier is packed. `--test` restores the package into a fresh project, with a
//! `NuGet` cache of its own and no `TEIGEODATA_LIB`, and runs the parity program from it under
//! the JIT and as a `NativeAOT` executable: the library must come from the package.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::package_c::version;

const USAGE: &str = "usage: cargo xtask package-dotnet [--natives <dir>] [--test]";

const PACKAGE: &str = "Teispace.TeiGeodata";

/// Each runtime identifier the package carries and its library's file name.
const RIDS: &[(&str, &str)] = &[
    ("osx-arm64", "libteigeodata.dylib"),
    ("osx-x64", "libteigeodata.dylib"),
    ("linux-x64", "libteigeodata.so"),
    ("linux-arm64", "libteigeodata.so"),
    ("linux-musl-x64", "libteigeodata.so"),
    ("linux-musl-arm64", "libteigeodata.so"),
    ("win-x64", "teigeodata.dll"),
    ("win-arm64", "teigeodata.dll"),
];

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let (mut natives, mut test) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--natives" => {
                natives = Some(PathBuf::from(
                    it.next().ok_or_else(|| vec![USAGE.to_owned()])?,
                ));
            }
            "--test" => test = true,
            _ => return Err(vec![USAGE.to_owned()]),
        }
    }
    let version = version(root)?;
    check_version(root, &version)?;
    let out = root.join("target/dotnet");
    let _ = fs::remove_dir_all(&out);
    let libraries: Vec<(&str, PathBuf)> = match &natives {
        Some(dir) => RIDS
            .iter()
            .map(|(rid, file)| (*rid, dir.join(rid).join(file)))
            .collect(),
        None => vec![host_library(root)?],
    };
    let missing: Vec<String> = libraries
        .iter()
        .filter(|(_, f)| !f.is_file())
        .map(|(_, f)| format!("{}: missing", f.display()))
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let runtimes = out.join("stage/runtimes");
    let mut report = String::new();
    for (rid, library) in &libraries {
        let dir = runtimes.join(rid).join("native");
        fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        let name = library.file_name().unwrap_or_default();
        fs::copy(library, dir.join(name))
            .map_err(|e| vec![format!("{}: {e}", library.display())])?;
        let size = fs::metadata(library).map_or(0, |m| m.len());
        let _ = writeln!(report, "runtimes/{rid}/native: {size} B");
    }
    let project = root.join("bindings/dotnet/TeiGeodata");
    dotnet(
        root,
        &[
            "pack",
            &project.display().to_string(),
            "-c",
            "Release",
            "-o",
            &out.display().to_string(),
            &format!("-p:Version={version}"),
            &format!("-p:TeiGeodataNatives={}", runtimes.display()),
        ],
        &[],
    )?;
    let package = out.join(format!("{PACKAGE}.{version}.nupkg"));
    if !package.is_file() {
        return Err(vec![format!("{}: not written", package.display())]);
    }
    let size = fs::metadata(&package).map_or(0, |m| m.len());
    let _ = writeln!(report, "{}: {} KB", package.display(), size / 1024);
    if test {
        if natives.is_some() {
            return Err(vec![
                "--test runs this machine's runtime identifier: leave out --natives".into(),
            ]);
        }
        let rid = libraries.first().map_or("", |(rid, _)| *rid);
        test_package(root, &out, &version, rid)?;
        let _ = writeln!(
            report,
            "  restored from the package; parity ok under the JIT and NativeAOT ({rid})"
        );
    }
    Ok(report.trim_end().to_owned())
}

/// The version `Library.Version` checks must be the workspace's.
fn check_version(root: &Path, version: &str) -> Result<(), Vec<String>> {
    let support = root.join("bindings/dotnet/TeiGeodata/Support.cs");
    let text =
        fs::read_to_string(&support).map_err(|e| vec![format!("{}: {e}", support.display())])?;
    if text.contains(&format!("public const string Version = \"{version}\";")) {
        Ok(())
    } else {
        Err(vec![format!(
            "{}: Library.Version is not the workspace version {version}",
            support.display()
        )])
    }
}

/// Builds this machine's library (the `dist` profile); its runtime identifier and file.
fn host_library(root: &Path) -> Result<(&'static str, PathBuf), Vec<String>> {
    let os = match std::env::consts::OS {
        "macos" => "osx",
        "windows" => "win",
        "linux" if cfg!(target_env = "musl") => "linux-musl",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    let wanted = format!("{os}-{arch}");
    let &(rid, file) = RIDS
        .iter()
        .find(|(rid, _)| *rid == wanted)
        .ok_or_else(|| vec![format!("no .NET runtime identifier is packed for {wanted}")])?;
    crate::package_c::command(
        root,
        "cargo",
        &[
            "build",
            "--locked",
            "--profile",
            "dist",
            "-p",
            "teigeodata-c",
        ],
    )?;
    Ok((rid, root.join("target/dist").join(file)))
}

/// Restores the package into `out/test` and runs the parity program from it.
fn test_package(root: &Path, out: &Path, version: &str, rid: &str) -> Result<(), Vec<String>> {
    let dir = out.join("test");
    let write = |name: &str, text: &str| {
        fs::write(dir.join(name), text).map_err(|e| vec![format!("{name}: {e}")])
    };
    fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    write(
        "nuget.config",
        &format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<configuration>\n  <packageSources>\n    \
             <clear />\n    <add key=\"local\" value=\"{}\" />\n    \
             <add key=\"nuget.org\" value=\"https://api.nuget.org/v3/index.json\" />\n  \
             </packageSources>\n</configuration>\n",
            out.display()
        ),
    )?;
    let program = root.join("bindings/dotnet/Parity/Program.cs");
    write(
        "Parity.csproj",
        &format!(
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    \
             <TargetFramework>net10.0</TargetFramework>\n    <Nullable>enable</Nullable>\n    \
             <ImplicitUsings>enable</ImplicitUsings>\n    <LangVersion>latest</LangVersion>\n    \
             <InvariantGlobalization>true</InvariantGlobalization>\n    \
             <AssemblyName>Parity</AssemblyName>\n  </PropertyGroup>\n  <ItemGroup>\n    \
             <Compile Include=\"{}\" />\n    \
             <PackageReference Include=\"{PACKAGE}\" Version=\"[{version}]\" />\n  </ItemGroup>\n</Project>\n",
            program.display()
        ),
    )?;
    let cache = dir.join("packages").display().to_string();
    let env = [("NUGET_PACKAGES", cache.as_str())];
    dotnet(&dir, &["run", "-c", "Release"], &env)?;
    let aot = dir.join("aot");
    dotnet(
        &dir,
        &[
            "publish",
            "-c",
            "Release",
            "-r",
            rid,
            "-p:PublishAot=true",
            "-o",
            &aot.display().to_string(),
        ],
        &env,
    )?;
    let exe = aot.join(if rid.starts_with("win") {
        "Parity.exe"
    } else {
        "Parity"
    });
    let library = RIDS.iter().find(|(r, _)| *r == rid).map_or("", |(_, f)| f);
    if !aot.join(library).is_file() {
        return Err(vec![format!(
            "{}: the package's library was not published",
            aot.display()
        )]);
    }
    run_checked(
        Command::new(&exe)
            .current_dir(&dir)
            .env_remove("TEIGEODATA_LIB"),
        &exe.display().to_string(),
    )
}

fn dotnet(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<(), Vec<String>> {
    let mut cmd = Command::new("dotnet");
    cmd.current_dir(dir)
        .args(args)
        .env_remove("TEIGEODATA_LIB")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .env("DOTNET_NOLOGO", "1");
    for (k, v) in env {
        cmd.env(k, v);
    }
    run_checked(&mut cmd, &format!("dotnet {}", args.join(" ")))
}

/// Runs a command with its output shown, and fails when it does.
fn run_checked(cmd: &mut Command, what: &str) -> Result<(), Vec<String>> {
    let status = cmd.status().map_err(|e| vec![format!("{what}: {e}")])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![format!("{what} failed ({status})")])
    }
}
