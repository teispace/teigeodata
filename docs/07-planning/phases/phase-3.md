---
title: "Phase 3: forge and Nepal data"
status: living
updated: 2026-11-08
---

# Phase 3: forge and Nepal data

**Goal:** the forge builds Nepal's main and lite packs from pinned sources, reproducibly, with
every section and every QA gate, and publishes how accurate they are. Exit: milestone **M3**
([roadmap](../roadmap.md)).

**Research:**
- [forge pipeline](../../02-research/notes/forge-pipeline.md): the DAG, readers, conflation,
  geofix, topology, determinism, corrections and the gates;
- [geometry and topology](../../02-research/notes/geometry-topology.md): ARCS, POLYGONS,
  POLY_INDEX and the topology stage (D-088, B10);
- [DEM section](../../02-research/notes/dem-section.md): the elevation codec (D-089, B11);
- [postal](../../02-research/notes/postal.md): schemes, sources and POSTAL (D-090);
- [settlement core](../../02-research/notes/settlement-core.md): buildings, the core and the
  consensus point (D-091);
- the measured baselines of [S5](../../02-research/benches/s5-nepal.md) and
  [S6](../../02-research/benches/s6-elevation.md).

Order (D-092): an end-to-end pack first, so every later stage lands in a pack that is read back
and tested in place. Every stage is streaming and bounded in memory, keyed by the content hash
of its inputs, and deterministic; nothing in it names Nepal (P16).

## Steps

### 3.0 Research
- [x] The four notes above; D-088 to D-092; Q-001 answered as far as primary sources go (D-090)

### 3.1 Vertical slice
- [x] The forge depends on `teigeodata-format` (`write`) and writes a pack from the stages that
  exist: IDMAP (serials in Hilbert order, D-075), PLACES, HIERARCHY, NAMES, SEARCH, PLACE_INDEX
  and the timezone REGION
