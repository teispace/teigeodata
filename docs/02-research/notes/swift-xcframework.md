---
title: "Swift: the XCFramework, the privacy manifest and simulator tests"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# Swift: the XCFramework, the privacy manifest and simulator tests

## 1. Question

Phase 5.3 ships the Swift binding as SwiftPM expects: an XCFramework as a `binaryTarget`, a
privacy manifest, and tests on iOS, macOS and the simulators. The strategy is already in the
[bindings note](bindings-and-mcp.md), §3.8: SwiftPM only, since CocoaPods trunk turns read-only
on 2026-12-02. This note answers the open details:

- the slices;
- static or dynamic;
- which of Apple's required-reason APIs the library calls;
- how the parity checks run on a simulator;
- how one package serves development and release.

## 2. Summary and recommendation

- **A static XCFramework, `CTeiGeodata`, with three slices.** It is built from `teigeodata-c`'s
  static library with the `dist` profile:
  - `ios-arm64`;
  - `ios-arm64_x86_64-simulator`;
  - `macos-arm64_x86_64`.

  The headers folder holds `teigeodata.h` and a module map naming the module `CTeiGeodata`,
  the name the generated Swift wrapper imports. It is static because an app's linker then
  strips the functions the app never calls, which is tree-shaking for Swift. A dynamic
  framework would ship all of it.
- **Deployment targets: iOS 15 and macOS 12.** They are set for the Rust builds
  (`IPHONEOS_DEPLOYMENT_TARGET`, `MACOSX_DEPLOYMENT_TARGET`) and in `Package.swift` alike. The
  wrapper (typed throws, `AsyncStream`, task cancellation) builds and passes there.
- **The privacy manifest declares one API category: file timestamps.** The library imports
  `stat`, `fstat`, `fstatat` and `lstat`, read from the iOS static library's undefined symbols
  on 2026-11-14. It imports none of the other listed functions:
  - boot time: `mach_absolute_time`, `systemUptime`;
  - disk space: `statfs`, `statvfs` and their kin;
  - user defaults and active keyboards.

  The manifest's reasons are C617.1 (files in the app's container) and 3B52.1 (files the user
  opened). It declares no tracking and no collected data. It sits in the `TeiGeodata` target's
  resources, so SwiftPM puts it in `TeiGeodata_TeiGeodata.bundle`; this is where Apple expects
  a static library's manifest.
- **One `Package.swift`, two modes.**
  - Development (default): the system library built in `target/`, on macOS and Linux.
  - `TEIGEODATA_XCFRAMEWORK=1`: the XCFramework beside the manifest.

  A release's root `Package.swift` points the same targets at the release asset's URL and
  checksum. That belongs to the release pipeline (5.10).
- **The parity checks are a library, `ParityChecks`.** The `Parity` executable (macOS, Linux)
  and a Swift Testing test (`ParityTests`) both call it. `xcodebuild test` runs the test on an
  iOS simulator, which reads the repository's fixtures in place.
- **`cargo xtask package-swift --test`** builds the XCFramework, zips it, prints the SwiftPM
  checksum, and runs the checks against it with `swift test` on macOS and `xcodebuild test` on
  a simulator.

Measured 2026-11-14 on this Mac (Xcode 27.2, Swift 6.4.2):

| Measure | Result |
|---|---|
| Zip size | 17,253 KB |
| Full run, five targets with fat LTO plus both tests | 2 min 56 s |
| Simulator test | passes on iPhone 18 Pro Max |
| Planted wrong `tz.version` | `** TEST FAILED **` |
| Manifest in the built bundle | present |

## 3. Findings

- **Xcode's `nm` cannot read Rust 1.99's bitcode.** The precompiled `compiler_builtins` objects
  carry LLVM 23 bitcode, which Xcode's `nm` (Apple LLVM 21) rejects ("Unknown attribute
  kind"). Linking is unaffected: the machine code is what `ld` uses, and the macOS static smoke
  test and the simulator test both link. To inspect, use the symbols the other objects list,
  or rustup's `llvm-nm`.
