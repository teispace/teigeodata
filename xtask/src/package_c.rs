//! `cargo xtask package-c`: the C library as it ships (phase 5.2, D-286).
//!
//! Builds `teigeodata-c` with the `dist` profile for one target and stages
//! `teigeodata-c-<version>-<target>[-lite]/`:
//!
//! ```text
//! include/teigeodata.h                  the generated header (lite: its features set to 0)
//! lib/libteigeodata.so.<abi>, .so       ELF: the shared library and its link name
//! lib/libteigeodata.dylib               Apple: install name @rpath/libteigeodata.dylib
//! bin/teigeodata.dll, lib/teigeodata.dll.lib, lib/teigeodata.lib    Windows
//! lib/libteigeodata.a                   the static library (Unix)
//! lib/pkgconfig/teigeodata.pc           relocatable through ${pcfiledir}
//! lib/cmake/teigeodata/*.cmake          `teigeodata::teigeodata` (shared), `teigeodata::static`
//! LICENSE, NOTICE
//! ```
//!
//! then archives it (`.tar.gz`, `.zip` for Windows) and checks the shared library's exports
//! (`cargo xtask abi --lib`). `--smoke` builds `bindings/c/parity.c` against the staged
//! package through `pkg-config` (shared and static) and through `CMake` (both targets), and runs
//! each.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const HEADER: &str = "bindings/c/teigeodata.h";

/// The kind of library files a target produces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Os {
    Apple,
    Windows,
    Elf,
}

struct Plan {
    root: PathBuf,
    target: Option<String>,
    triple: String,
    lite: bool,
    out: PathBuf,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let target = value("--target");
    let triple = match &target {
        Some(t) => t.clone(),
        None => host_triple()?,
    };
    let plan = Plan {
        root: root.to_path_buf(),
        target,
        triple,
        lite: has("--lite"),
        out: value("--out").map_or_else(|| root.join("target/package"), PathBuf::from),
    };
    let os = os_of(&plan.triple);
    build(&plan)?;
    let built = built_dir(&plan);
    let version = version(root)?;
    let name = format!(
        "teigeodata-c-{version}-{}{}",
        plan.triple,
        if plan.lite { "-lite" } else { "" }
    );
    let stage = plan.out.join(&name);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|e| vec![format!("{}: {e}", stage.display())])?;
    }
    let shared = stage_files(&plan, os, &built, &stage, &version)?;
    let mut summary = vec![own_name(os, &shared, &version)?];
    if os == Os::Windows {
        summary.push("exports not checked on Windows yet (research note c-abi-gates §7)".into());
    } else {
        let mut gate = vec!["--since".to_owned(), last_tag(root)?, "--lib".into()];
        gate.push(shared.display().to_string());
        if plan.lite {
            gate.push("--lite".into());
        }
        summary.push(crate::abi::run(root, &gate)?);
    }
    if has("--smoke") {
        summary.push(smoke(&plan, os, &stage)?);
    }
    let archive = archive(&plan.out, &name, os)?;
    let bytes = std::fs::metadata(&archive).map_or(0, |m| m.len());
    summary.push(format!("{}: {} KB", archive.display(), bytes / 1024));
    Ok(summary.join("\n"))
}

fn build(plan: &Plan) -> Result<(), Vec<String>> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(&plan.root).args([
        "build",
        "--locked",
        "--profile",
        "dist",
        "-p",
        "teigeodata-c",
    ]);
    if let Some(t) = &plan.target {
        cmd.args(["--target", t]);
    }
    if plan.lite {
        cmd.arg("--no-default-features");
    }
    let status = cmd.status().map_err(|e| vec![format!("cargo: {e}")])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec!["building teigeodata-c failed".into()])
    }
}

fn built_dir(plan: &Plan) -> PathBuf {
    let target = plan.root.join("target");
    match &plan.target {
        Some(t) => target.join(t).join("dist"),
        None => target.join("dist"),
    }
}

