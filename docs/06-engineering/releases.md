---
title: "Releases and versioning"
status: accepted
updated: 2026-11-12
---

# Releases and versioning

How the code is versioned and tagged, so a consumer can pin it (TG-01, D-198). Data releases
(packs, the catalog) have their own cadence and keys:
[catalog keys](catalog-keys.md), [ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md).

## Code

- **One version for the workspace.** Every crate shares `workspace.package.version`; a tag
  `vX.Y.Z` on `main` releases them together. A consumer pins the tag through `git`:

  ```toml
  [dependencies]
  teigeodata-engine = { git = "https://github.com/teispace/teigeodata", tag = "v0.1.0" }
  teigeodata-sync = { git = "https://github.com/teispace/teigeodata", tag = "v0.1.0" }
  ```

  Publishing to crates.io needs the maintainer's account and waits for that; the tags come
  first.
- **SemVer, 0.x rules until 1.0.** Before 1.0.0 a minor bump (`0.1` → `0.2`) may break the
  public API; a patch bump (`0.1.0` → `0.1.1`) never does. From 1.0.0 only a major bump breaks.
- **Stable surface:** the public Rust API of `teigeodata-engine` and `teigeodata-sync`, the
  `tgd` command line and its JSON output. The other crates are public so the bindings and the
  forge can use them; their APIs may change in any minor release before 1.0.
- **The toolchain** is pinned in `rust-toolchain.toml`; the crates build on that version and
  newer (`rust-version`).
- **[CHANGELOG.md](../../CHANGELOG.md)** names every change by area, and every breaking change
  under its own heading with what to change in the calling code. A change that breaks the
  stable surface without a changelog entry is a bug.

## Packs against code

A pack and a reader meet through the [pack format](../04-architecture/pack-format.md), not the
crate version: readers skip unknown section kinds and non-critical tags, and refuse a file
whose `min_reader_major` is above their own. A new section (such as `TZ_RULES`) therefore needs
no new code to keep older packs working, and older readers keep reading newer packs. Packs
built for a release name the reader version they need in the catalog.

## Cutting a release

1. Every gate of the [quality bar](quality-bar.md) is green on `main` in CI.
2. Move the changelog's "Unreleased" section under the new version and date; bump
   `workspace.package.version`, the bindings' version (`bindings/Cargo.toml`,
   `bindings/parity.json`, `bindings/c/parity.c`, `bindings/dart/pubspec.yaml`,
   `bindings/node/package.json`: each parity test checks it against the core's) and the
   forge's path dependencies (`forge/crates/forge/Cargo.toml`); `cargo update -w` in the
   root, `bindings/` and `forge/`; commit
   `chore(repo): release vX.Y.Z`.
3. Tag `vX.Y.Z` (annotated) on that commit and push the tag.
4. Check from a scratch project: `cargo build` with the `git` + `tag` dependency above, then
   open a pack with `Layer::open` and answer a `get`.

## Data

Until the signed catalog is live ([catalog keys](catalog-keys.md): the offline key ceremony
comes first), data is released by hand as a GitHub pre-release (D-247):

1. Build the packs and layers from a commit on `main` whose CI is green.
2. Tag `data-YYYY.MM.DD` on that commit: `gh release create data-YYYY.MM.DD --target <sha>
   --prerelease`; a second release on one day is `data-YYYY.MM.DD.2`, then `.3` (D-261).
3. Attach the `.tgd` files with their summaries and the attribution, renamed where names would
   clash (`np-summary-lite.md`), plus `SHA256SUMS` (`shasum -a 256`) and `B3SUMS` when `b3sum`
   is at hand. Never the `.key-*` file or the TSV reports. Always `ids-<build>.tar.gz`
   (`cargo xtask ids save <build> <file>`): the ID registries the files were numbered with, so
   the next build keeps every published ID (D-261; `cargo xtask ids restore` puts them back).
4. The notes say what each file is (rows, wire size), the code commit or tag that reads it
   (`git` dependency with `rev` or `tag`), the licence with the attribution, that it is outside
   the signed catalog, and how to verify the checksums.
5. **Name no consumer.** Consumer projects may be private: assets, summaries and notes use
   generic names (a profile's layer is published under a name for what it holds, such as
   `world-np-lite.tgd`), and no gate or doc of a consumer is cited.

The first: [data-2026.11.12](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.12). The latest: [data-2026.11.13.2](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.13.2), built from `c4add7b` for readers at `v0.2.0` (India's districts and towns in the layer, CLDR's Nepali country names, and the first `ids-lite.tar.gz`); every checksum verified from a fresh download, and `cargo xtask ids restore` found it and matched all 250 registries.
Once the catalog is live, data releases go through it and these pre-releases stop.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-12 | Data releases as GitHub pre-releases until the catalog is live; the first, `data-2026.11.12` | consumers need the data before the key ceremony (D-247) |
| 2026-11-11 | The bindings' versions are part of a release (they were left at 0.0.0 by v0.1.0) | the parity test failed |
| 2026-11-11 | First version: one workspace version, SemVer with 0.x rules, `git` tags first | TG-01 (D-198) |
