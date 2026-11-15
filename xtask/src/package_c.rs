//! `cargo xtask package-c`: the C library as it ships (phase 5.2, D-286).
//!
//! Builds `teigeodata-c` with the `dist` profile for one target and stages
//! `teigeodata-c-<version>-<target>[-lite]/`:
//!
//! ```text
//! include/teigeodata.h                  the generated header (lite: its features set to 0)
//! include/teigeodata.hpp                the generated C++17 wrapper over it (5.8d)
//! include/teigeodata_reader.h(pp)       the result reader, C99 and C++17, header-only (D-322)
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
//! package through `pkg-config` (shared and static) and through `CMake` (both targets), and
//! `bindings/c/parity.cpp` through `CMake` (shared), and runs each.
//!
//! `--prebuilt` also writes the shared library alone as the release asset the Dart hook and
//! the Go loader download, `teigeodata-<version>-<target>-<file>` (D-289, D-297; full variant
//! only). For Android and iOS targets that file is all it builds: their apps link the library
//! through Dart's hooks, not through a C package (Android through the NDK's clang for API 24,
//! the AAR's lowest; iOS for the Swift package's lowest version).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const HEADER: &str = "bindings/c/teigeodata.h";
const CPP_HEADER: &str = "bindings/c/teigeodata.hpp";
/// The hand-written result readers, copied as they are.
const READERS: [&str; 2] = ["teigeodata_reader.h", "teigeodata_reader.hpp"];

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
    /// Extra environment for cargo (a cross-compiler).
    env: Vec<(String, String)>,
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
        env: Vec::new(),
    };
    let prebuilt = has("--prebuilt");
    if prebuilt && plan.lite {
        return Err(vec!["--prebuilt is the full library: drop --lite".into()]);
    }
    if is_mobile(&plan.triple) {
        if !prebuilt {
            return Err(vec![format!(
                "{}: only --prebuilt (no C package for a mobile target)",
                plan.triple
            )]);
        }
        return mobile(plan);
    }
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
    if prebuilt {
        summary.push(write_prebuilt(&plan, &version, &shared)?);
    }
    let archive = archive(&plan.out, &name, os)?;
    let bytes = std::fs::metadata(&archive).map_or(0, |m| m.len());
    summary.push(format!("{}: {} KB", archive.display(), bytes / 1024));
    Ok(summary.join("\n"))
}

fn is_mobile(triple: &str) -> bool {
    triple.contains("-android") || triple.contains("-apple-ios")
}

/// The file name of the shared library on `triple`, as the Dart hook and the Go loader name
/// it (`Target.fileName`, `prebuilt.FileName`).
fn library_file(triple: &str) -> &'static str {
    match os_of(triple) {
        Os::Windows => "teigeodata.dll",
        Os::Apple => "libteigeodata.dylib",
        Os::Elf => "libteigeodata.so",
    }
}

/// The release asset name of `triple`'s library (`assetName` in the Dart hook, `Asset` in Go).
pub(crate) fn prebuilt_name(version: &str, triple: &str) -> String {
    format!("teigeodata-{version}-{triple}-{}", library_file(triple))
}

fn write_prebuilt(plan: &Plan, version: &str, shared: &Path) -> Result<String, Vec<String>> {
    let to = plan.out.join(prebuilt_name(version, &plan.triple));
    std::fs::create_dir_all(&plan.out).map_err(|e| vec![format!("{}: {e}", plan.out.display())])?;
    std::fs::copy(shared, &to).map_err(|e| vec![format!("{}: {e}", shared.display())])?;
    let bytes = std::fs::metadata(&to).map_or(0, |m| m.len());
    Ok(format!("{}: {} KB", to.display(), bytes / 1024))
}