/// Copies the files into `stage` and returns the shared library's staged path.
fn stage_files(
    plan: &Plan,
    os: Os,
    built: &Path,
    stage: &Path,
    version: &str,
) -> Result<PathBuf, Vec<String>> {
    let io = |what: &Path, e: std::io::Error| vec![format!("{}: {e}", what.display())];
    let (include, lib) = (stage.join("include"), stage.join("lib"));
    let cmake = lib.join("cmake/teigeodata");
    let pkgconfig = lib.join("pkgconfig");
    for dir in [&include, &cmake, &pkgconfig] {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let copy =
        |from: &Path, to: &Path| std::fs::copy(from, to).map(|_| ()).map_err(|e| io(from, e));

    let mut header =
        std::fs::read_to_string(plan.root.join(HEADER)).map_err(|e| io(Path::new(HEADER), e))?;
    if plan.lite {
        let on = "#define TEIGEODATA_FEATURE_STORE 1";
        if !header.contains(on) {
            return Err(vec![format!("{HEADER} lacks `{on}`")]);
        }
        header = header.replace(on, "#define TEIGEODATA_FEATURE_STORE 0");
    }
    std::fs::write(include.join("teigeodata.h"), header).map_err(|e| io(&include, e))?;
    for file in ["LICENSE", "NOTICE"] {
        copy(&plan.root.join(file), &stage.join(file))?;
    }

    let abi = abi_suffix(version);
    let (shared, static_lib, implib) = match os {
        Os::Apple => {
            let to = lib.join("libteigeodata.dylib");
            copy(&built.join("libteigeodata.dylib"), &to)?;
            copy(&built.join("libteigeodata.a"), &lib.join("libteigeodata.a"))?;
            (to, "libteigeodata.a", None)
        }
        Os::Elf => {
            let real = format!("libteigeodata.so.{abi}");
            let to = lib.join(&real);
            copy(&built.join("libteigeodata.so"), &to)?;
            symlink(&real, &lib.join("libteigeodata.so"))?;
            copy(&built.join("libteigeodata.a"), &lib.join("libteigeodata.a"))?;
            (to, "libteigeodata.a", None)
        }
        Os::Windows => {
            let bin = stage.join("bin");
            std::fs::create_dir_all(&bin).map_err(|e| io(&bin, e))?;
            let to = bin.join("teigeodata.dll");
            copy(&built.join("teigeodata.dll"), &to)?;
            copy(
                &built.join("teigeodata.dll.lib"),
                &lib.join("teigeodata.dll.lib"),
            )?;
            copy(&built.join("teigeodata.lib"), &lib.join("teigeodata.lib"))?;
            (to, "teigeodata.lib", Some("teigeodata.dll.lib"))
        }
    };
    let native = native_static_libs(plan)?;
    std::fs::write(
        pkgconfig.join("teigeodata.pc"),
        pc_file(version, &native, plan.lite),
    )
    .map_err(|e| io(&pkgconfig, e))?;
    let shared_rel = shared
        .strip_prefix(stage)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    std::fs::write(
        cmake.join("teigeodataConfig.cmake"),
        cmake_config(os, &shared_rel, static_lib, implib, &native, plan.lite),
    )
    .map_err(|e| io(&cmake, e))?;
    std::fs::write(
        cmake.join("teigeodataConfigVersion.cmake"),
        cmake_version(version),
    )
    .map_err(|e| io(&cmake, e))?;
    Ok(shared)
}

/// The name a program linked against the shared library records, read from the file: the
/// install name on Apple, the soname on ELF (`build.rs` of teigeodata-c). A wrong one passes
/// the smoke test from the build tree and fails on a user's machine, so it is checked apart.
fn own_name(os: Os, shared: &Path, version: &str) -> Result<String, Vec<String>> {
    let (program, args, want): (&str, &[&str], String) = match os {
        Os::Apple => ("otool", &["-D"], "@rpath/libteigeodata.dylib".into()),
        Os::Elf => (
            "readelf",
            &["-d"],
            format!("[libteigeodata.so.{}]", abi_suffix(version)),
        ),
        Os::Windows => return Ok("own name: a DLL is found by its file name".into()),
    };
    let out = Command::new(program)
        .args(args)
        .arg(shared)
        .output()
        .map_err(|e| vec![format!("{program}: {e}")])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let found = match os {
        Os::Apple => text.lines().nth(1).unwrap_or_default().trim().to_owned(),
        _ => text
            .lines()
            .find(|l| l.contains("(SONAME)"))
            .and_then(|l| l.rsplit(' ').next())
            .unwrap_or_default()
            .to_owned(),
    };
    if found == want {
        Ok(format!("own name: {want}"))
    } else {
        Err(vec![format!(
            "{} records `{found}` as its name, not `{want}`",
            shared.display()
        )])
    }
}

/// What a program linking the static library must also link, from rustc.
fn native_static_libs(plan: &Plan) -> Result<Vec<String>, Vec<String>> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(&plan.root).args([
        "rustc",
        "--locked",
        "--profile",
        "dist",
        "-p",
        "teigeodata-c",
        "--crate-type",
        "staticlib",
    ]);
    if let Some(t) = &plan.target {
        cmd.args(["--target", t]);
    }
    if plan.lite {
        cmd.arg("--no-default-features");
    }
    // Cargo replays the note from its cache when the crate is fresh.
    cmd.args(["--", "--print", "native-static-libs"]);
    let out = cmd
        .output()
        .map_err(|e| vec![format!("cargo rustc: {e}")])?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text
        .lines()
        .find_map(|l| l.split("native-static-libs:").nth(1))
        .ok_or_else(|| vec![format!("rustc gave no native-static-libs:\n{text}")])?;
    Ok(line.split_whitespace().map(str::to_owned).collect())
}

