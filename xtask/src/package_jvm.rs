//! `cargo xtask package-jvm`: the JVM binding as it ships (phase 5.4, D-288).
//!
//! - `--android`: the JNI library (`bindings/jvm`, `teigeodata-jvm`) for `arm64-v8a`,
//!   `armeabi-v7a` and `x86_64` through cargo-ndk (`dist` profile, Android API 24), stripped,
//!   each 64-bit library checked for 16 KB `LOAD` alignment ([`check_alignment`]); with the
//!   Java classes (`--release 17`, desugared by D8) as `target/jvm/teigeodata-<v>.aar`.
//! - `--desktop`: the Java classes and the host's native library, plus any folder of others
//!   given with `--natives <dir>` (`<os>-<arch>/<file>`, as CI collects them), as
//!   `target/jvm/teigeodata-<v>.jar` under `META-INF/native/<os>-<arch>/`; `--test` runs the
//!   parity checks from the JAR with no `java.library.path`, so the JAR's loader is what finds
//!   the library.
//! - `--android-test`: the parity checks on a running emulator or device (`adb`), from a dex of
//!   the classes and the test, through `app_process`.
//! - `--maven [<dir>] [--sign]`: the Maven Central bundle of the JAR and the AAR in `<dir>`
//!   (default `target/jvm`), see [`crate::maven`].

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::package_c::{command, version};

/// The Android ABIs: (ABI folder, Rust target, 64-bit).
const ABIS: &[(&str, &str, bool)] = &[
    ("arm64-v8a", "aarch64-linux-android", true),
    ("armeabi-v7a", "armv7-linux-androideabi", false),
    ("x86_64", "x86_64-linux-android", true),
];

/// The lowest Android API level the library supports (D-288).
pub(crate) const MIN_SDK: &str = "24";

/// Google Play's page size for 64-bit libraries.
pub(crate) const PAGE: u64 = 16 * 1024;

const LIB: &str = "libteigeodata_jvm.so";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, Vec<String>> {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let out = root.join("target/jvm");
    std::fs::create_dir_all(&out).map_err(|e| vec![format!("{}: {e}", out.display())])?;
    let version = version(root)?;
    let mut summary = Vec::new();
    if has("--android") {
        summary.push(android(root, &out, &version)?);
    }
    if has("--desktop") {
        summary.push(desktop(
            root,
            &out,
            &version,
            value("--natives").map(PathBuf::from),
            has("--test"),
        )?);
    }
    if has("--android-test") {
        summary.push(android_test(root, &out)?);
    }
    if has("--maven") {
        let built = value("--maven")
            .filter(|v| !v.starts_with("--"))
            .map_or_else(|| crate::maven::default_dir(root), PathBuf::from);
        summary.push(crate::maven::bundle(
            root,
            &out,
            &version,
            &built,
            has("--sign"),
        )?);
    }
    if summary.is_empty() {
        return Err(vec![
            "name --android, --desktop, --android-test or --maven".into(),
        ]);
    }
    Ok(summary.join("\n"))
}

pub(crate) fn ndk_bin(tool: &str) -> Result<PathBuf, Vec<String>> {
    let ndk = std::env::var("ANDROID_NDK_HOME")
        .map_err(|_| vec!["ANDROID_NDK_HOME is not set".to_owned()])?;
    let prebuilt = Path::new(&ndk).join("toolchains/llvm/prebuilt");
    let host = std::fs::read_dir(&prebuilt)
        .map_err(|e| vec![format!("{}: {e}", prebuilt.display())])?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .next()
        .ok_or_else(|| vec![format!("no host toolchain in {}", prebuilt.display())])?;
    Ok(host.join("bin").join(tool))
}

/// The Java classes of the binding, compiled into `dir`.
fn classes(root: &Path, dir: &Path, with_test: bool) -> Result<(), Vec<String>> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    let mut sources = java_files(&root.join("bindings/jvm/java"))?;
    if with_test {
        sources.push(root.join("bindings/jvm/test/org/teispace/teigeodata/ParityTest.java"));
    }
    let mut args: Vec<String> = ["--release", "17", "-Xlint:all", "-Werror", "-d"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    args.push(dir.display().to_string());
    args.extend(sources.iter().map(|p| p.display().to_string()));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    command(root, "javac", &refs)
}