/// An Android or iOS library: built with the target's compiler and deployment floor, its own
/// name checked, written as the release asset.
fn mobile(mut plan: Plan) -> Result<String, Vec<String>> {
    let triple = plan.triple.clone();
    let os = os_of(&triple);
    if triple.contains("-android") {
        let clang_triple = if triple == "armv7-linux-androideabi" {
            "armv7a-linux-androideabi"
        } else {
            triple.as_str()
        };
        let clang = crate::package_jvm::ndk_bin(&format!(
            "{clang_triple}{}-clang",
            crate::package_jvm::MIN_SDK
        ))?;
        let ar = crate::package_jvm::ndk_bin("llvm-ar")?;
        let key = triple.replace('-', "_");
        plan.env = vec![
            (format!("CC_{key}"), clang.display().to_string()),
            (format!("AR_{key}"), ar.display().to_string()),
            (
                format!("CARGO_TARGET_{}_LINKER", key.to_uppercase()),
                clang.display().to_string(),
            ),
        ];
    } else {
        plan.env = vec![(
            "IPHONEOS_DEPLOYMENT_TARGET".into(),
            crate::package_swift::IOS_MIN.into(),
        )];
    }
    build(&plan)?;
    let built = built_dir(&plan).join(library_file(&triple));
    let version = version(&plan.root)?;
    let mut summary = Vec::new();
    // Android loads a library by its file name, so its soname is unversioned (build.rs).
    summary.push(if os == Os::Apple {
        own_name(os, &built, &version)?
    } else {
        soname(&built, "libteigeodata.so")?
    });
    // Google Play's 16 KB page rule (Android 15 and later) for the 64-bit libraries, as the
    // AAR's (D-288); `armv7-linux-androideabi` is 32-bit.
    if triple.ends_with("-linux-android") {
        let bytes = std::fs::read(&built).map_err(|e| vec![format!("{}: {e}", built.display())])?;
        crate::package_jvm::check_alignment(&bytes, crate::package_jvm::PAGE)
            .map_err(|e| vec![format!("{triple}: {e}")])?;
        summary.push("LOAD segments 16 KB aligned".into());
    }
    summary.push(write_prebuilt(&plan, &version, &built)?);
    Ok(summary.join("\n"))
}

/// The soname of an ELF file, checked against `want` (an Android library is read with the
/// NDK's `llvm-readelf`, which reads any ELF).
fn soname(file: &Path, want: &str) -> Result<String, Vec<String>> {
    let readelf = crate::package_jvm::ndk_bin("llvm-readelf")?;
    let out = Command::new(&readelf)
        .arg("-d")
        .arg(file)
        .output()
        .map_err(|e| vec![format!("{}: {e}", readelf.display())])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let found = text
        .lines()
        .find(|l| l.contains("(SONAME)"))
        .and_then(|l| l.rsplit(' ').next())
        .unwrap_or_default()
        .trim_matches(['[', ']'])
        .to_owned();
    if found == want {
        Ok(format!("own name: {want}"))
    } else {
        Err(vec![format!(
            "{}: soname `{found}`, not `{want}`",
            file.display()
        )])
    }
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
        // Without the store, with every component (D-291).
        cmd.args(["--no-default-features", "--features", "components"]);
    }
    cmd.envs(plan.env.iter().map(|(k, v)| (k, v)));
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
    copy(&plan.root.join(CPP_HEADER), &include.join("teigeodata.hpp"))?;
    for file in READERS {
        copy(
            &plan.root.join("bindings/c").join(file),
            &include.join(file),
        )?;
    }
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
    let bundled = if os == Os::Windows {
        bundle_import_libs(plan, &native, &lib)?
    } else {
        Vec::new()
    };
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
        cmake_config(
            os,
            &shared_rel,
            static_lib,
            implib,
            &native,
            &bundled,
            plan.lite,
        ),
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
        "--color",
        "never",
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
        // Without the store, with every component (D-291).
        cmd.args(["--no-default-features", "--features", "components"]);
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
    Ok(line.split_whitespace().map(strip_ansi).collect())
}

