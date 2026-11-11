---
title: "Reply to Tithipatra's brief"
status: living
updated: 2026-11-11
---

# Reply to Tithipatra's brief

Tithipatra's maintainer handed over its first brief to teigeodata on 2026-11-11. The brief was
checked against main at `70561f8`. This page is our reply: one row per request (TG), its place
in our plan, its status, and the commit that closes it. Each request is built as a general
feature (P16), never as a case for one consumer. The plan items live in
[phase 4 §4.9b](../../07-planning/phases/phase-4.md#49b-consumer-requests); the decision to take
the brief is D-193.

We accept the brief's split of concerns (teigeodata: place, height, zone outlines, zone rules
and offsets; teistro: civil-time policy, sky and calendars, and the adapter).

## Status

| TG | Request | Where in our plan | Status | Commit | Notes |
|---|---|---|---|---|---|
| TG-01 | Releases a consumer can pin | 4.9b, then Phase 8 | planned | | A `v0.x` SemVer tag of the workspace with `CHANGELOG.md` once TG-02 to TG-04 land. Before 1.0 a minor version may break, as SemVer allows for 0.x; the changelog names each break. Publishing to crates.io needs the maintainer's account, so the first tags are git tags (`cargo` builds from `git` + `tag`) |
| TG-02 | ISO 3166 codes on admin units | 4.9b (format, forge, engine) | researched | this change | Not there yet: packs carry no ISO codes. Codes come from data files with their source (`split.yaml` already maps Nepal's provinces to ISO 3166-2), never derived from names. [Note](../../02-research/notes/iso-3166-codes.md): a small `ISO_CODES` section in the `admin` component and `Engine::iso_code(&place)`, called on each entry `admin_at` and `ancestors` return (`Place` stays small, so points-only builds carry nothing). Nepal's codes are NP-P1 to NP-P7; India's include the 2023 changes (IN-OD, IN-CG, IN-TS, IN-UK) |
| TG-03 | Zone rules (TZif) in the pack, from the outlines' tzdb release | 4.9b, per ADR-0033 | planned | | ADR-0033 already decided "our own TZif bundle built from the matching tzdb release". The section name and the three queries follow the brief; the TZif reader is our own `no_std` code (RFC 9636), so the engine needs no time library |
| TG-04 | A place's extent: `bbox_of`, `boundary(place, lod)` | 4.9b (engine) | done (D-194) | this change | `Engine::bbox_of(&place) -> Option<Rect>` (1e-6° grid, rounded outward) and `Engine::boundary(&place, lod, &mut Outline)` (rings with part and hole flags, level of detail 0 full to 6 coarsest, tolerance in cm). The format already stores each outline's box and arcs at several levels of detail; the engine exposes them |
| TG-05 | Nepal and India releases through sync | Phase 8 data release; W1 | blocked | | Nepal's release waits on maintainer steps: the input mirror (D-127), the signing ceremony, two real releases. India is wave W1 and needs its research note first |
| TG-06 | Search across scripts: substrings of 3+ characters | 4.9b (search) | planned (research first) | | Exact, prefix and fuzzy search across Latin and Devanagari exist (ADR-0032). Substring (infix) matching does not; it needs an index we measure for size first |
| TG-07 | A height says where it came from | 4.9b (format, engine) | planned | | The DEM section stores resolution and datum; it gains the source's identity |
| TG-08 | Zones before 1970 (`backzone`) | 4.9b (research) | researched | this change | [Note](../../02-research/notes/zones-before-1970.md): tzdb 2026f's `backzone` has no South Asian zone (only a commented-out `Asia/Chennai` placeholder), so a backzone build changes no South Asian instant; the default build already carries `Asia/Kolkata`'s Madras time (+05:21:10) and IST since 1906, and the war-time +06:30. No outline source exists for Bombay or Calcutta time (timezone-boundary-builder issue 171 is open), so a pre-1955 Mumbai instant reads `Asia/Kolkata` and we ship nothing invented. Whether the rules are built with `backzone` limited to `zone.tab` is decided with TG-03 |
| TG-09 | Horizon profile from the DEM | [ideas](../ideas.md) | idea | | Scheduled after the DEM ships in releases |

## Questions back

- TG-02: the codes come from a per-entry call, `iso_code(&place)`, not inside `admin_at`'s and
  `ancestors`'s answers. Does a per-entry call meet the request? It keeps `Place` the same size
  for every user.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-11 | First reply: every TG placed in the plan | Tithipatra's brief (D-193) |
