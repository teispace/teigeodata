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
