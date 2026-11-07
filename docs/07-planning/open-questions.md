---
title: "Open questions"
status: living
updated: 2026-11-07
---

# Open questions

Questions waiting for research or the maintainer. Each has a recommendation. When answered,
move it to the [decision log](../03-decisions/log.md) and delete it here.

| # | Question | Recommendation | Waiting on |
|---|---|---|---|
| Q-002 | Which lite-build components are possible per country? Still unverified: India LGD/pincode/Census 2011 PCA licences (block lite admin below state), Survey of India 2022-policy terms, AW3D30 terms, whether GHSL and HRSL used OSM (WorldPop R2025A does: main only, D-091) | Admission by ADR-0030; verify each before its wave | [Source licences](../02-research/notes/source-licences.md) |
| Q-003 | Binding strategy confirmed (own C ABI + IDL + generators; napi/wasm-bindgen/PyO3/JNI shims generated from the IDL)? | Research confirms it with five changes (ADR-0016 revised); S4 confirmed the generators, with the IDL taken from a safe API crate (D-066); accept after the result-format spike | [Bindings and MCP](../02-research/notes/bindings-and-mcp.md), [S4](../02-research/benches/s4-bindings.md) |
| Q-004 | Conduct and security contact addresses for GOVERNANCE.md and SECURITY.md | A Teispace address the maintainer chooses; until then, GitHub private vulnerability reporting | Maintainer |
| Q-007 | Does Teispace control the domain `teispace.com` (needed to verify the `com.teispace` Maven namespace)? Reserve npm `@teispace`, PyPI/pub.dev `teigeodata`, NuGet prefix `Teispace.*`, JSR scope | Verify the domain by DNS TXT; reserve names before the first release | Maintainer |
| Q-008 | Legal review of the Copernicus DEM terms for the CC-BY lite build | Until reviewed, lite uses NASADEM (CC0) | Maintainer (legal) |
| Q-009 | Nepal has no open postcode or ward-boundary source of its own: confirm with Nepal Post and the Survey Department whether one can be opened | Ask both; until then use HDX COD-AB (CC BY-IGO) wards and sourced postcode lists | Nepal sources research; also the GPO "UPU Postal Code English" PDF and the India Post pincode CSV, which refuse automated download (D-090) |
| Q-015 | Where does the **lite** (CC-BY) build get the Nepal-perspective claim polygon? COD-AB excludes it (D-054) and the OSM disputed relation is ODbL | Digitise it clean-room from the Survey Department's 2020 map once its licence is confirmed; until then lite marks the area as disputed and assigns no admin unit there | Maintainer (licence) |
| Q-016 | Which independent vertical reference can score GEDTM30 (CC BY 4.0), when it was trained on ICESat-2 ATL08 and GEDI, our reference? | Use a source never used in its training: national survey benchmarks or LiDAR where open (USA 3DEP when the rollout reaches it), GNSS on monuments. Until then the lite build uses NASADEM (scored against ATL08 in S6.3) | Research; maintainer for Nepal survey data |
| Q-011 | Census 2021 ward-level tables: format and licence (NSO sites refused automated access) | Manual check by the maintainer or a later fetch | Maintainer |
| Q-014 | Which CORS mirror serves packs to browser apps (ADR-0037)? | An object store with free egress and a custom domain; needs an account | Maintainer (account) |
| Q-012 | Can the CC-BY lite build be *tested* against ODbL boundaries without contaminating it? | Treat tests as non-ingest only if the result never feeds back; get a legal opinion | Maintainer (legal) |
