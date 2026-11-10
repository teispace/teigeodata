---
title: "Ideas inbox"
status: living
updated: 2026-11-07
---

# Ideas inbox

Ideas not yet planned. Each is researched before it is scheduled.

| Date | Idea | Notes |
|---|---|---|
| 2026-11-06 | Street and address components | Out of v1 scope for now ([scope](../00-vision/scope.md)); revisit after W1 |
| 2026-11-06 | A paid hosted tier (API and MCP, all data) | Design stays ready ([ADR-0025](../03-decisions/adr/ADR-0025-hosted-ready.md)) |
| 2026-11-06 | DSM-to-terrain bias correction with ICESat-2 ground truth | Improves elevation beyond the v1 target |
| 2026-11-06 | A review web app for flagged records | Lets contributors work the QA queue in a browser |
| 2026-11-07 | Land-cover-aware DEM fusion: Copernicus on open ground, NASADEM or a canopy correction under forest (S6.3: the plain mean already gives LE90 17.3 m in Nepal) | Improves elevation in forested hills; fit on the S6 per-point outputs, scored on a held-out part |
| 2026-11-07 | Zero-copy blobs for Python (buffer protocol), the JVM (direct `ByteBuffer` with a cleaner) and Dart (native finalizer), as Swift and Node already have | Removes the one copy per result in those three (S4) |
| 2026-11-07 | Faster strings into the JVM and Node (JNI UTF-16 critical access; one napi call instead of two) | They cost 470 ns and 190 ns per string round trip today (S4); matters for single-name calls |
| 2026-11-07 | Java FFM (Panama) for desktop and server JVMs, JNI kept for Android | FFM upcalls should be cheaper than JNI's 234 ns per callback; measure before deciding |
| 2026-11-07 | A PtrHash-style MPHF next to IDMAP for bulk ID lookups (3–27 ns instead of 82–91 ns, at +0.37 B per place: B5) | Worth it only if a measured workload resolves IDs in bulk, for example a server joining large ID lists |
| 2026-11-07 | Interleave each B1 block's header (width, base, offset) in one array instead of three | A cold `get` touches about four cache lines today; one would do. A/B on B1 before the format freezes |
| 2026-11-07 | A per-key importance maximum column in SEARCH (1 byte per key before packing), read by `best_in` instead of three decodes (starts, first row, importance) | The `search_prefix` count is 1.04 M instructions over about 1,250 keys with random importance, where block maxima skip nothing; real importance is skewed, so measure on Nepal and India before adding about 6% to the section |
| 2026-11-09 | The timezone of a single-zone country from its admin units, not a timezone REGION: `tz_at` is the country's zone wherever ADMIN_REGION answers the country; a multi-zone country stores zones per admin unit and a REGION only for units a zone border splits | The Nepal main pack's timezone REGION is 2.0 MB (17% of 11.75 MB) for one zone inside the country; the neighbours' zones belong to their own packs and the baseline |
| 2026-11-09 | Simplify the finest admin arcs to the source's own accuracy (COD-AB is metres, the arcs keep every vertex: 593,610, 2.66 MB) | ARCS is the largest section; a 1–2 m tolerance should drop many vertices; measure against G-topo and the admin-at agreement first |
| 2026-11-09 | A Wikidata item whose class was abolished (the class has P576, as Nepal's village development committee Q1530705 since 2017) is a former unit, not a current settlement: read the class's dissolution in the places query | 2,716 Nepal VDC items are current populated places; without a stated GeoNames ID they stay apart from the GeoNames former unit (Duhagadhi, Junga Thapachaur, Rampur Khadauna); needs a query change and a re-fetch |
| 2026-11-10 | IDMAP for a pack of many namespaces: order the sorted-ID permutation by namespace runs (each namespace's rows rise with its serials) instead of one FOR over all rows | The world baseline's IDMAP is 187,404 B for 34,410 rows (5.4 B per row, 0.35 B in a country pack), 5% of the file; measure against the current coding first |
| 2026-11-10 | Cities' alternate names in the world baseline, filtered (native scripts and the languages a country uses) | `cities15000` holds 357,970 alternate names (3.5 MB raw, untagged by language); names in other scripts would let users search "काठमाडौं"-style for world cities; A/B bytes against the 8 MB budget |
| 2026-11-10 | `admin_at` for a coastal point just outside the coarse coast: the nearest country within a few hundred metres, flagged as near, not inside | 517 of 34,162 world cities lie outside Natural Earth's coast at full resolution; a fallback tier could still name their country |