fn java_files(dir: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| vec![format!("{}: {e}", d.display())])? {
            let p = e.map_err(|e| vec![e.to_string()])?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "java") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn android(root: &Path, out: &Path, version: &str) -> Result<String, Vec<String>> {
    let bindings = root.join("bindings");
    let mut build = Command::new("cargo");
    build.current_dir(&bindings).arg("ndk");
    for (abi, _, _) in ABIS {
        build.args(["-t", abi]);
    }
    build.args([
        "--platform",
        MIN_SDK,
        "build",
        "--locked",
        "--profile",
        "dist",
        "-p",
        "teigeodata-jvm",
    ]);
    let status = build
        .status()
        .map_err(|e| vec![format!("cargo ndk: {e} (install cargo-ndk 4.1.2)")])?;
    if !status.success() {
        return Err(vec!["building the JNI library for Android failed".into()]);
    }
    let stage = out.join("aar");
    let _ = std::fs::remove_dir_all(&stage);
    let strip = ndk_bin("llvm-strip")?;
    let mut sizes = Vec::new();
    for (abi, target, wide) in ABIS {
        let dir = stage.join("jni").join(abi);
        std::fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        let lib = dir.join(LIB);
        let from = bindings.join("target").join(target).join("dist").join(LIB);
        std::fs::copy(&from, &lib).map_err(|e| vec![format!("{}: {e}", from.display())])?;
        command(
            root,
            &strip.display().to_string(),
            &["--strip-unneeded", &lib.display().to_string()],
        )?;
        let bytes = std::fs::read(&lib).map_err(|e| vec![format!("{}: {e}", lib.display())])?;
        if *wide {
            check_alignment(&bytes, PAGE).map_err(|e| vec![format!("{abi}: {e}")])?;
        }
        sizes.push(format!("{abi} {} KB", bytes.len() / 1024));
    }
    let classes_dir = out.join("classes");
    classes(root, &classes_dir, false)?;
    command(
        root,
        "jar",
        &[
            "--create",
            "--file",
            &stage.join("classes.jar").display().to_string(),
            "-C",
            &classes_dir.display().to_string(),
            ".",
        ],
    )?;
    let manifest = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <manifest xmlns:android=\"http://schemas.android.com/apk/res/android\"\n\
         \x20   package=\"org.teispace.teigeodata\">\n\
         \x20   <uses-sdk android:minSdkVersion=\"{MIN_SDK}\" />\n\
         </manifest>\n"
    );
    let files = [
        ("AndroidManifest.xml", manifest.as_str()),
        ("R.txt", ""),
        // The classes JNI reaches by name: kept whole, so a shrinking app keeps them.
        (
            "proguard.txt",
            "# teigeodata: the JNI layer finds these classes and members by name.\n\
             -keep class org.teispace.teigeodata.** { *; }\n",
        ),
    ];
    for (name, text) in files {
        std::fs::write(stage.join(name), text).map_err(|e| vec![format!("{name}: {e}")])?;
    }
    let aar = out.join(format!("teigeodata-{version}.aar"));
    let _ = std::fs::remove_file(&aar);
    zip_dir(root, &stage, &aar)?;
    let kb = std::fs::metadata(&aar).map_or(0, |m| m.len()) / 1024;
    Ok(format!(
        "{}: {kb} KB ({}; 64-bit LOAD segments 16 KB aligned)",
        aar.display(),
        sizes.join(", ")
    ))
}

