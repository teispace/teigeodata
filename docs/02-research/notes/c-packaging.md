---
title: "Packaging the C library: names, pkg-config, CMake, archives"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# Packaging the C library: names, pkg-config, CMake, archives

## 1. Question

How does the C ABI ship (phase 5.2, last item)? The plan asks for shared and static libraries
per platform, the header, pkg-config and CMake files, on GitHub Releases. Three things decide
the result:

- what the library files are called;
- the name each file records for the programs that link it;
- how a C or C++ build finds the library.

## 2. Summary and recommendation

- **A packaging crate, `teigeodata-c`, with `[lib] name = "teigeodata"`.** It re-exports
  `teigeodata-ffi`. rustc exports a dependency's `no_mangle` functions (the
  [ABI gates note](c-abi-gates.md), §3.1), so it gives `libteigeodata.so`, `.dylib` and `.a`,
  or `teigeodata.dll` with its import library, all built directly. Renaming the files after
  the build would break the DLL's import library (it names the DLL) and the dylib's install
  name.
- **Its `build.rs` sets the name each library records.**
  - Apple: `@rpath/libteigeodata.dylib`. rustc's default was the absolute build path
    `…/target/release/deps/libteigeodata_ffi.dylib`, measured 2026-11-14.
  - ELF: the soname `libteigeodata.so.<abi>`, where `<abi>` is `0.<minor>` before 1.0 and
    `<major>` after. It changes exactly when `cargo xtask abi` reports a break.
  - Android: an unversioned soname, because it loads libraries by file name.
- **`cargo xtask package-c`** builds with the `dist` profile, stages a relocatable tree and
  archives it: `.tar.gz`, or `.zip` for Windows. The tree holds:
  - the header, whose lite form sets `TEIGEODATA_FEATURE_STORE` to 0;
  - the shared and static libraries;
  - a pkg-config file using `${pcfiledir}`;
  - a CMake package with `teigeodata::teigeodata` (shared) and `teigeodata::static`, and a
    version file under which only the same minor is compatible before 1.0;
  - LICENSE and NOTICE.
- **Three checks run on the staged files.** The exports gate. The library's own recorded
  name, read with `otool -D` or `readelf -d`, because a wrong one would still pass a smoke
  test run from the build tree. And `--smoke`: `bindings/c/parity.c` is built and run through
  pkg-config (shared and static) and through CMake (both targets).
- **The workflow `c-package.yml` runs on six targets**, full and lite. On a `v*` tag it runs
  `cargo xtask abi --release` and attaches the archives and a SHA256SUMS file to the release.

Measured 2026-11-14 on aarch64-apple-darwin (`dist`: fat LTO):

| Package | Archive |
|---|---|
| Full | 4,968 KB |
| Lite | 2,325 KB |

Each runs all four smoke builds.

## 3. Findings

### 3.1 Names

- **macOS.** The install name rustc gives a cdylib is its build path. A program linked to it
  looks there and fails on any other machine. `-Wl,-install_name,@rpath/…` fixes this, and
  CMake's `IMPORTED_SONAME` matches it.
- **Windows.** The import library records the DLL's name, so the DLL keeps the name it was
  linked with.
- **A clash checked.** The CLI's binary is `tgd`, so a library named `teigeodata` clashes with
  no output, `.pdb` files included.

### 3.2 pkg-config

`prefix=${pcfiledir}/../..` makes the `.pc` file relocatable. Both pkg-config and pkgconf
define `pcfiledir`. GStreamer's packaging moved to it from `--define-prefix`, which depends on
which tool runs.

One difference: with `PKG_CONFIG_SYSROOT_DIR` set, pkg-config prefixes the sysroot to
`pcfiledir` and pkgconf does not. That matters only for cross builds into a sysroot.

`Libs.private` carries rustc's `native-static-libs` (macOS: `-liconv -lSystem -lc -lm`), which
static linking needs.

### 3.3 CMake

