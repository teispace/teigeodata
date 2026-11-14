---
title: "Node.js and WebAssembly: prebuilt addons, one ES module, variants by component"
status: research
updated: 2026-11-14
owner: maintainer's agents
revisit: 2027-02-14
---

# Node.js and WebAssembly: prebuilt addons, one ES module, variants by component

## 1. Question

Phase 5.7 ships the napi-rs addon with per-platform prebuilds and the WebAssembly build "per
feature variant, a size budget per variant (S3), browser, Deno, Bun and Workers"
([phase 5](../../07-planning/phases/phase-5.md)). The [bindings note](bindings-and-mcp.md)
§3.2 and §3.3 chose napi-rs 3 and wasm-bindgen. Open:

- which variants are worth publishing, measured, and how a variant is built from one API;
- which wasm-bindgen output loads in browsers, Deno, Bun, Node.js and Workers;
- how the Node.js package finds its prebuilt addon, and which platforms and Node.js lines it
  supports;
- what each package is tested on.

## 2. Summary and recommendation

- **Variants are sets of pack components** (D-291). `teigeodata-api` has a Cargo feature per
  component; a build without one keeps every function and answers as a layer installed
  without it does. `components()` names what a build has.
- **Two WebAssembly variants:** `full` (every component) and `lookup` (every component but
  `search` and `substring`: places at a point, their names, admin units, zones, heights,
  postal codes). `fold` belongs to `search` (D-292), so `lookup` leaves out its Unicode
  tables. Measured below: `lookup` is 43% smaller compressed. A points-only build has no
  names, which no product wants; it is not published.
- **One ES module for every host:** wasm-bindgen `--target web` with three small entries
  chosen by `package.json` conditions: `workerd` (imports the `.wasm` as a module, the way
  Workers load WebAssembly), `node` (Node.js, Bun and Deno's npm resolution: reads the file
  and instantiates synchronously) and `default` (browsers, bundlers, Deno URLs: `await init()`
  fetches the file beside the module). Every entry exports the same API and `init()`, which
  is a no-op where loading was synchronous.
- **The WebAssembly `Engine` reads packs from bytes** (`openBytes`); a page fetches a pack
  itself. `openFile` and the store (files, threads) are not in the module.
- **Node.js:** the generated napi-rs addon, one npm package per platform as napi-rs lays it
  out (`os`, `cpu`, `libc` fields), named in the main package's `optionalDependencies`; the
  main package's loader picks by `process.platform`, `process.arch` and the C library. Built
  for Node-API 4 (`napi4`), which every supported Node.js line, Bun and Deno provide. The
  oldest Node.js is 22 (20 reached its end of life on 2026-04-30).

Measured 2026-11-14 on this Mac (wasm32-unknown-unknown, the `wasm` profile: `opt-level =
"z"`, fat LTO, one codegen unit, `panic = "abort"`; wasm-bindgen 0.2.130 `--target web`, then
`wasm-opt -Oz` from binaryen 133):

| Build | raw B | gzip -9 B | Brotli 11 B |
|---|---:|---:|---:|
| every component (`full`) | 595,416 | 249,443 | 203,746 |
| every component, `simd128` on | 587,435 | 246,897 | 202,196 |
| every component but `search` and `substring`, without `fold` (`lookup`, D-292) | 369,607 | 140,724 | 116,582 |
| `point` only, with `fold` | 424,795 | 174,872 | 142,931 |
| `point` only, without `fold` | 252,942 | 89,106 | 74,534 |

## 3. Findings

- **The base is most of a small build.** In the points-only module (before `wasm-opt`),
  `twiggy` 0.8.0 shows `.rodata` at 181 KB, nearly all Unicode normalisation tables reached
  only through `fold`, and `bitpack::unpack` at 74 KB, the 33 unrolled widths of the column
  decoder (B1, kept unrolled for speed: about 4 G values/s in WebAssembly, D-074).
- **`simd128` does not shrink the decoder** (1.3%); whether it speeds it up is a benchmark
  question for 5.9, not a size one.
- **`strip = true` removes the `target_features` section** that wasm-bindgen reads to know
  which WebAssembly features are on; it warns. The `wasm` profile keeps it (`strip = false`)
  and `wasm-bindgen --remove-name-section --remove-producers-section` then `wasm-opt` drop
  names and debug sections instead.
- **`-Oz`, `-Os` and `-O3` in `wasm-opt` differ by under 1%** after LTO at `opt-level = "z"`.
- **wasm-bindgen 0.2.130 enables weak references by default** (no `--weak-refs` flag any
  more): a dropped `Engine` is freed by a `FinalizationRegistry`, and `free()` and `using`
  free it at once.
- **Node.js cannot `fetch` a `file:` URL**, so the default `--target web` loader fails there;
  the `node` entry reads the file with `node:fs` and calls `initSync`. Deno and Bun fetch
  `file:` URLs, and resolve npm packages with the `node` condition too.
