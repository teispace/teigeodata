---
title: "Phase 3: forge and Nepal data"
status: living
updated: 2026-11-07
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
- [ ] The forge depends on `teigeodata-format` (`write`) and writes a pack from the stages that
  exist: IDMAP (serials in Hilbert order, D-075), PLACES, HIERARCHY, NAMES, SEARCH, PLACE_INDEX
  and the timezone REGION
- [ ] Content-addressed stage cache; `forge build <cc> --main|--lite`
- [ ] Sign, open with the reader, run every section's `check`; G-det: two builds, identical bytes

### 3.2 Places: core and consensus
- [ ] Building readers (OSM, Microsoft, Google Open Buildings v3), tiled and streaming
- [ ] The settlement core and the weighted Weiszfeld consensus (D-091); elevation from the DEM
  at the consensus point, surveyed heights for summits (D-089)
- [ ] A held-out, hand-labelled set of about 300 Nepal places for Q-005 (D-062)

### 3.3 B10 geometry
- [ ] Topology stage: `i_overlay` i64 noding, shared arcs, parents dissolved from children,
  Douglas–Peucker levels with a crossing guard (G-topo)
- [ ] ARCS, POLYGONS, POLY_INDEX; the admin REGION on the shared arcs; the A/B and timing
  against D-083

### 3.4 B11 DEM
- [ ] The DEM section (D-089): codec, tile directory, point and bilinear queries; measured
  bytes and decode time against the note's model and against LERC and zstd

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
