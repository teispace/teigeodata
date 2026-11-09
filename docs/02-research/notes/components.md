---
title: "Components: download only the data asked for"
status: research
updated: 2026-11-09
owner: format, sync
revisit: 2027-05-01
---

# Components: download only the data asked for

## 1. Question

Consumers need very different slices of a pack. One app wants latitude and longitude only. A
second also wants altitude, a third the timezone, and a fourth reverse geocoding. How do they
download, store and load exactly that slice, verified, with no wasted bytes and one code path?
This feeds phase 4 item 4.1b, [ADR-0015](../../03-decisions/adr/ADR-0015-tree-shaking.md) and
D-133.

## 2. Summary and recommendation

- **The section is the unit of download.** Each section belongs to exactly one component, and
  a component is a set of sections. A client fetches the frames of the sections its components
  need. The transport already plans this (`Frames::plan_sections`, D-112). Each section is
  verified by its hash in the signed head, so no new trust path is needed. The pack stays one
  signed file per address, and a client may hold any subset of its sections.
- **Split what is mixed today.** ELEVATION moves out of PLACES into its own section. Timezone
  and admin each get a per-place column (a few KB), so the region indexes (1.6 MB and 3.2 MB on
  Nepal) are needed only to look up an arbitrary coordinate.
- **Code follows data.** Each component is a Cargo feature, and a binary without the feature
  carries none of its readers.

Nepal main, wire bytes (zstd -19 in 1 MiB frames, 2026-11-09): lat/long 447 KB; with IDs and
the spatial index 504 KB; with altitude 633 KB; the whole pack 8.2 MB.

## 3. Findings

Measured with `cargo xtask check-pack <pack> --dump <dir>` (each section and column as a
file), then `zstd -19` per 1 MiB chunk, the transport's largest frame.

Sections, Nepal 2026.11.0 main (100,358 places):

| Section | Raw | Wire | Wire/raw | Today needed for |
|---|---:|---:|---:|---|
| ARCS | 2,659,828 | 2,462,415 | 93% | boundaries; admin at any point (ADMIN_REGION runs on the arcs) |
| REGION (timezones) | 2,007,656 | 1,606,314 | 80% | timezone at any point |
| SEARCH | 2,487,196 | 1,596,603 | 64% | name search |
| NAMES | 2,199,036 | 905,782 | 41% | names |
| ADMIN_REGION | 1,066,180 | 719,233 | 67% | admin at any point |
| PLACES | 673,884 | 593,044 | 88% | every query |
| HIERARCHY | 384,820 | 117,818 | 31% | parents and children |
| ARCS_OVERVIEW | 94,780 | 83,364 | 88% | coarse boundaries |
| PLACE_INDEX | 56,160 | 53,693 | 96% | nearest, box, reverse |
| IDMAP | 35,424 | 2,951 | 8% | ID → row |
| other (POLYGONS, POLY_INDEX, POSTAL, BLOCK_HASHES) | 60,268 | 55,705 | | |
| all | 11,725,232 | 8,196,922 | 70% | |

Lite: 8,818,084 raw, 6,074,939 wire (no REGION, no POSTAL).

PLACES by column (main):

| Column | Raw | Wire |
|---|---:|---:|
| LAT | 226,580 | 223,681 |
| LON | 226,564 | 223,845 |
| ELEVATION | 134,340 | 129,273 |
| KIND | 24,148 | 7,189 |
| ADMIN_LEVEL | 43,284 | 6,501 |
| FLAGS | 18,916 | 4,161 |

- The coordinates are already at their entropy: zstd saves 1% on 2.23 B per coordinate.
- Elevation is 22% of the wire bytes of PLACES. Every lat/long-only consumer pays it today.
- The region indexes answer "which unit holds this coordinate". For the places themselves the
  answer is fixed at build time and costs a few bits per place: Nepal has 6 timezone areas
  (3 bits per place, about 38 KB raw before compression, less after), and the admin chain is
  already in HIERARCHY.
- The security model fits: the signed head holds every section's hash and BLAKE3 block hashes
  per 16 KiB (D-078), and "a ranged client never downloads the whole target"
  ([download and delta](download-and-delta.md) §3).

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A: one file per component | whole-file installs; simple storage | rows must align across files (a dependency hash per component); more catalog entries; the same head repeated per file | reject |
| B: one signed file; sections chosen per component, fetched by frame | one trust path, one catalog entry; the planner exists; adding a component later fetches only its frames; same path as repair and section updates | elevation sits inside PLACES and must move out; storage must hold a pack with sections missing | **use** |
| C: B at column level (ranges inside a section, verified by block hashes) | finest grain without changing sections | columns must be block-aligned; partial sections complicate every reader; more frames | reject: B gets the same bytes once ELEVATION is its own section |

## 5. What we build ourselves, and why

1. **Format:** an ELEVATION section (one column, out of PLACES), and per-place timezone and
   admin columns, as COMPONENTS fields or their own sections (measure both). Each section kind
   is mapped to its component in one table in `teigeodata-format`, the single source for the
   forge, the planner, the engine and the bindings.
2. **Storage:** a pack is stored as its head plus the sections installed. The file is sparse,
   or an equivalent layout on platforms without sparse files, and the head is unchanged so its
   signature still verifies. A sidecar records which sections are present and verified.
3. **Engine:** the layer opens with any subset of sections. A query that needs a missing
   component returns `MissingComponent(name)`, never a wrong or empty answer.
4. **Sync:** `Request::region("np").components(&[Point, Elevation])` plans the frames of those
   sections. Adding a component later fetches only its frames.
5. **Gates:**
   - Per-component wire sizes, reported by `check-pack`.
   - Points-only Nepal: at most 0.55 MB on the wire.
   - The sum of all components equals the whole pack.
   - Each component, alone and with others, answers its queries identically to the full pack
     (the metamorphic gate, extended).

## 6. Impact on the plan

- Phase 4 item 4.1b becomes field-level components on option B.
- ADR-0015's "components chosen at build or download time" holds, now with the unit stated.
- The schema's component table ([schema](../../05-data/schema.md#components)) is the list; `core`
  is split into `point` (coordinates, IDs, spatial index) and the rest.

## 7. Risks and open items

- Sparse files: APFS, ext4 and NTFS support them. The browser's OPFS and Android's scoped storage
  do not guarantee holes (**unverified**); the fallback stores sections at their offsets in a
  compact file with an offset map.
- Moving ELEVATION out of PLACES changes the format (reader version bump, before v1, so no
  compatibility cost).
- Search needs NAMES (it returns rows and the names to show). A component's dependencies must be
  explicit in the table.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| zstd CLI | 1.5.7 | 2026-11-09 | local |

## 9. Sources

- [ADR-0015](../../03-decisions/adr/ADR-0015-tree-shaking.md), [download and delta](download-and-delta.md),
  [offline loading and trust](offline-loading-and-trust.md), [pack format](../../04-architecture/pack-format.md).
