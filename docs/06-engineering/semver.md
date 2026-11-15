---
title: "What 1.0 promises"
status: accepted
updated: 2026-11-15
---

# What 1.0 promises

From v1.0.0, teigeodata follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)
on every surface below. A **major** release may break a surface. A **minor** release only adds
to one. A **patch** release only fixes, and adds nothing a caller could come to depend on.
How releases are cut and tagged is in [releases](releases.md); security fixes and supported
versions are in [`SECURITY.md`](../../SECURITY.md).

Each surface has a gate, so a break is caught before a release, not reported after it.

| Surface | Compatible in a minor | A break, so a major | Gate |
|---|---|---|---|
| Rust API of every published crate | New items, new optional features, new variants of a `#[non_exhaustive]` enum | A removed or renamed item, a changed signature, a new required trait bound, a variant added to an exhaustive enum | `cargo-semver-checks` in `release.yml` against the last tag |
| C ABI (`teigeodata.h`) | New functions, new error codes, new enum constants. A new option is a new record and function, since a record's layout is fixed | A removed or changed function, a field added to or changed in a record, a changed constant | `cargo xtask abi` against the last tag (D-285), [abi.md](../04-architecture/abi.md) |
| Language bindings | What the C ABI and the IDL add, in every language at once | What the ABI breaks, or a renamed method in one language | Each binding's `--check` against the IDL; the parity suites |
| Pack format | New section kinds, manifest tags without the critical bit, extension columns | Anything an older 1.x reader would misread or refuse | [The format freeze](../02-research/notes/format-freeze.md), `tests/compat.rs` |
| Catalog (`schema: 1`) | New fields, which clients ignore | A changed or removed field, or `schema: 2` | `catalog::tests`, the TUF corpus |
| HTTP API (`/v1/…`, `/packs`, `/health`) | New query parameters, new response fields, new endpoints | A removed parameter or field, a changed meaning or status code | `tests/http.rs`, `tests/query.rs` |
| Nominatim-style endpoints (`/search`, `/reverse`, `/lookup`) | What Nominatim adds | Leaving Nominatim's shape | The conformance corpus, `tests/nominatim.rs` (D-314) |
| MCP tools and resources | New tools, new optional arguments, new output fields | A removed or renamed tool, a new required argument, a changed output field | `tests/mcp_http.rs`, the `mcp-conformance` workflow |
| `tgd` command line and its `--json` output | New commands, new flags, new JSON fields | A removed command or flag, a changed exit code or JSON field | `crates/teigeodata-cli/tests` |

## Rules that apply everywhere

- **Adding is never breaking for a careful caller.** A caller must ignore JSON fields it does
  not know, and must handle an error code it does not know. In Rust, every public enum that
  may grow, error codes included, is `#[non_exhaustive]`; the 1.0 API review checks each one
  ([phase 8.4](../07-planning/phases/phase-8.md)). In the bindings, an unknown code maps to the
  generic error.
- **The pack format and the code version are separate.** A 1.x reader reads every 1.x pack,
  every format-0 pack, and every pack of a later 1.x data release. A client refuses to
  download a pack that needs a newer reader (D-336). Data releases (new places, fixed
  coordinates, new countries) are not code releases and change no surface. A data change
  never needs a new code version.
- **Data is not an API.** A place's coordinates, names, population and elevation may change in
  any data release; they are corrected, not versioned. Stable IDs keep their meaning: a retired
  ID redirects to its survivor or becomes a tombstone
  ([ADR-0008](../03-decisions/adr/ADR-0008-stable-ids.md)).
- **Not covered:**
  - each crate's `fuzz` feature, and the modules behind it (`teigeodata_format::fuzz` and the
    rest), which serve the harnesses and change with them;
  - items marked `#[doc(hidden)]`;
  - the forge's crates and its command line (a build tool, not a library);
  - `xtask`;
  - the text of error messages, which may be reworded (their codes and variants are stable);
  - performance figures, which only get better within their budgets;
  - the exact bytes of a pack, which depend on the forge's version and its data.

## Deprecation

An item that is to be removed is deprecated first, in a minor release. In Rust that means
`#[deprecated]` with its replacement. In the IDL it is a deprecation note that each binding
carries as its language's own marker. It is named in `CHANGELOG.md` under **Deprecated**. It
stays for at least that minor release and is removed only in the next major. A deprecation
never changes behaviour.

## MSRV and platforms

- The minimum supported Rust version is `rust-version` in `Cargo.toml` (1.99 today), and the
  `msrv` job builds on it ([quality bar](quality-bar.md)). Raising it is a **minor** release,
  named in the changelog, and never a patch.
- A platform or language runtime leaves support only in a minor release that names it, when its
  vendor has ended support (for example a Python version past its end of life, D-290).

## Before 1.0

Until v1.0.0, a minor release (`0.3` → `0.4`) may break any surface, and a patch never does.
Breaks are listed under **Breaking** in the changelog.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-15 | Written: each surface, what breaks it, its gate; deprecation, MSRV | Phase 8.4, D-336 |
