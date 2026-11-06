---
title: "Open questions"
status: living
updated: 2026-11-06
---

# Open questions

Questions waiting for research or the maintainer. Each has a recommendation. When answered,
move it to the [decision log](../03-decisions/log.md) and delete it here.

| # | Question | Recommendation | Waiting on |
|---|---|---|---|
| Q-001 | Nepal's current postal code scheme. Research: press reports a move from 5 to 7 digits with the PITS upgrade; the new code may be the CBS local-level code plus a 2-digit ward (unverified); the GPO's PDF list (modified 2026-10-14) did not download in full; no machine-readable list or licence | Obtain the GPO list (manual download), confirm the structure, ask about licence; store both schemes with validity periods | [Nepal sources](../02-research/notes/nepal-sources.md) |
| Q-002 | Which lite-build components are possible per country? Still unverified: India LGD/pincode/Census 2011 PCA licences (block lite admin below state), Survey of India 2022-policy terms, AW3D30 terms, whether WorldPop/GHSL/HRSL used OSM | Admission by ADR-0030; verify each before its wave | [Source licences](../02-research/notes/source-licences.md) |
| Q-003 | Binding strategy confirmed (own C ABI + IDL + generators; napi/wasm-bindgen/PyO3/JNI shims generated from the IDL)? | Research confirms it with five changes (ADR-0016 revised); accept after spike S4 and the result-format spike | [Bindings and MCP](../02-research/notes/bindings-and-mcp.md) |
| Q-004 | Conduct and security contact addresses for GOVERNANCE.md and SECURITY.md | A Teispace address the maintainer chooses; until then, GitHub private vulnerability reporting | Maintainer |
| Q-005 | Horizontal accuracy targets per feature class | Set from spike S5 measurements | Spike S5 |
| Q-006 | Pack hosting: GitHub Releases only, or a mirror (for example Cloudflare R2) as well | GitHub Releases first; the mirror stays pluggable | Phase 4 |
| Q-007 | Does Teispace control the domain `teispace.com` (needed to verify the `com.teispace` Maven namespace)? Reserve npm `@teispace`, PyPI/pub.dev `teigeodata`, NuGet prefix `Teispace.*`, JSR scope | Verify the domain by DNS TXT; reserve names before the first release | Maintainer |
| Q-008 | Legal review of the Copernicus DEM terms for the CC-BY lite build | Until reviewed, lite uses NASADEM (CC0) | Maintainer (legal) |
| Q-009 | Nepal has no open postcode or ward-boundary source of its own: confirm with Nepal Post and the Survey Department whether one can be opened | Ask both; until then use HDX COD-AB (CC BY-IGO) wards and sourced postcode lists | Nepal sources research |
| Q-010 | Does COD-AB Nepal v02 geometry include the 2020 claim area (Kalapani, Lipulekh, Limpiyadhura)? Its area field (147,653 km²) suggests so | Download once (26 MB) and check in spike S5 | Spike S5 |
| Q-011 | Census 2021 ward-level tables: format and licence (NSO sites refused automated access) | Manual check by the maintainer or a later fetch | Maintainer |
| Q-013 | Zone precedence where source polygons overlap (S2 found Shanghai/Urumqi, Hebron/Jerusalem, Juba/Khartoum, Tbilisi/Moscow and Kathmandu/Kolkata overlaps in timezone-boundary-builder 2026f) | Return every zone; the single answer follows our admin layer under the Nepal perspective, else the source's documented preference, else the lowest zone ID; list each overlap in the release notes | Phase 2 |
| Q-012 | Can the CC-BY lite build be *tested* against ODbL boundaries without contaminating it? | Treat tests as non-ingest only if the result never feeds back; get a legal opinion | Maintainer (legal) |