fn pc_file(version: &str, native: &[String], lite: bool) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "prefix=${{pcfiledir}}/../..");
    let _ = writeln!(s, "includedir=${{prefix}}/include");
    let _ = writeln!(s, "libdir=${{prefix}}/lib");
    let _ = writeln!(s);
    let _ = writeln!(s, "Name: teigeodata");
    let _ = writeln!(
        s,
        "Description: Offline geodata: places, admin areas, time zones, elevation{}",
        if lite { " (lite: no store)" } else { "" }
    );
    let _ = writeln!(s, "Version: {version}");
    let _ = writeln!(s, "URL: https://github.com/teispace/teigeodata");
    let _ = writeln!(s, "Cflags: -I${{includedir}}");
    let _ = writeln!(s, "Libs: -L${{libdir}} -lteigeodata");
    let _ = writeln!(s, "Libs.private: {}", native.join(" "));
    s
}

fn cmake_config(
    os: Os,
    shared: &str,
    static_lib: &str,
    implib: Option<&str>,
    native: &[String],
    lite: bool,
) -> String {
    // rustc names system libraries for a linker command line; CMake takes them bare or as
    // flags, and MSVC's `.lib` names as they are.
    let link: Vec<String> = native
        .iter()
        // MSVC's `/defaultlib:msvcrt` names the C runtime, which CMake links already.
        .filter(|l| !l.starts_with('/'))
        .map(|l| {
            l.strip_prefix("-l")
                .map_or_else(|| l.clone(), str::to_owned)
        })
        .collect();
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# teigeodata's CMake package (cargo xtask package-c). Targets: teigeodata::teigeodata,\n\
         # the shared library, and teigeodata::static. Relocatable: paths are relative to this file."
    );
    let _ = writeln!(
        s,
        "get_filename_component(_tg_prefix \"${{CMAKE_CURRENT_LIST_DIR}}/../../..\" ABSOLUTE)"
    );
    let _ = writeln!(
        s,
        "set(teigeodata_LITE {})",
        if lite { "ON" } else { "OFF" }
    );
    let _ = writeln!(s, "if(NOT TARGET teigeodata::teigeodata)");
    let _ = writeln!(s, "  add_library(teigeodata::teigeodata SHARED IMPORTED)");
    let _ = writeln!(
        s,
        "  set_target_properties(teigeodata::teigeodata PROPERTIES"
    );
    let _ = writeln!(s, "    IMPORTED_LOCATION \"${{_tg_prefix}}/{shared}\"");
    match (os, implib) {
        (Os::Windows, Some(implib)) => {
            let _ = writeln!(s, "    IMPORTED_IMPLIB \"${{_tg_prefix}}/lib/{implib}\"");
        }
        (Os::Apple, _) => {
            let _ = writeln!(s, "    IMPORTED_SONAME \"@rpath/libteigeodata.dylib\"");
        }
        _ => {
            let soname = shared.rsplit('/').next().unwrap_or(shared);
            let _ = writeln!(s, "    IMPORTED_SONAME \"{soname}\"");
        }
    }
    let _ = writeln!(
        s,
        "    INTERFACE_INCLUDE_DIRECTORIES \"${{_tg_prefix}}/include\")"
    );
    let _ = writeln!(s, "endif()");
    let _ = writeln!(s, "if(NOT TARGET teigeodata::static)");
    let _ = writeln!(s, "  add_library(teigeodata::static STATIC IMPORTED)");
    let _ = writeln!(s, "  set_target_properties(teigeodata::static PROPERTIES");
    let _ = writeln!(
        s,
        "    IMPORTED_LOCATION \"${{_tg_prefix}}/lib/{static_lib}\""
    );
    let _ = writeln!(
        s,
        "    INTERFACE_INCLUDE_DIRECTORIES \"${{_tg_prefix}}/include\""
    );
    let _ = writeln!(s, "    INTERFACE_LINK_LIBRARIES \"{}\")", link.join(";"));
    let _ = writeln!(s, "endif()");
    let _ = writeln!(s, "unset(_tg_prefix)");
    s
}

