---
title: "Phase 3: forge and Nepal data"
status: living
updated: 2026-11-09
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
- [x] B11 on all 45 Nepal tiles (D-109): 139.3 MB for the 9° × 5° box, 1.907 bits per sample,
  all probes within 1 m, 163 s
- [ ] B11 on 2–3 tiles elsewhere, and against LERC and zstd
- [ ] The DEM section in the Nepal packs: its own component, tiles masked to the country
- [x] Elevation at each place's consensus point from the DEM section into PLACES column 7
  (D-109, D-111): `forge make` runs the recipe's `dem` tileset as a stage, `build` samples it
  at every place's final point and checks every row on read back. Nepal main: 101,542 of
  101,550 places (with D-110's admin places), PLACES +129,872 B (10.7 bits per place) on the
  earlier 96,847; against GeoNames' SRTM3 field
  P90 130 m at the place's point, 26 m at the record's own point. Lite has none (Copernicus
  is main only, D-031)
- [ ] Elevation in the lite build: a NASADEM section (CC0, EGM96 heights to EGM2008, 3601 ×
  3601 tiles that `dem-section` does not take yet), or Copernicus once Q-008 clears it
- [ ] Surveyed heights for summits (D-089; moved from 3.2)
- [ ] Review the consensus point where it moves a hill settlement far from its records: the
  height there differs from GeoNames' SRTM3 by more than 100 m for 14,597 records (P90 130 m,
  26 m at the records' own points); review with the consensus gate

### 3.5 Postal
- [x] `data/countries/np/postal.yaml`, the POSTAL section (D-090, D-105), its queries and the
  link to PLACES: 753 local-level and 6,743 ward codes in the Nepal main pack, checked against
  the GPO table on every build; 10.1 KB, exact lookup 155 ns, place → codes 179 ns; the world's
  5.0 M codes within 3.1% of the model ([benchmark](../../02-research/benches/phase-3.md#postal))
- [ ] Nepal's codes in the lite build: wait for a stated licence (Q-009) or derive them from
  the CBS codes
- [ ] Point → postal code in the SDK: ADMIN_REGION's unit, then its records (with the engine)

### 3.6 Names and corrections
- [x] Display names from the lexicon and a romaniser (D-106): rule tables as data
  (`data/romanise/ne-Deva.yaml`), 58.7% exact on 6,660 OSM name pairs against ICU's 22.0%;
  Devanagari display names 6,269 → 0 where a Latin name exists, 175 romanised; respellings
  reviewed through `naming-<build>.tsv` ([note](../../02-research/notes/names-canonical.md))
- [x] The aliases rules in the build (D-060, D-107): a record's names only when corroborated,
  no name that names an admin unit or another place nearby; Nepal main 40 records and 3,312
  names not taken, each listed with what it names in `dropped-names-<build>.tsv`
- [x] Admin records of the point sources (D-110): linked to their COD-AB unit, former units
  kept flagged under their successor, divisions as localities; golden pass 0.754 → 0.871,
  historical 0.068 → 0.886 (`pack-eval` on the built pack)
- [x] Duplicate clusters (D-116): measured; the 1,872 same-name collisions within 2 km are
  mostly different hamlets with generic names (only 15 of 5,176 same-name pairs with building
  cores come within 100 m). Same-name settlements within 100 m after the core are merged:
  50 in Nepal main, listed in `merged-<build>.tsv`. G-conf (stated-link recall and precision)
  moves to the CI gates in 3.7
- [x] Former units: stated links join a settlement and its former unit; former units rank
  below settlements (D-118): 2,856 twins merged, golden recall@1 0.801, pass 0.887
- [ ] Rank a town over its same-name twins (Damak's two GeoNames records 0.8 km apart; a
  municipality and its seat town): the golden misses that rank the wanted place second.
  Find the queries with `S3_MISSES=first` on `pack-eval`
- [x] Corrections as data with guards (D-117): five ops, retire/apply/stale per build,
  G-corr in the build, `forge correction-guard`, schema checked by `check-data`; Nepal's first
  4 (Newa-script names, Q20588832 same as Patan), unreviewed until the maintainer signs them;
  golden pass 0.879. Also: GeoNames `iata`/`abbr` rows as short names (KTM; abbreviation
  recall@1 0.927 → 0.951)
- [ ] The maintainer reviews `data/countries/np/corrections/` (`review.by: unreviewed`)
- [ ] `not-same-as` (a cannot-link in conflation) when the first case needs it

### 3.7 Gates, lite and accuracy
- [ ] G-det, G-lic, G-topo, G-pip, G-conf, G-corr, G-budget in CI
- [ ] Attribution in every pack (D-031): the sources' notices, including Copernicus now that
  main packs carry its heights
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
| 2026-11-09 | 3.6: display names and romanisation built (D-106); the aliases rules split into their own item | 6.4% of places showed Devanagari; the build carries every member's names without D-060's rules |
| 2026-11-09 | 3.6: D-060's alias rules built into the build (D-107) | the build carried every member's names; S3 chose the fold exemption (recall@1 0.957 against 0.949) |
| 2026-11-09 | 3.6: admin records linked, former units kept (D-110) | `pack-eval` showed 94 golden queries with no answer in the pack, all GeoNames admin records |
| 2026-11-09 | 3.4: place elevations built (D-111); lite elevation and summit heights made their own items | Copernicus is main only; the lite DEM needs NASADEM in `dem-section` |
| 2026-11-09 | 3.4/3.6/3.7: the admin-record item closed by D-110; consensus-height review and pack attribution added | elevations merged with D-110; main packs now carry Copernicus-derived heights |
| 2026-11-09 | 3.6: duplicate clusters measured and merged within 100 m (D-116); ranking of same-name twins its own item | most collisions are different hamlets; the golden misses are ranking |
| 2026-11-09 | 3.6: corrections built (D-117); review and `not-same-as` their own items | the ADR's design, measured on the golden misses |
| 2026-11-09 | 3.6: former units in conflation and ranking (D-118) | found while ranking same-name twins |