- **TLS needs no Security framework.** It is ureq with rustls over ring and Mozilla's roots
  (D-168). rustc's `native-static-libs` for `aarch64-apple-ios` is `-liconv -lSystem -lc -lm`,
  all of which an app links already.
- **Paths are portable.** The parity program resolves every fixture beside `parity.json`, so
  the same code runs from `swift run`, `swift test` and a simulator.
- **The zip's bytes vary between runs.** `ditto` stores file times, so the checksum differs
  each time. A release must state the checksum of the file it uploads, which the workflow does.

- **The reason codes.** For file timestamps there are four:
  - C617.1: metadata of files in the app's container, app group or CloudKit container;
  - 3B52.1: files the user granted access to;
  - DDA9.1: showing timestamps to the user;
  - 0A2A.1: an SDK that wraps the timestamp API for the app to call.

  We use the first two. The library never returns a timestamp to the app, which rules out
  DDA9.1 and 0A2A.1. These meanings come from secondary sources that agree with each other;
  Apple's page itself was not reached (**unverified against the primary page**; check it before
  the first App Store submission).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Static XCFramework, `binaryTarget` | The app's linker strips unused code; one zip; SwiftPM native | Two Rust static libraries in one app collide ([packaging note](c-packaging.md), §7) | ours | Xcode 27.2 | **Chosen** |
| Dynamic framework XCFramework | Isolates Rust's symbols | Ships every function; needs code signing of the embedded framework; slower launch | ours | n/a | Not now; revisit if a collision is reported |
| Source build through a SwiftPM build plugin | No binary | Needs Rust on every developer's machine | n/a | n/a | Rejected |
| CocoaPods | n/a | Trunk read-only from 2026-12-02 | n/a | n/a | Rejected (bindings note) |

## 5. What we build ourselves, and why

`xtask/src/package_swift.rs` uses lipo, `xcodebuild -create-xcframework`, `ditto` and
`swift package compute-checksum`. These are Apple's own tools, and no Rust packaging tool covers
an XCFramework plus its test run.

## 6. Impact on the plan

- [Phase 5](../../07-planning/phases/phase-5.md) 5.3, done when `apple.yml` is green.
- D-287.
- 5.10 writes the root `Package.swift` with the release URL and checksum.
- 5.5 (Flutter on iOS) can reuse the XCFramework.

## 7. Risks and open items

- **Mac Catalyst and visionOS are not built.** Catalyst (`aarch64-apple-ios-macabi`) is a
  matrix row to add. visionOS is a tier-3 target that needs `build-std` on nightly.
- **The zip is not reproducible** (§3). A fixed time on its entries would make it so.
- **watchOS and tvOS** have no consumer yet.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Xcode | 27.2 (27B52) | 2026-11-14 | `xcodebuild -version` |
| Swift | 6.4.2 | 2026-11-14 | `swift --version` |
| Rust | 1.99.1 (LLVM 23.1.1) | 2026-11-14 | `rustc -vV` (from the nm message) |

## 9. Sources

- [Privacy manifest files](https://developer.apple.com/documentation/bundleresources/privacy-manifest-files), Apple, accessed 2026-11-14.
- [Describing use of required reason API](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api), Apple (not reached 2026-11-14; see §3).
- [Fixing the App Store's missing API declaration for file timestamps](https://ptkd.com/journal/how-to-fix-itms-91053-for-file-timestamp-api), ptkd.com, accessed 2026-11-14.
- [Apple privacy manifest policy](https://docs.unity3d.com/Manual/apple-privacy-manifest-policy.html), Unity, accessed 2026-11-14.
- [Missing API declaration](https://developer.apple.com/forums/thread/750057), Apple Developer Forums, accessed 2026-11-14.
- [Distributing binary frameworks as Swift packages](https://developer.apple.com/documentation/xcode/distributing-binary-frameworks-as-swift-packages), Apple, accessed 2026-11-14.
- [Creating a multiplatform binary framework bundle](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle), Apple, accessed 2026-11-14.
