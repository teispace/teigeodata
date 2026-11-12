---
title: "ADR-0040: Section parts: a kind split into several sections"
status: accepted
updated: 2026-11-12
---

# ADR-0040: Section parts: a kind split into several sections

Status: accepted, 2026-11-12. Refines [ADR-0015](ADR-0015-tree-shaking.md) and D-133, D-135.

## Context

D-213 asks that names split by language and outlines by admin level and level of detail, each
its own component a client or a profile picks. Today a component is a set of section kinds and
a pack has at most one section of a kind, so a section is the smallest thing a client can
fetch, verify and install. The [research note](../../02-research/notes/separable-components.md)
compared parts inside the one pack, a pack per part (as `np.dem` is), build-time profiles only,
and byte ranges inside one section.

## Decision

- A section table entry carries a **part** key, a `u32` in four of its eight reserved bytes. A
  kind may appear more than once, one section per part; the table is ordered by kind, then
  part. Part 0 is the **base part**, which is every section as packs have them today, and the
  first section of any kind that is split.
- The key's meaning is **fixed per kind**, so the table describes itself: a language for NAMES,
  SEARCH and SUBSTRING (`*` for the languages without a part of their own); the deepest admin
  level held (`level + 1`) and a level of detail (`<< 8`) for ARCS, `ARCS_OVERVIEW`,
  POLYGONS, POLY_INDEX and `ADMIN_REGION`. Other known kinds take only the base part; a kind
  the reader does not know may have parts.
- **Outline parts are whole copies** (amended with step 3, D-249): the part of level *k* holds
  every outline section over levels 0 to *k*, on only the arcs they use; a reader takes one
  part, and the base holds every level. Arc groups that a reader joins were the first design;
  they were dropped because polygons and the admin index address arcs and vertices globally.
- **Levels of detail are outline parts too** (step 4, D-250): at each published tolerance the
  pack has a part for every level, the deepest included, on simplified, snapped arcs with the
  boxes, labels and admin index rebuilt; the base keeps full resolution. An outline key's low
  byte is at most 31 and its lod at most 6, so it never reads as `*` or a language.
- BLOCK_HASHES' directory names each covered section by kind and part, in the `u32` that was
  reserved there.
- Each part is a section: its own hash in the signed head, its own frames on the wire, its own
  block hashes past 1 MiB. Ranged, partial and delta installs need no new trust path.
- A language gets its own part when its names other than canonical ones reach 128 KiB of text;
  smaller ones share the `*` part, which keeps the head under its 16 KiB limit. (Planned at
  16 KiB, one hash block; measured with step 2, D-248: every part held costs its own lookups, and
  16 KiB gave Nepal 8 parts and twice the search time.)
- Levels of detail are stored as full copies, one part per level, not as refinements.

## Consequences

- A pack without parts is byte for byte what it was: the shipped baseline and every embedded
  layer rebuilt identically after the change (2026-11-12).
- A reader built before this ADR refuses a pack with parts (a non-zero reserved byte), never
  misreads it.
- Each consumer of a split kind (the engine's names, search and outline readers, the forge's
  writers, the catalog's sizes, `tgd install`) learns parts in its own step, in the order of
  the research note: format, names and search by language, outlines by level group, levels of
  detail, then the catalog and the profiles.
- Release assets grow by the duplicated levels of detail; clients download only what they pick.
