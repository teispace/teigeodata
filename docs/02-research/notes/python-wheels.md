---
title: "Python: abi3 and abi3t wheels, the blob as a buffer, free threading"
status: accepted
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# Python: abi3 and abi3t wheels, the blob as a buffer, free threading

## 1. Question

Phase 5.6 ships the generated PyO3 module as wheels. The [bindings note](bindings-and-mcp.md),
§3.5, chose two wheel families (abi3 from 3.10, abi3t for free-threaded 3.15), the buffer
protocol for blobs, and `gil_used = false` after a thread-safety review. Open:

- whether the buffer protocol is possible in an abi3 wheel built for 3.10;
- how one crate yields both families, and which wheels cibuildwheel then builds and tests;
- how a free-threaded build is proven to keep the GIL off.

## 2. Summary and recommendation

- **The floor is CPython 3.11, not 3.10.** The buffer protocol (`Py_buffer`,
  `PyBuffer_FillInfo`) entered the limited API in 3.11: PyO3 compiles its buffer support only
  for `any(not(Py_LIMITED_API), Py_3_11)`. CPython 3.10 reached its end of life on
  2026-10-01 (PEP 619, final), so nothing supported is lost.
- **One crate, two families.** `pyo3` features `abi3-py311` and `abi3t-py315`: built by
  Python 3.11 to 3.14 the module is `abi3` (`PyInit__native`); built by 3.15 or later it is
  `abi3t` (PEP 793's `PyModExport__native`), which free-threaded and GIL builds both load.
  maturin 1.16 tags them `cp311-abi3` and `cp315-abi3.abi3t`.
- **Blobs are lent, not copied.** Every blob result is a `teigeodata.Blob`: a frozen PyO3 class
  that owns the Rust bytes and fills a read-only buffer with `PyBuffer_FillInfo` (which holds a
  reference to the object, so the bytes outlive every view). `memoryview(blob)`,
  `bytes(blob)` (a copy), numpy and pyarrow read it in place; writing to a view raises
  `TypeError`. The pure-Python reader is now `teigeodata.blob.Reader` and reads any buffer.
- **Wheels by cibuildwheel 4.3.5,** configured in `bindings/python/pyproject.toml`: cp311
  builds the abi3 wheel, which cibuildwheel reuses and tests on 3.12 to 3.15
  (`find_compatible_wheel`); cp315t builds the abi3t wheel, cp314t a version-specific one.
  Linux (manylinux and musllinux, x86_64 and aarch64), macOS (arm64; x86_64 cross-built,
  tested under Rosetta), Windows (x64, arm64). Each wheel is tested from its installed copy.
- **Free threading is tested, not assumed.** `tests/test_threads.py` runs 8 threads × 2,000
  rounds of search, get, reverse and a lent blob on one engine and compares every answer with
  one thread's; on a free-threaded build it also checks that importing teigeodata left the
  GIL off. A wrong expected value fails it.

Measured 2026-11-14 on this Mac (M-series):

| Measure | Result |
|---|---|
| `sample_rows(1000)` (37 KB): lent vs with a copy | 17.2 vs 17.8 µs (3.3% less) |
| `sample_rows(1000000)` (40 MB): lent vs with a copy | 23.4 vs 24.6 ms (4.8% less), and no second 40 MB |
| Wheels (macOS arm64, `dist` profile) | `cp311-abi3` 1,897 KB; `cp315-abi3.abi3t` 1,907 KB |
| Parity plus threads from installed wheels | pass on 3.11 and 3.14 (abi3), 3.15 (abi3t) |
| Threads test, 8 × 2,000 rounds, 3.15 | 0.7 s |

## 3. Findings

- **A module built without `PYO3_BUILD_EXTENSION_MODULE` links the build interpreter's
  libpython.** Loaded into another version it brings a second runtime: a 3.14-built module
  crashed 3.11 and 3.15 in `_Py_Dealloc` during import, and a 3.11-built one crashed 3.14.
  maturin sets the variable; plain `cargo build` did not. The repository's
  `.cargo/config.toml` now sets it for every build (only PyO3 reads it), and `otool -L` shows
  no Python framework after.
- **An abi3t module cannot be loaded by 3.11 to 3.14** ("does not define module export
  function"), as PEP 793 intends: the abi3 wheel serves them.
- **Read-only buffers raise `TypeError` on write**, not `BufferError`.
- **`collections.abc.Buffer` is 3.12.** The stub imports it by version (type checkers carry
  `typing_extensions`); `blob.py` imports it only for annotations.
- **maturin adds a CycloneDX SBOM** to the wheel (`dist-info/sboms/`).
- **abi3audit 0.0.26 does not know PEP 793's `PyModExport_<module>`** and fails the abi3t
  wheel on it under `--strict` (CI, 2026-11-14). The cp315t audit is
  `bindings/python/tools/abi3t_audit.py`: abi3audit's report, that one symbol allowed,
  anything else a failure. Drop it once abi3audit knows abi3t.
- **cibuildwheel targets macOS 10.9 by default; Rust's x86_64 floor is 10.12**, so delocate
  refused the x86_64 wheel. `MACOSX_DEPLOYMENT_TARGET = "10.12"` (arm64 starts at 11.0).
- **PEP 639 forbids `..` in `license-files`**, so the workflow copies `LICENSE` and `NOTICE`
  into the package folder (gitignored) and checks every wheel holds both.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| abi3 3.11 + abi3t 3.15 + cp314t | Three wheels per platform cover 3.11 onward, GIL and free-threaded; zero-copy blobs | 3.10 dropped (end of life) | n/a | PyO3 0.30.1, maturin 1.16.0 | **Chosen** |
| abi3 3.10 + abi3t 3.15 | Keeps 3.10 | No buffer protocol in abi3: every blob copied | n/a | n/a | Rejected |
| Version-specific wheels per Python | Full C API | A wheel per version per platform | n/a | n/a | Rejected |
| cibuildwheel | Builds and tests in each Python, reuses abi3 wheels, manylinux and musllinux, runs abi3audit | Slower than one maturin build | BSD-2 | 4.3.5 | **Chosen** |
| maturin-action alone | Fast | Tests per version are ours to script | MIT/Apache | v1.51.0 | Not needed |

## 5. What we build ourselves, and why

The `Blob` class in the generated module (about 40 lines in `python.rs.j2`, the only `unsafe`
being the documented `PyBuffer_FillInfo` call) and the threads test. Packaging is maturin and
cibuildwheel, configured in one file.

## 6. Impact on the plan

- [Phase 5](../../07-planning/phases/phase-5.md) 5.6: built; done when `python.yml` is green
  on every platform, the free-threaded tests included.
- D-290. The bindings note §3.5's 3.10 floor is superseded.
- 5.10 publishes the wheels to PyPI with trusted publishing.

## 7. Risks and open items

- **cibuildwheel does not reuse abi3t wheels yet** (its `find_compatible_wheel` knows abi3 and
  `none`): 3.16t will build its own until it does.
- **musllinux and Windows arm64** run in CI only.
- **The x86_64 macOS wheel's tests need Rosetta** on the runner.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| PyO3 | 0.30.1 | 2026-11-14 | crates.io |
| maturin | 1.16.0 | 2026-11-14 | crates.io, PyPI |
| cibuildwheel | 4.3.5 (f9b966c) | 2026-11-14 | PyPI, GitHub |
| CPython | 3.11.18, 3.14.10, 3.15.2 | 2026-11-14 | local, endoflife.date |
| rustup | 1.29.3 | 2026-11-14 | `rustup --version` |

## 9. Sources

- PyO3 0.30.1 guide, "Building and distribution" (`abi3`, `abi3t`), and `src/buffer.rs`, read 2026-11-14.
- cibuildwheel v4.3.5 `docs/options.md`, `docs/faq.md` and `cibuildwheel/util/packaging.py`, read 2026-11-14.
- [PEP 619](https://peps.python.org/pep-0619/) (3.10 schedule) and [PEP 803](https://peps.python.org/pep-0803/) (abi3t), peps.python.org, accessed 2026-11-14.
- [Python end-of-life dates](https://endoflife.date/python), accessed 2026-11-14.