A config package with imported targets is what `find_package(teigeodata CONFIG)` expects.
Paths are taken from `CMAKE_CURRENT_LIST_DIR`, so the tree can be unpacked anywhere.

The static target's `INTERFACE_LINK_LIBRARIES` is the native list without MSVC's
`/defaultlib:` entries, which name the C runtime CMake links already.

### 3.4 What the smoke test caught

The first lite run failed to link the store's functions. `#include "teigeodata.h"` had
resolved beside `parity.c` in `bindings/c/`, the repository's header, not the staged one. So
the smoke test now copies `parity.c` into a folder of its own. Without that, the full package
had been tested against the wrong header.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| A packaging crate named `teigeodata` | Right file names from the linker on every OS; the DLL and import library agree | One more crate (two lines of code) | ours | built 2026-11-14 | **Chosen** |
| Rename the ffi crate's library to `teigeodata` | No new crate | Would collide with a future `teigeodata` Rust library crate; the tests' link names change | ours | n/a | Rejected |
| Rename files after the build (`install_name_tool`, `patchelf`) | No crate | Breaks the Windows import library; needs `patchelf` | n/a | n/a | Rejected |
| pkg-config with `${pcfiledir}` | Relocatable; pkg-config and pkgconf | Sysroot difference (§3.2) | n/a | pkgconf 3.0.10 | **Chosen** |
| CMake config package | `find_package` with imported targets | n/a | n/a | CMake 4.4/4.5 | **Chosen** |
| vcpkg and Conan ports | Package managers | Need the released archives first | n/a | n/a | Later, after the first release |

## 5. What we build ourselves, and why

`xtask/src/package_c.rs` (one command; no packaging tool fits a Rust library that ships C
files), and `crates/teigeodata-c`. Proven 2026-11-14 on planted faults:

- **An install name set to `@loader_path/…`:** the own-name check fails.
- **The repository's header used instead of the lite one:** the lite smoke fails to link
  (§3.4).

The exports gate's proofs are in the [ABI gates note](c-abi-gates.md).

## 6. Impact on the plan

- [Phase 5](../../07-planning/phases/phase-5.md) 5.2's packaging item. It is done when
  `c-package.yml` is green on its six targets.
- D-286.
- Swift (5.3) and Dart (5.5) package this library.

## 7. Risks and open items

- **Two Rust static libraries in one program** both define the Rust standard library's
  symbols, and the link fails. This is a known limitation of Rust staticlibs. The shared
  library is the default; the static one is for a program with no other Rust library.
- **Windows exports are not checked** ([ABI gates note](c-abi-gates.md), §7).
- **musl builds and other BSDs** are not built. A data-driven matrix row adds them.
- **The Linux libraries need glibc 2.35 or later**, because they are built on Ubuntu 22.04.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| CMake | 4.5.0 local; 4.4.1 the latest announced | 2026-11-14 | `cmake --version`; kitware.com |
| pkgconf | 3.0.10 | 2026-11-14 | Homebrew |
| actions/download-artifact | v8.0.2 | 2026-11-14 | GitHub API |
| actions/upload-artifact | v7.0.2 | 2026-11-14 | GitHub API |

## 9. Sources

- [Bug 62018: document `${pcfiledir}`](https://bugs.freedesktop.org/show_bug.cgi?id=62018), freedesktop.org, accessed 2026-11-14.
- [cerbero: make pkg-config files relocatable](https://gitlab.freedesktop.org/gstreamer/cerbero/-/merge_requests/1049), GStreamer, accessed 2026-11-14.
- [pkg-config issue 52: sysroot and pcfiledir](https://gitlab.freedesktop.org/pkg-config/pkg-config/-/issues/52), freedesktop.org, accessed 2026-11-14.
- [CMake 4.4.1 available](https://www.kitware.com/cmake-4-4-1-available-for-download/), Kitware, 2026-07-30.
- [MSYS2 pkg-config notes](https://www.msys2.org/docs/pkgconfig/), MSYS2, accessed 2026-11-14.
