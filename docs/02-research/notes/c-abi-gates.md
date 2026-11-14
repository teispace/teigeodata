---
title: "The C ABI's gates: compatibility and exported symbols"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# The C ABI's gates: compatibility and exported symbols

## 1. Question

Phase 5.2 needs two more gates on the C ABI, alongside the drift check and the header-against-
cbindgen check that already run (D-065, [bindings §1](../../04-architecture/bindings.md)):

- **ABI compatibility:** does the library still serve a program built against the last
  release's header?
- **Symbol visibility:** does the shared library export only the header's functions?

Each gate must run on every OS we build for and be proven on a planted break.

## 2. Summary and recommendation

- **Compatibility: diff our own declarations.** `cargo xtask abi` compares the declarations in
  `bindings/c/teigeodata.h` with the header at the last `v*` tag. A removed or changed function,
  callback, struct or constant is a break. An addition is not. The report names the lowest
  version the next release may take: with a break that is the next minor before 1.0 (as Cargo
  reads versions) and the next major after it. `--release` fails when the workspace's version
  is lower. This is enough because the header is generated from the IDL and cbindgen checks it
  against the compiled crate. Every struct is `repr(C)` with fixed-width fields, so equal
  declarations mean an equal layout on every target.
- **Symbols: read them with `nm` and compare with the header.** The library must export each
  header function (in the lite build, all but the store's) and nothing else. The one
  exception is a dependency symbol that cannot be hidden (§3.2). It is listed with its reason.
- libabigail is not used. It reads ELF and DWARF, so it works on Linux only, and its Rust
  support is **unverified** (§4).

Measured 2026-11-14 on v0.3.1 to `main`: 0 breaks, 68 additions. The full library exports 62
functions; the lite library exports 38, its size 1.1 MB (arm64, release).

## 3. Findings

### 3.1 What rustc exports from a cdylib

rustc hands the linker an export list: a version script on ELF, `-exported_symbols_list` on
Mach-O. The list holds every `#[no_mangle]` function of the crate **and of its dependencies**
(rustc's `symbol-visibility` test). C objects that build scripts compile stay hidden.

### 3.2 The one leak: blake3 on aarch64

blake3 1.8.7, on aarch64 with NEON, defines `blake3_compress_in_place_portable` as
`#[no_mangle] extern "C"` in `src/ffi_neon.rs`. Its C NEON code calls back into the Rust
portable code through it. So the macOS, iOS, Android and Linux arm64 libraries export it.

- **ld64 won't hide it.** `-Wl,-unexported_symbol,_blake3_*` fails with "-unexported_symbol
  cannot be used with -exported_symbol*" (tried 2026-11-14).
- **GNU ld won't hide it.** It takes one version script, and rustc already supplies one.
  Reports say lld merges several, but rustc's script wins.
- **The blake3 `pure` or `no_neon` features would hide it**, but they give up the NEON
  hashing used to verify packs.
- **It is harmless.** The BLAKE3 C library defines the same function with the same signature
  and the same result. If another copy interposes on it (ELF's flat namespace), the hash does
  not change. Mach-O's two-level namespace binds it within our library anyway.

The gate allows this name, with that reason, and nothing else.

### 3.3 The release baseline

The v0.3.1 header is spike S4's six functions. Those six are unchanged, so nothing has broken
since that release. The CI checkout fetches history and tags with `filter: blob:none`, so the
old header is fetched only when the gate asks for it.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Diff of the generated header's declarations | Every OS; no install; exact for our `repr(C)` fixed-width types; names the next version | Trusts that the header matches the library, which cbindgen checks | ours | built 2026-11-14 | **Chosen** |
| libabigail `abidiff` | Reads the binary itself, including layouts from DWARF | ELF only; needs debug info in the release build; Rust DWARF support **unverified** | Apache-2.0 with LLVM exception | 2.9 (Debian, 2026) | Not now; revisit when Linux packages ship |
| cargo-semver-checks | Mature, rustdoc-based | Checks the Rust API, not the C ABI | Apache-2.0/MIT | n/a | Not for this gate |
| Symbol list via `nm` | Present with every toolchain (llvm-nm, GNU nm) | Windows needs `dumpbin` or `llvm-nm` on the DLL | n/a | llvm-nm (Xcode) | **Chosen** |
| Linker hiding (`-unexported_symbol`, a second version script) | Would remove the leak | Rejected by ld64 next to rustc's list; GNU ld takes one script | n/a | tried | Rejected (§3.2) |

## 5. What we build ourselves, and why

`xtask/src/abi.rs` reuses the header parser that the cbindgen check uses
(`bindings/cheader.rs`). The lite build's expected set comes from the IDL's `feature` marks, so
no function name is hard-coded. Proven 2026-11-14 on planted faults:

- a parameter type changed in the header: reported as a break, minimum 0.4.0;
- `--release` at 0.3.1 with that break: fails;
- the allow list emptied: the blake3 symbol fails;
- `--lite` run on the full library: 24 extra exports fail.

The unit tests cover a removed function, a changed parameter, a grown struct, a changed
constant and a changed callback.

## 6. Impact on the plan

- [Phase 5](../../07-planning/phases/phase-5.md) 5.2: the gates item is done. Packaging
  remains.
- Decision D-285. CI's `bindings` job runs `cargo xtask abi --lib` on the full and the lite
  library.
- The release pipeline (5.10) runs `cargo xtask abi --release`.

## 7. Risks and open items

- **Windows exports are not checked yet.** The packaging step adds a `.dll` check, with
  `llvm-nm` or `dumpbin /exports`.
- **The header declares the store's functions in the lite build too.** A lite caller that uses
  them gets a link error, not a compile error. The packaging step should guard them in the
  header behind a macro.
- **Breaks before 1.0 are allowed with a minor bump.** The gate makes them visible; it does not
  forbid them.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| blake3 | 1.8.7 | 2026-11-14 | Cargo.lock, its `build.rs` and `src/ffi_neon.rs` |
| cbindgen | 0.29.4 | 2026-11-14 | `cbindgen --version` |
| libabigail | 2.9 | 2026-11-14 | Ubuntu manpages (abigail-doc 2.9-2) |

## 9. Sources

- [rustc's symbol-visibility test](https://rust.googlesource.com/rust-lang/rust/+/HEAD/tests/run-make/symbol-visibility/rmake.rs), rust-lang, accessed 2026-11-14.
- [rust-lang/rust#104707](https://github.com/rust-lang/rust/issues/104707), on rustc's version script, accessed 2026-11-14.
- [How to use linker version scripts in Rust 1.54+](https://users.rust-lang.org/t/how-to-use-linker-version-scripts-in-rust-1-54/64033), users.rust-lang.org, accessed 2026-11-14.
- [libabigail(7)](https://man7.org/linux/man-pages/man7/libabigail.7.html), man7.org, accessed 2026-11-14.
- [ABI compatibility testing with libabigail](https://developers.redhat.com/articles/2024/05/20/application-binary-interface-compatibility-testing-libabigail), Red Hat, 2024-05-20.