/// A word without terminal colour codes (`ESC [ … m`), which a coloured note may carry even
/// with `--color never` set by a parent's environment.
fn strip_ansi(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The import libraries of the `windows-targets` crates (`windows.0.52.0.lib`) that the static
/// library names: they live in a crate's `lib/` folder, not on a user's machine, so the
/// package carries them. Returns their file names, copied into `lib`.
fn bundle_import_libs(
    plan: &Plan,
    native: &[String],
    lib: &Path,
) -> Result<Vec<String>, Vec<String>> {
    let wanted: Vec<&String> = native
        .iter()
        .filter(|l| {
            l.starts_with("windows.")
                && Path::new(l.as_str())
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("lib"))
        })
        .collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(&plan.root)
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .map_err(|e| vec![format!("cargo metadata: {e}")])?;
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| vec![format!("cargo metadata: {e}")])?;
    let dirs: Vec<PathBuf> = meta
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| {
            p.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|n| n.starts_with("windows_") && n.ends_with("_msvc"))
        })
        .filter_map(|p| p.get("manifest_path").and_then(serde_json::Value::as_str))
        .filter_map(|m| Path::new(m).parent().map(|d| d.join("lib")))
        .collect();
    let mut copied = Vec::new();
    for name in wanted {
        let from = dirs
            .iter()
            .map(|d| d.join(name))
            .find(|f| f.is_file())
            .ok_or_else(|| vec![format!("no crate holds {name} for {}", plan.triple)])?;
        std::fs::copy(&from, lib.join(name))
            .map_err(|e| vec![format!("{}: {e}", from.display())])?;
        copied.push(name.clone());
    }
    Ok(copied)
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
    bundled: &[String],
    lite: bool,
) -> String {
    // rustc names system libraries for a linker command line; CMake takes them bare or as
    // flags, and MSVC's `.lib` names as they are.
    let link: Vec<String> = native
        .iter()
        // MSVC's `/defaultlib:msvcrt` names the C runtime, which CMake links already.
        .filter(|l| !l.starts_with('/'))
        .map(|l| {
            if bundled.contains(l) {
                return format!("${{_tg_prefix}}/lib/{l}");
            }
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
    let parity_cpp = dir.join("parity.cpp");
    std::fs::copy(plan.root.join("bindings/c/parity.cpp"), &parity_cpp)
        .map_err(|e| vec![format!("parity.cpp: {e}")])?;
    // The golden values, beside the copies (`#include "parity_data.h"`).
    std::fs::copy(
        plan.root.join("bindings/c/parity_data.h"),
        dir.join("parity_data.h"),
    )
    .map_err(|e| vec![format!("parity_data.h: {e}")])?;
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

    done.extend(cmake_smoke(plan, os, stage, &dir, &parity, &parity_cpp)?);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(format!(
        "smoke: parity.c and parity.cpp passed through {}",
        done.join(", ")
    ))
}

/// The `CMake` half of the smoke test: `parity.c` against both targets, `parity.cpp` against the
/// shared one; the names of the runs that passed.
fn cmake_smoke(
    plan: &Plan,
    os: Os,
    stage: &Path,
    dir: &Path,
    parity: &Path,
    parity_cpp: &Path,
) -> Result<Vec<String>, Vec<String>> {
    let mut done = Vec::new();
    let project = dir.join("cmake");
    std::fs::create_dir_all(&project).map_err(|e| vec![format!("{}: {e}", project.display())])?;
    let lists = format!(
        "cmake_minimum_required(VERSION 3.16)\n\
         project(smoke C CXX)\n\
         find_package(teigeodata {version} REQUIRED CONFIG)\n\
         add_executable(smoke_shared \"{p}\")\n\
         target_link_libraries(smoke_shared PRIVATE teigeodata::teigeodata)\n\
         add_executable(smoke_static \"{p}\")\n\
         target_link_libraries(smoke_static PRIVATE teigeodata::static)\n\
         add_executable(smoke_cpp \"{cpp}\")\n\
         target_compile_features(smoke_cpp PRIVATE cxx_std_17)\n\
         target_link_libraries(smoke_cpp PRIVATE teigeodata::teigeodata)\n",
        version = version(&plan.root)?,
        p = parity.display().to_string().replace('\\', "/"),
        cpp = parity_cpp.display().to_string().replace('\\', "/"),
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
    for exe in ["smoke_shared", "smoke_static", "smoke_cpp"] {
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
    Ok(done)
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

pub(crate) fn command(root: &Path, program: &str, args: &[&str]) -> Result<(), Vec<String>> {
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

pub(crate) fn version(root: &Path) -> Result<String, Vec<String>> {
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
    fn colour_codes_are_stripped() {
        assert_eq!(strip_ansi("m\u{1b}[0m"), "m");
        assert_eq!(strip_ansi("\u{1b}[1m-lc"), "-lc");
        assert_eq!(strip_ansi("-liconv"), "-liconv");
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

    #[test]
    fn prebuilt_assets_are_named_as_the_dart_hook_and_go_name_them() {
        let cases = [
            (
                "aarch64-apple-ios-sim",
                "teigeodata-0.3.1-aarch64-apple-ios-sim-libteigeodata.dylib",
            ),
            (
                "armv7-linux-androideabi",
                "teigeodata-0.3.1-armv7-linux-androideabi-libteigeodata.so",
            ),
            (
                "x86_64-pc-windows-msvc",
                "teigeodata-0.3.1-x86_64-pc-windows-msvc-teigeodata.dll",
            ),
            (
                "aarch64-unknown-linux-musl",
                "teigeodata-0.3.1-aarch64-unknown-linux-musl-libteigeodata.so",
            ),
        ];
        for (triple, name) in cases {
            assert_eq!(prebuilt_name("0.3.1", triple), name);
        }
        assert!(is_mobile("x86_64-apple-ios") && is_mobile("x86_64-linux-android"));
        assert!(!is_mobile("aarch64-apple-darwin") && !is_mobile("x86_64-unknown-linux-gnu"));
    }
}
