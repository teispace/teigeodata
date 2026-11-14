---
title: "The JVM and Android packages: the AAR, the desktop JAR, 16 KB pages"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# The JVM and Android packages: the AAR, the desktop JAR, 16 KB pages

## 1. Question

Phase 5.4 ships one generated JNI layer two ways, decided in the
[bindings note](bindings-and-mcp.md) (§3.6 and §3.7) and spike S6's plan:

- **An AAR** for Android, built with cargo-ndk, with a 16 KB ELF gate in CI;
- **A desktop JAR** carrying native libraries per OS and architecture.

The open questions:

- whether a Rust library linked by the NDK meets Google Play's 16 KB page rule without extra
  flags (left **unverified** in that note);
- the lowest Android API level;
- the Java language level;
- how a desktop JAR finds its library;
- how the parity checks run on Android without a Gradle project.

## 2. Summary and recommendation

- **16 KB pages hold by default with NDK r28 and later.** Measured 2026-11-14 with NDK r28.2 and
  cargo-ndk 4.1.2: `libteigeodata_jvm.so` for arm64-v8a and x86_64 has every `LOAD` segment
  aligned to 0x4000. armeabi-v7a has 0x1000; Play's rule is for 64-bit devices.
  `cargo xtask package-jvm --android` checks the 64-bit libraries by reading their ELF program
  headers itself, so no `readelf` is needed, and fails below 16 KB. A planted
  `-Wl,-z,max-page-size=4096` fails it. GitHub's Ubuntu image defaults to NDK r27, which
  predates this default, so CI uses `ANDROID_NDK_LATEST_HOME` (r30).
- **minSdk 24 and Java 17.** The Java layer uses `CompletableFuture` and `java.util.function`
  (API 24) and records. D8 desugars records at any API level; `d8 --min-api 24` dexes the
  classes with no warning. The classes are compiled with `--release 17`, the class level D8 and
  AGP 8 and later read. The desktop JAR asks for Java 17.
- **The AAR is assembled by hand, without Gradle.** It is a zip holding:
  - `AndroidManifest.xml` (minSdk 24, no permissions);
  - `classes.jar`;
  - `jni/<abi>/libteigeodata_jvm.so`, stripped;
  - `proguard.txt`, which keeps `org.teispace.teigeodata.**` because JNI reaches classes and
    members by name;
  - an empty `R.txt`.

  No permission is merged into apps. An app that uses the store adds `INTERNET` itself, so the
  engine alone stays offline.
- **The desktop JAR puts libraries at `META-INF/native/<os>-<arch>/`.** A hand-written
  `NativeLoader` tries `System.loadLibrary` first, which covers Android and a desktop caller that
  set `java.library.path`. Failing that, it extracts the JAR's library into
  `<tmp>/teigeodata-<sha256>/` and loads it from there:
  - the file is written beside its final name and moved into place, so a concurrent JVM never
    loads half a file;
  - it is written once and reused (proven: a second run made no new folder);
  - a missing platform fails with the platform's name.

  CI builds six desktop platforms, tests the JAR on each OS that can run it, and assembles
  one JAR with all six.
- **Parity on Android runs through `app_process`.** The classes and `ParityTest` are dexed with
  D8, then pushed with the device ABI's library, `parity.json` and the fixtures, and run by
  `app_process` on API 24 and API 35 x86_64 emulators in CI (ReactiveCircus/android-emulator-runner).
  No Gradle project or instrumented test is needed. This machine runs no emulator, by the
  shared-machine rule.

Measured 2026-11-14 (`dist` profile, fat LTO, stripped):

| Package | Size |
|---|---|
| AAR | 5,128 KB |
| arm64-v8a library | 3,178 KB |
| armeabi-v7a library | 2,297 KB |
| x86_64 library | 3,826 KB |
| Desktop JAR, macOS arm64 only | 1,693 KB |
| Both, built on this Mac | 2 min 39 s |

## 3. Findings

- **The libraries need only the NDK's system libraries.** Each `NEEDED` list is `libc.so`,
  `libm.so` and `libdl.so`, so no `libc++_shared.so` ships.
- **No soname is set.** Android loads a library by its file name, and so does the loader's
  extracted copy on desktop.
- **The generated Java uses records**, so `--release 11` fails and 17 is the floor.
  `NativeLoader` avoids `InputStream.readAllBytes`, which Android has only from API 33. It runs
  only on desktop, but the class ships in the AAR too.
