---
title: "Releases and versioning"
status: accepted
updated: 2026-11-11
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
   `workspace.package.version`; commit `chore(repo): release vX.Y.Z`.
3. Tag `vX.Y.Z` (annotated) on that commit and push the tag.
4. Check from a scratch project: `cargo build` with the `git` + `tag` dependency above, then
   open a pack with `Layer::open` and answer a `get`.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-11 | First version: one workspace version, SemVer with 0.x rules, `git` tags first | TG-01 (D-198) |
