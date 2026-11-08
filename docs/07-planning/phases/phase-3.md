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
- [ ] Topology stage: `i_overlay` i64 noding, shared arcs, parents dissolved from children,
  Douglas–Peucker levels with a crossing guard (G-topo)
- [ ] ARCS, POLYGONS, POLY_INDEX; the admin REGION on the shared arcs; the A/B and timing
  against D-083

### 3.4 B11 DEM
- [ ] The DEM section (D-089): codec, tile directory, point and bilinear queries; measured
  bytes and decode time against the note's model and against LERC and zstd
- [ ] Elevation at each place's consensus point from the DEM section; surveyed heights for
  summits (D-089; moved from 3.2)

### 3.5 Postal
- [ ] `data/countries/np/postal.yaml`, the POSTAL section (D-090), its queries and the link to
  PLACES

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