fn desktop(
    root: &Path,
    out: &Path,
    version: &str,
    natives: Option<PathBuf>,
    test: bool,
) -> Result<String, Vec<String>> {
    let bindings = root.join("bindings");
    let status = Command::new("cargo")
        .current_dir(&bindings)
        .args([
            "build",
            "--locked",
            "--profile",
            "dist",
            "-p",
            "teigeodata-jvm",
        ])
        .status()
        .map_err(|e| vec![format!("cargo: {e}")])?;
    if !status.success() {
        return Err(vec!["building the JNI library failed".into()]);
    }
    let stage = out.join("jar");
    classes(root, &stage, false)?;
    let host = host_platform()?;
    let file = native_file_name();
    let mut platforms = vec![host.clone()];
    let place = |platform: &str, from: &Path| -> Result<(), Vec<String>> {
        let dir = stage.join("META-INF/native").join(platform);
        std::fs::create_dir_all(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        let name = from.file_name().unwrap_or_default();
        std::fs::copy(from, dir.join(name))
            .map(|_| ())
            .map_err(|e| vec![format!("{}: {e}", from.display())])
    };
    place(&host, &bindings.join("target/dist").join(&file))?;
    if let Some(dir) = natives {
        for e in std::fs::read_dir(&dir).map_err(|e| vec![format!("{}: {e}", dir.display())])? {
            let platform_dir = e.map_err(|e| vec![e.to_string()])?.path();
            let platform = platform_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if platform == host || !platform_dir.is_dir() {
                continue;
            }
            for f in std::fs::read_dir(&platform_dir).map_err(|e| vec![e.to_string()])? {
                place(&platform, &f.map_err(|e| vec![e.to_string()])?.path())?;
            }
            platforms.push(platform);
        }
    }
    platforms.sort();
    let jar = out.join(format!("teigeodata-{version}.jar"));
    let _ = std::fs::remove_file(&jar);
    command(
        root,
        "jar",
        &[
            "--create",
            "--file",
            &jar.display().to_string(),
            "-C",
            &stage.display().to_string(),
            ".",
        ],
    )?;
    let kb = std::fs::metadata(&jar).map_or(0, |m| m.len()) / 1024;
    let mut summary = format!("{}: {kb} KB ({})", jar.display(), platforms.join(", "));
    if test {
        let tests = out.join("test-classes");
        let _ = std::fs::remove_dir_all(&tests);
        std::fs::create_dir_all(&tests).map_err(|e| vec![format!("{}: {e}", tests.display())])?;
        command(
            root,
            "javac",
            &[
                "--release",
                "17",
                "-cp",
                &jar.display().to_string(),
                "-d",
                &tests.display().to_string(),
                &root
                    .join("bindings/jvm/test/org/teispace/teigeodata/ParityTest.java")
                    .display()
                    .to_string(),
            ],
        )?;
        let sep = if cfg!(windows) { ";" } else { ":" };
        let cp = format!("{}{sep}{}", jar.display(), tests.display());
        // No java.library.path: the library must come out of the JAR.
        command(
            &bindings,
            "java",
            &[
                "--enable-native-access=ALL-UNNAMED",
                "-cp",
                &cp,
                "org.teispace.teigeodata.ParityTest",
                "parity.json",
            ],
        )?;
        summary.push_str("\nparity passed from the JAR, its library extracted by NativeLoader");
    }
    Ok(summary)
}

/// The parity checks on an emulator or device: the classes and the test as dex (D8), the
/// library for the device's ABI, `bindings/parity.json` and the fixtures, run by
/// `app_process`.
fn android_test(root: &Path, out: &Path) -> Result<String, Vec<String>> {
    let sdk =
        std::env::var("ANDROID_HOME").map_err(|_| vec!["ANDROID_HOME is not set".to_owned()])?;
    let tools = Path::new(&sdk).join("build-tools");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&tools)
        .map_err(|e| vec![format!("{}: {e}", tools.display())])?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    versions.sort();
    let d8 = versions
        .last()
        .map(|v| v.join("d8"))
        .ok_or_else(|| vec!["no Android build-tools".to_owned()])?;
    let classes_dir = out.join("android-test-classes");
    classes(root, &classes_dir, true)?;
    let dex = out.join("android-test-dex");
    let _ = std::fs::remove_dir_all(&dex);
    std::fs::create_dir_all(&dex).map_err(|e| vec![format!("{}: {e}", dex.display())])?;
    let mut args: Vec<String> = vec![
        "--release".into(),
        "--min-api".into(),
        MIN_SDK.into(),
        "--output".into(),
        dex.display().to_string(),
    ];
    for f in walk_classes(&classes_dir)? {
        args.push(f.display().to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    command(root, &d8.display().to_string(), &refs)?;

    let abi = adb_setup(root, &["shell", "getprop", "ro.product.cpu.abi"])?;
    let abi = abi.trim();
    let lib = out.join("aar/jni").join(abi).join(LIB);
    if !lib.is_file() {
        return Err(vec![format!(
            "{} missing: run --android first",
            lib.display()
        )]);
    }
    let device = "/data/local/tmp/teigeodata";
    // A device just booted can refuse a command for a few seconds (its storage or adbd still
    // settling: CI saw `mkdir` fail with no message): setup steps, each idempotent, are tried
    // again; the parity run itself never is.
    adb_setup(root, &["shell", "rm", "-rf", device])?;
    adb_setup(root, &["shell", "mkdir", "-p", &format!("{device}/lib")])?;
    adb_setup(
        root,
        &[
            "push",
            &lib.display().to_string(),
            &format!("{device}/lib/{LIB}"),
        ],
    )?;
    adb_setup(
        root,
        &[
            "push",
            &dex.join("classes.dex").display().to_string(),
            &format!("{device}/classes.dex"),
        ],
    )?;
    adb_setup(
        root,
        &["shell", "mkdir", "-p", &format!("{device}/bindings")],
    )?;
    adb_setup(
        root,
        &[
            "push",
            &root.join("bindings/parity.json").display().to_string(),
            &format!("{device}/bindings/parity.json"),
        ],
    )?;
    adb_setup(
        root,
        &[
            "push",
            &root.join("bindings/fixtures").display().to_string(),
            &format!("{device}/bindings/"),
        ],
    )?;
    // app_process takes its class path from CLASSPATH; ART's default temporary folder does not
    // exist for a shell process, so the store test gets one.
    let script = format!(
        "mkdir -p {device}/tmp && cd {device}/bindings && CLASSPATH={device}/classes.dex \
         app_process -Djava.library.path={device}/lib -Djava.io.tmpdir={device}/tmp \
         /system/bin org.teispace.teigeodata.ParityTest parity.json 2>&1; echo exit=$?"
    );
    let text = adb_out(root, &["shell", &script])?;
    let _ = adb_out(root, &["shell", "rm", "-rf", device]);
    // The test's progress lines go to stderr, merged into the same stream above; the exit
    // status is its own line.
    if text.contains("jvm parity ok") && text.lines().any(|l| l.trim() == "exit=0") {
        Ok(format!(
            "parity passed on Android ({abi}) through app_process"
        ))
    } else {
        Err(vec![format!(
            "parity failed on Android ({abi}):\n{text}\ndevice log:\n{}",
            device_log(root)
        )])
    }
}

/// What the device logged about a failed run: a crash, or the reason the process was killed.
fn device_log(root: &Path) -> String {
    const KEYS: [&str; 9] = [
        "lowmemorykiller",
        "lmkd",
        "app_process",
        "teigeodata",
        "DEBUG",
        "libc",
        "AndroidRuntime",
        "Killing",
        "art ",
    ];
    let log = adb_out(root, &["logcat", "-d", "-t", "400"]).unwrap_or_default();
    log.lines()
        .filter(|l| KEYS.iter().any(|k| l.contains(k)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn walk_classes(dir: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| vec![format!("{}: {e}", d.display())])? {
            let p = e.map_err(|e| vec![e.to_string()])?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "class") {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// `adb_out` for an idempotent setup step: tried up to five times, two seconds apart, waiting
/// for the device before each retry.
fn adb_setup(root: &Path, args: &[&str]) -> Result<String, Vec<String>> {
    let mut last = Vec::new();
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = adb_out(root, &["wait-for-device"]);
        }
        match adb_out(root, args) {
            Ok(text) => return Ok(text),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn adb_out(root: &Path, args: &[&str]) -> Result<String, Vec<String>> {
    let out = Command::new("adb")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| vec![format!("adb: {e}")])?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(vec![format!("adb {} failed: {text}", args.join(" "))])
    }
}

fn zip_dir(root: &Path, dir: &Path, to: &Path) -> Result<(), Vec<String>> {
    // `jar` writes a plain zip and is on every machine that builds the JVM binding; `-M`
    // leaves out its manifest, which an AAR does not have.
    command(
        root,
        "jar",
        &[
            "--create",
            "--no-manifest",
            "--file",
            &to.display().to_string(),
            "-C",
            &dir.display().to_string(),
            ".",
        ],
    )
}

fn native_file_name() -> String {
    if cfg!(target_os = "windows") {
        "teigeodata_jvm.dll".into()
    } else if cfg!(target_os = "macos") {
        "libteigeodata_jvm.dylib".into()
    } else {
        "libteigeodata_jvm.so".into()
    }
}

/// The host's `<os>-<arch>`, named as `NativeLoader.platform()` names it.
fn host_platform() -> Result<String, Vec<String>> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        other => return Err(vec![format!("no JAR folder for {other}")]),
    };
    Ok(format!("{os}-{}", std::env::consts::ARCH))
}

/// Every `PT_LOAD` segment of a 64-bit little-endian ELF file is aligned to `page` (Google
/// Play's 16 KB rule for Android 15 and later devices).
pub(crate) fn check_alignment(elf: &[u8], page: u64) -> Result<(), String> {
    let u16_at = |at: usize| {
        elf.get(at..at + 2)
            .and_then(|b| b.try_into().ok())
            .map(u16::from_le_bytes)
    };
    let u32_at = |at: usize| {
        elf.get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let u64_at = |at: usize| {
        elf.get(at..at + 8)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes)
    };
    if elf.get(..4) != Some(b"\x7fELF".as_slice()) {
        return Err("not an ELF file".into());
    }
    if elf.get(4) != Some(&2) || elf.get(5) != Some(&1) {
        return Err("not a 64-bit little-endian ELF file".into());
    }
    let bad = || "a truncated ELF header".to_owned();
    let phoff = usize::try_from(u64_at(0x20).ok_or_else(bad)?).map_err(|_| bad())?;
    let phentsize = usize::from(u16_at(0x36).ok_or_else(bad)?);
    let phnum = usize::from(u16_at(0x38).ok_or_else(bad)?);
    let mut loads = 0;
    for i in 0..phnum {
        let at = phoff + i * phentsize;
        if u32_at(at).ok_or_else(bad)? != 1 {
            continue; // not PT_LOAD
        }
        loads += 1;
        let align = u64_at(at + 0x30).ok_or_else(bad)?;
        if align < page || align % page != 0 {
            return Err(format!(
                "LOAD segment {i} is aligned to {align} bytes, not {page} (link with NDK r28 or later, or -Wl,-z,max-page-size={page})"
            ));
        }
    }
    if loads == 0 {
        return Err("no LOAD segment".into());
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "a test builds a fixed-size ELF header by offset"
)]
mod tests {
    use super::*;

    /// A minimal ELF64 header with one program header of the given type and alignment.
    fn elf(kind: u32, align: u64) -> Vec<u8> {
        let mut b = vec![0u8; 0x40 + 0x38];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        b[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes());
        b[0x36..0x38].copy_from_slice(&0x38u16.to_le_bytes());
        b[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
        b[0x40..0x44].copy_from_slice(&kind.to_le_bytes());
        b[0x40 + 0x30..0x40 + 0x38].copy_from_slice(&align.to_le_bytes());
        b
    }

    #[test]
    fn sixteen_kb_loads_pass_and_four_kb_fail() {
        assert!(check_alignment(&elf(1, 0x4000), PAGE).is_ok());
        assert!(check_alignment(&elf(1, 0x10000), PAGE).is_ok());
        assert!(check_alignment(&elf(1, 0x1000), PAGE).is_err());
        assert!(
            check_alignment(&elf(6, 0x1000), PAGE).is_err(),
            "no LOAD at all"
        );
        assert!(check_alignment(b"\x7fELF\x01\x01", PAGE).is_err(), "32-bit");
        assert!(check_alignment(b"MZ", PAGE).is_err());
    }
}