- [x] The ID registry: stable IDs across releases and between main and lite, with redirects
  (D-094, [forge](../../04-architecture/forge.md#ids-across-releases))
- [x] Content-addressed stage cache: `forge make <cc> [--lite]` from `build.yaml` (D-095,
  [forge](../../04-architecture/forge.md#run-every-stage-make))
- [x] Sign, open with the reader, run every section's `check`; G-det: two builds, identical bytes
  ([forge build](../../04-architecture/forge.md#build-a-pack-build))

### 3.2 Places: core and consensus
- [x] Building readers (OSM, Microsoft, Google Open Buildings v3), tiled and streaming (D-096,
  [forge](../../04-architecture/forge.md#buildings-buildings))
- [x] The settlement core and the weighted Weiszfeld consensus (D-091, D-096): agreement gate
  P90 146 m against 180 m, fatal on failure, proved on a planted mismatch
  ([forge](../../04-architecture/forge.md#settlement-core-and-consensus-core))
- [ ] Elevation from the DEM at the consensus point, surveyed heights for summits (D-089):
  moved to 3.4, which builds the DEM section the elevation is read from
- [x] The labelling sample: 301 Nepal places in `tests/golden/np-places.csv`, drawn by
  `forge labels sample` from GeoNames only, scored by `forge labels score` (D-062, D-096)
- [ ] The labels themselves: maintainer task (Q-005); the core parameters are fitted on them

### 3.3 B10 geometry
- [x] Topology stage: shared arcs, parents dissolved from children, Douglas–Peucker levels with
  a crossing guard (G-topo), verified on the written arcs (D-098,
  [forge](../../04-architecture/forge.md#topology-topology)); a source that is not noded is
  snap rounded first (D-103, [note](../../02-research/notes/snap-rounding.md)); sliver and gap
  reassignment joins with the first source that needs it
- [x] ARCS and POLYGONS sections, in the Nepal packs (D-097)
- [x] POLY_INDEX on the tree shared with PLACE_INDEX, in the Nepal packs; leaf order and node
  size A/B (D-099, [B10](../../02-research/benches/phase-3.md#b10-poly_index-and-ring-reads))
- [x] The admin REGION on the shared arcs (ADMIN_REGION, D-100): 977 KB against 3.63 MB with
  its own vertices, same answers and speed
  ([B10](../../02-research/benches/phase-3.md#admin-region-on-the-shared-arcs))
- [x] A fine grid over the regions' extent for REGION and ADMIN_REGION (D-101): Nepal admin
  queries on places 296 → 153 ns for 89 KB
  ([B10](../../02-research/benches/phase-3.md#a-fine-grid-over-the-packs-extent))
- [x] Coarse levels of detail without decoding every vertex: ARCS_OVERVIEW from level 4
  (D-102), 95 KB on Nepal, levels 4 to 6 read 3.3 to 3.5 times faster
  ([B10](../../02-research/benches/phase-3.md#coarse-reads-from-an-overview)); the
  whole-world render is measured with the world data release
- [x] The India measurement of ARCS, POLYGONS, the overview and ADMIN_REGION: 3.39 B per
  vertex, overview 7.7% of ARCS, ADMIN_REGION 2.11 MB at 51–67 ns, after snap rounding
  (D-103) ([B10](../../02-research/benches/phase-3.md#india-adm2-geoboundaries))

### 3.4 B11 DEM
- [x] The DEM section (D-089, D-104): codec, tile directory, point and bilinear queries, the
  forge's `dem-section` stage, a fuzz case; on four tiles equal to the note's model (1.11 to
  2.94 bits per sample), 13.6 ns per sample, 31 µs per cold query
  ([B11](../../02-research/benches/phase-3.md#b11-dem))
- [ ] B11 on all 45 Nepal tiles and 2–3 tiles elsewhere (download of 1.84 GB waiting for the
  maintainer's approval), and against LERC and zstd
- [ ] The DEM section in the Nepal packs: its own component, tiles masked to the country
- [ ] Elevation at each place's consensus point from the DEM section; surveyed heights for
  summits (D-089; moved from 3.2)

### 3.5 Postal
- [x] `data/countries/np/postal.yaml`, the POSTAL section (D-090, D-105), its queries and the
  link to PLACES: 753 local-level and 6,743 ward codes in the Nepal main pack, checked against
  the GPO table on every build; 10.1 KB, exact lookup 155 ns, place → codes 179 ns; the world's
  5.0 M codes within 3.1% of the model ([benchmark](../../02-research/benches/phase-3.md#postal))
- [ ] Nepal's codes in the lite build: wait for a stated licence (Q-009) or derive them from
  the CBS codes
- [ ] Point → postal code in the SDK: ADMIN_REGION's unit, then its records (with the engine)

### 3.6 Names and corrections
- [ ] Canonical spellings from the lexicon, transliteration, the aliases stage (D-060)
- [ ] Corrections as data with guards (apply, retire, stale); G-corr

### 3.7 Gates, lite and accuracy
- [ ] G-det, G-lic, G-topo, G-pip, G-conf, G-corr, G-budget in CI
- [ ] The lite build from clean-room sources only
- [ ] `ACCURACY.md` for Nepal against held-out truth

### 3.8 Exit review
- [ ] M3 criteria met; progress updated

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | Phase 3 planned; vertical slice first | the four research notes (D-088 to D-092) |
| 2026-11-07 | 3.1: `forge build` writes, signs and reads back the Nepal pack; the cache and ID registry stay open | the slice ran end to end first |
| 2026-11-07 | 3.1: the ID registry added as its own item (D-094) | IDs must hold across releases before any pack is published |
| 2026-11-07 | 3.1 done: `forge make` with keyed stages and stamped outputs (D-095) | the slice runs from one command |
| 2026-11-08 | 3.2: readers, core and consensus built (D-096); DEM elevation moved to 3.4; labelling is a maintainer task | elevation needs the DEM section; labels must come from people, not the forge |
| 2026-11-08 | 3.3: ARCS, POLYGONS and the topology stage built (D-097, D-098); noding by `i_overlay` waits for a source that needs it | COD-AB is noded; the stage refuses a source that is not |
| 2026-11-08 | 3.3: POLY_INDEX built (D-099); a coarse-LOD read item added | B10 measured coarse reads costing as much as full ones |
| 2026-11-08 | 3.3: ADMIN_REGION on ARCS built (D-100); a per-pack top grid item added | B10: dense admin borders make the world top grid the limit |
| 2026-11-08 | 3.3: the per-pack top grid built as a fine grid beside the world one (D-101) | Keeping the world quadtree and directory makes the window an accelerator that cannot change answers |
| 2026-11-08 | 3.3: coarse reads through ARCS_OVERVIEW (D-102); the India measurement made its own item | B10's A/B: per-block bounds cannot skip cumulative deltas |
| 2026-11-08 | 3.3: snap rounding in the topology stage (D-103); India measured | India is the first source that is not noded (4 edge pairs) |
| 2026-11-08 | 3.4: the DEM section built (D-104); the 45-tile measurement and the pack integration made their own items | the codec matches the model on four tiles; the full Nepal set needs a 1.84 GB download |
| 2026-11-09 | 3.5: POSTAL built on place runs (D-105); the lite build's Nepal codes and the point query made their own items | runs cut Nepal from 39.6 to 10.1 KB; the GPO table states no licence |
