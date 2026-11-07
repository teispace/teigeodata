---
title: "Ideas inbox"
status: living
updated: 2026-11-06
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