- **Workers load WebAssembly as an imported module** (`import wasm from "./x.wasm"`), and
  `fetch` cannot reach the bundle's own files; the `workerd` condition is the one their
  bundler resolves. Their only size limit is 64 MiB uncompressed (bindings note §3.14).
- **The query paths call nothing wasm32 lacks.** `SystemTime` and `std::fs` appear only in
  `open_file`, threads only in the store; both are left out of the module.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| wasm-bindgen `--target web` + per-host entries | One module and one glue file for every host; ES modules; no bundler needed | Node.js needs its own entry (no `file:` fetch) | MIT/Apache-2.0 | 0.2.130 | **Chosen** |
| `--target bundler` | Bundlers resolve the `.wasm` import | Browsers without a bundler, Deno and Node.js need other builds | same | same | Rejected |
| `--target module` (source phase imports) | The future single target | ESBuild and Node.js 24 only today (wasm-bindgen guide) | same | same | Revisit 2027 |
| A package per host (`web`, `nodejs`, `deno`) | Each is the tool's default | Three copies of the module per variant | same | same | Rejected |
| napi-rs `wasm32-wasip1-threads` fallback | Same addon API in a WASI runtime | Needs `@napi-rs/wasm-runtime` and threads (cross-origin isolation in browsers); our wasm-bindgen module already serves those hosts | MIT | `@napi-rs/wasm-runtime` 1.2.7 | Not needed |
| Variants by component (`full`, `lookup`) | 43% smaller compressed for apps without search; same API | Two packages | n/a | n/a | **Chosen** |
| One variant | Simplest | Every lookup-only page pays for search's tables | n/a | n/a | Rejected |
| `@napi-rs/cli` 3.10.12 for the Node.js packages | Writes the platform packages and a loader for many platforms | A build-time Node.js dependency; its loader replaces our generated `index.js` | MIT | 3.10.12 | Use its layout and names, not the tool |

## 5. What we build ourselves, and why

- **`wasm.rs.j2`**: the wasm-bindgen shim, generated from the IDL like every other layer
  (records read from plain objects, enums from strings, errors with a `code`, a callback's
  exception rethrown), so the module and the Node.js addon cannot drift.
- **Three entry files of a few lines each** and `cargo xtask package-wasm`, which builds a
  variant, runs wasm-bindgen and wasm-opt, and assembles its package.
- **The platform packages and loader for Node.js** in `cargo xtask package-node`, in the napi-rs
  layout, so no Node.js tool is needed to build.

## 6. Impact on the plan

- D-291 (the API per component), D-292 (`fold` with `search`); a decision for the packaging
  when it is proven.
- [Phase 5](../../07-planning/phases/phase-5.md) 5.7 in sub-steps: the shim and variants, the
  WebAssembly package tested on Node.js, Deno, Bun, a browser and workerd, then the Node.js
  platform packages.
- Size budgets per variant become a gate (Z3 in the [benchmarks](../../06-engineering/benchmarks.md)).

## 7. Risks and open items

- **A panic aborts the module** (`panic = "abort"`; wasm32 has no stable unwinding): the
  instance is unusable after one. The API returns errors instead of panicking (the workspace
  lints deny `unwrap`, `expect`, `panic` and indexing), and the parity suite runs every call.
- **`simd128` speed** is not measured yet (5.9).
- **The browser store** (installing packs over `fetch` into OPFS) is not in this step; a page
  fetches packs itself until it is.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| wasm-bindgen, CLI | 0.2.130 | 2026-11-14 | crates.io |
| js-sys | 0.3.107 | 2026-11-14 | crates.io |
| binaryen (`wasm-opt`) | version_133 | 2026-11-14 | GitHub releases, Homebrew |
| twiggy | 0.8.0 | 2026-11-14 | crates.io |
| napi, napi-derive, napi-build | 3.16.2, 3.7.1, 2.6.0 | 2026-11-14 | crates.io |
| `@napi-rs/cli` | 3.10.12 | 2026-11-14 | npm |
| `@napi-rs/wasm-runtime` | 1.2.7 | 2026-11-14 | npm |
| wasm-pack | 0.15.1 | 2026-11-14 | npm |
| Node.js | 22.23.5, 24.22.3, 26.13.1 (LTS lines) | 2026-11-14 | nodejs.org `index.json` |

## 9. Sources

- wasm-bindgen 0.2.130 guide, `reference/deployment.md` (targets), and `wasm-bindgen --help`,
  read 2026-11-14.
- `@napi-rs/cli` 3.10.12 package docs (`build`, `create-npm-dirs`, `artifacts`,
  `pre-publish`), read 2026-11-14.
- [Node.js releases](https://nodejs.org/dist/index.json), accessed 2026-11-14.