/// Before 1.0 a minor release may break the ABI, so only the same minor is compatible; after
/// it, the same major.
fn cmake_version(version: &str) -> String {
    format!(
        "# Compatible: the same {} as {version} and not older (D-286).\n\
         set(PACKAGE_VERSION \"{version}\")\n\
         set(PACKAGE_VERSION_COMPATIBLE FALSE)\n\
         if(NOT PACKAGE_FIND_VERSION VERSION_GREATER PACKAGE_VERSION)\n\
         \x20 string(REGEX MATCH \"^[0-9]+\\\\.[0-9]+\" _tg_have \"${{PACKAGE_VERSION}}\")\n\
         \x20 string(REGEX MATCH \"^[0-9]+\\\\.[0-9]+\" _tg_want \"${{PACKAGE_FIND_VERSION}}\")\n\
         \x20 if(PACKAGE_FIND_VERSION_MAJOR EQUAL 0)\n\
         \x20   if(_tg_have STREQUAL _tg_want)\n\
         \x20     set(PACKAGE_VERSION_COMPATIBLE TRUE)\n\
         \x20   endif()\n\
         \x20 elseif(PACKAGE_FIND_VERSION_MAJOR STREQUAL \"{major}\")\n\
         \x20   set(PACKAGE_VERSION_COMPATIBLE TRUE)\n\
         \x20 endif()\n\
         \x20 if(PACKAGE_FIND_VERSION STREQUAL PACKAGE_VERSION)\n\
         \x20   set(PACKAGE_VERSION_EXACT TRUE)\n\
         \x20 endif()\n\
         endif()\n",
        if version.starts_with("0.") {
            "minor"
        } else {
            "major"
        },
        major = version.split('.').next().unwrap_or("0"),
    )
}