- **API 24 is now run, and it found three things** (2026-11-14), none of which a desktop JVM
  or the API 35 emulator shows:
  - the generated classes used `java.lang.ref.Cleaner`, which Android has only from API 33,
    so `TeiGeodata` failed to load. A hand-written `NativeCleaner` does the same with a
    phantom reference per owner and one daemon thread, on any API level; the parity test
    checks it runs once, both on `clean()` and for an unreachable owner;
  - `java.nio.file` is API 26, so `NativeLoader` and the test use `java.io`;
  - the test's `Files.readString`, `Path.of` and `List.of` are Java 11 and are not on Android.

  Android's `InetAddress.getLoopbackAddress()` is `::1`, so the test's local host binds to
  `127.0.0.1` by name.
- **`app_process` details.** It takes its class path from `CLASSPATH`. Its system class loader
  searches `java.library.path`. ART's default `java.io.tmpdir` does not exist for a shell
  process, so the test is given one.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| AAR assembled by `cargo xtask` | No Gradle or AGP in the build; reproducible; seconds | We keep its layout (a documented zip) | ours | built 2026-11-14 | **Chosen** |
| Gradle library module with AGP | The usual Android build; lint | Gradle, AGP and SDK setup in every build; slower CI | Apache-2.0 | Gradle 9.9.1; AGP 9.x | Later, for Maven Central publishing if the POM tooling needs it |
| Parity through `app_process` | Real ART and JNI; no app or instrumented test | Not an app process: no Context | n/a | Android API 35 | **Chosen** |
| Instrumented test in a test app | Real app process | Gradle project and APK build | Apache-2.0 | n/a | Not needed for parity |
| Desktop natives extracted from the JAR | One artifact for every OS; works with any build tool | A temp folder on the user's disk | ours | built | **Chosen** |
| One JAR per platform (classifiers) | No extraction | Users pick the right classifier | n/a | n/a | Rejected |

## 5. What we build ourselves, and why

`xtask/src/package_jvm.rs` holds the AAR assembly, the JAR with natives, the 16 KB check (the
ELF64 program header table, about 40 lines, with unit tests) and the `app_process` run.
`bindings/jvm/java/.../NativeLoader.java` is hand-written; the generated `TeiGeodata` class
calls it.

## 6. Impact on the plan

- [Phase 5](../../07-planning/phases/phase-5.md) 5.4, done when `jvm.yml` is green.
- D-288.
- 5.10 publishes both to Maven Central (`com.teispace`), which needs the maintainer's namespace.

## 7. Risks and open items

- **API 24 is checked by running, not by a tool.** `jvm.yml` runs parity on API 24 and API 35
  emulators; a call the tests never make is not covered. Android lint's `NewApi` would cover
  every call but needs a Gradle project. A lighter gate was prototyped on 2026-11-14: list
  every `java.*` method the compiled classes call (`javap -c`) and look each up, through
  superclasses, in a platform's `android.jar`. Against API 31 (the oldest installed here) the
  classes call 131 such methods and none is missing, and a planted class using `Cleaner`,
  `ByteBuffer.slice(int, int)`, the covariant `ByteBuffer.position(int)`, `Files.readString`
  and `Path.of` is caught on all five. To become a fast-check gate it needs API 24's
  `android.jar`, so that `java.nio.file` (API 26) is caught too.
- **Two more API traps** found by the API 24 run: `ByteBuffer.slice(int, int)` (Java 13) in
  the test, and the progress lines on stderr arriving after `exit=` on stdout (the runner now
  merges them and looks for the `exit=0` line).
- **Native debug symbols are stripped** from the AAR. A separate symbols zip for Play Console
  is a later extra.
- **Maven Central** needs `-sources.jar` and `-javadoc.jar`, signatures and a POM (bindings
  note §3.6). Publishing work belongs to 5.10.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| cargo-ndk | 4.1.2 | 2026-11-14 | crates.io |
| jni | 0.22.4 | 2026-11-14 | crates.io |
| Android NDK | r28.2 local; r30 (30.0.16248370) in CI | 2026-11-14 | SDK manager XML; runner image readme |
| Gradle | 9.9.1 | 2026-11-14 | services.gradle.org |
| ReactiveCircus/android-emulator-runner | v2.38.0 | 2026-11-14 | GitHub API |
| Android build-tools (D8) | 36.1.0 local; 37.0.0 in CI | 2026-11-14 | SDK; runner image readme |

## 9. Sources

- [Support 16 KB page sizes](https://developer.android.com/guide/practices/page-sizes), Android Developers, accessed 2026-11-06 ([bindings note](bindings-and-mcp.md)).
- [AAR file anatomy](https://developer.android.com/studio/projects/android-library#aar-contents), Android Developers, accessed 2026-11-14.
- [Ubuntu 24.04 runner image](https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Readme.md), GitHub, accessed 2026-11-14.
- [android-emulator-runner](https://github.com/ReactiveCircus/android-emulator-runner), ReactiveCircus, v2.38.0, accessed 2026-11-14.