/// `0.<minor>` before 1.0, `<major>` after: the part of the version that changes with a break.
fn abi_suffix(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some("0"), Some(minor)) => format!("0.{minor}"),
        (Some(major), _) => major.to_owned(),
        _ => "0".into(),
    }
}

/// Builds and runs `bindings/c/parity.c` against the staged package: `pkg-config` shared and
/// static, `CMake` shared and static.
fn smoke(plan: &Plan, os: Os, stage: &Path) -> Result<String, Vec<String>> {
    if plan
        .target
        .as_deref()
        .is_some_and(|t| t != host_triple().unwrap_or_default())
    {
        return Ok("smoke skipped: cross target".into());
    }
    let dir = plan.out.join("smoke");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    // A copy beside nothing: `#include "teigeodata.h"` looks beside the source first, which
    // in bindings/c/ would be the repository's header, not the staged one.
    let parity = dir.join("parity.c");
    std::fs::copy(plan.root.join("bindings/c/parity.c"), &parity)
        .map_err(|e| vec![format!("parity.c: {e}")])?;
    let mut done = Vec::new();

    if os != Os::Windows {
        let pc = stage.join("lib/pkgconfig");
        for (kind, flags) in [("shared", "--libs"), ("static", "--static --libs")] {
            let exe = dir.join(format!("pc-{kind}"));
            let pkg = format!("pkg-config --cflags {flags} teigeodata");
            let link = if kind == "static" {
                // Name the archive so the linker cannot pick the shared library.
                format!(
                    "$({pkg} | sed 's|-lteigeodata|{}|')",
                    stage.join("lib/libteigeodata.a").display()
                )
            } else {
                format!("$({pkg}) -Wl,-rpath,{}", stage.join("lib").display())
            };
            let script = format!(
                "cc -std=c11 -Wall -Wextra -Werror {} {link} -o {}",
                parity.display(),
                exe.display()
            );
            sh(&plan.root, &script, &[("PKG_CONFIG_PATH", &pc)])?;
            run_exe(&plan.root, &exe)?;
            done.push(format!("pkg-config {kind}"));
        }
    }

    let project = dir.join("cmake");
    std::fs::create_dir_all(&project).map_err(|e| vec![format!("{}: {e}", project.display())])?;
    let lists = format!(
        "cmake_minimum_required(VERSION 3.16)\n\
         project(smoke C)\n\
         find_package(teigeodata {version} REQUIRED CONFIG)\n\
         add_executable(smoke_shared \"{p}\")\n\
         target_link_libraries(smoke_shared PRIVATE teigeodata::teigeodata)\n\
         add_executable(smoke_static \"{p}\")\n\
         target_link_libraries(smoke_static PRIVATE teigeodata::static)\n",
        version = version(&plan.root)?,
        p = parity.display().to_string().replace('\\', "/"),
    );
    std::fs::write(project.join("CMakeLists.txt"), lists)
        .map_err(|e| vec![format!("CMakeLists.txt: {e}")])?;
    let build = project.join("build");
    let prefix = format!("-DCMAKE_PREFIX_PATH={}", stage.display());
    let rpath = format!("-DCMAKE_BUILD_RPATH={}", stage.join("lib").display());
    command(
        &plan.root,
        "cmake",
        &[
            "-S",
            &project.display().to_string(),
            "-B",
            &build.display().to_string(),
            &prefix,
            &rpath,
            "-DCMAKE_BUILD_TYPE=Release",
        ],
    )?;
    command(
        &plan.root,
        "cmake",
        &[
            "--build",
            &build.display().to_string(),
            "--config",
            "Release",
        ],
    )?;
    for exe in ["smoke_shared", "smoke_static"] {
        let mut path = build.join(exe);
        if os == Os::Windows {
            path = build.join("Release").join(format!("{exe}.exe"));
            // The DLL is found beside the program or on PATH.
            let _ = std::fs::copy(
                stage.join("bin/teigeodata.dll"),
                build.join("Release/teigeodata.dll"),
            );
        }
        run_exe(&plan.root, &path)?;
        done.push(format!("cmake {exe}"));
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(format!(
        "smoke: parity.c passed through {}",
        done.join(", ")
    ))
}

fn sh(root: &Path, script: &str, env: &[(&str, &Path)]) -> Result<(), Vec<String>> {
    let mut cmd = Command::new("sh");
    cmd.current_dir(root).args(["-c", script]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| vec![format!("sh: {e}")])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(vec![format!(
            "`{script}` failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )])
    }
}

fn command(root: &Path, program: &str, args: &[&str]) -> Result<(), Vec<String>> {
    let out = Command::new(program)
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| vec![format!("{program}: {e}")])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(vec![format!(
            "{program} {} failed:\n{}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )])
    }
}

/// Runs a parity program from the root, where its fixture paths resolve.
fn run_exe(root: &Path, exe: &Path) -> Result<(), Vec<String>> {
    command(root, &exe.display().to_string(), &[])
}

fn archive(out: &Path, name: &str, os: Os) -> Result<PathBuf, Vec<String>> {
    let (file, args): (String, Vec<String>) = if os == Os::Windows {
        let f = format!("{name}.zip");
        (f.clone(), vec!["-a".into(), "-cf".into(), f, name.into()])
    } else {
        let f = format!("{name}.tar.gz");
        (f.clone(), vec!["-czf".into(), f, name.into()])
    };
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    command(out, "tar", &refs)?;
    Ok(out.join(file))
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) -> Result<(), Vec<String>> {
    std::os::unix::fs::symlink(target, link).map_err(|e| vec![format!("{}: {e}", link.display())])
}

#[cfg(not(unix))]
fn symlink(_target: &str, link: &Path) -> Result<(), Vec<String>> {
    Err(vec![format!(
        "{}: an ELF package is staged on a Unix host",
        link.display()
    )])
}

fn os_of(triple: &str) -> Os {
    if triple.contains("-apple-") {
        Os::Apple
    } else if triple.contains("-windows-") {
        Os::Windows
    } else {
        Os::Elf
    }
}

fn host_triple() -> Result<String, Vec<String>> {
    let out = Command::new("rustc")
        .args(["--print", "host-tuple"])
        .output()
        .map_err(|e| vec![format!("rustc: {e}")])?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn last_tag(root: &Path) -> Result<String, Vec<String>> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"])
        .output()
        .map_err(|e| vec![format!("git: {e}")])?;
    let tag = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if tag.is_empty() {
        Err(vec![
            "no release tag: fetch the tags (`git fetch --tags`)".into(),
        ])
    } else {
        Ok(tag)
    }
}

fn version(root: &Path) -> Result<String, Vec<String>> {
    let text = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| vec![format!("Cargo.toml: {e}")])?;
    text.lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .map(|v| v.trim_end_matches('"').to_owned())
        .ok_or_else(|| vec!["Cargo.toml: no workspace version".into()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_abi_suffix_changes_only_with_a_break() {
        assert_eq!(abi_suffix("0.3.1"), "0.3");
        assert_eq!(abi_suffix("1.4.2"), "1");
    }

    #[test]
    fn the_pc_file_is_relocatable() {
        let pc = pc_file("0.3.1", &["-lm".into()], false);
        assert!(pc.starts_with("prefix=${pcfiledir}/../..\n"));
        assert!(pc.contains("Libs.private: -lm\n"));
    }

    #[test]
    fn targets_map_to_their_files() {
        assert_eq!(os_of("aarch64-apple-darwin"), Os::Apple);
        assert_eq!(os_of("x86_64-pc-windows-msvc"), Os::Windows);
        assert_eq!(os_of("x86_64-unknown-linux-gnu"), Os::Elf);
        assert_eq!(os_of("aarch64-linux-android"), Os::Elf);
    }
}
