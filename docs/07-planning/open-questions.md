---
title: "Open questions"
status: living
updated: 2026-11-13
---

# Open questions

Questions waiting for research or the maintainer. Each has a recommendation. When answered,
move it to the [decision log](../03-decisions/log.md) and delete it here.

| # | Question | Recommendation | Waiting on |
|---|---|---|---|
| Q-002 | Which lite-build components are possible per country? Still unverified: India LGD/pincode/Census 2011 PCA licences (block lite admin below state), Survey of India 2022-policy terms, AW3D30 terms, whether GHSL and HRSL used OSM (WorldPop R2025A does: main only, D-091) | Admission by ADR-0030; verify each before its wave | [Source licences](../02-research/notes/source-licences.md) |
| Q-003 | Binding strategy confirmed (own C ABI + IDL + generators; napi/wasm-bindgen/PyO3/JNI shims generated from the IDL)? | Research confirms it with five changes (ADR-0016 revised); S4 confirmed the generators, with the IDL taken from a safe API crate (D-066); accept after the result-format spike | [Bindings and MCP](../02-research/notes/bindings-and-mcp.md), [S4](../02-research/benches/s4-bindings.md) |
| Q-009 | Nepal has no open postcode list: will Nepal Post open the GPO table under a stated licence? (D-205: main only until then) | Ask Nepal Post; the lite build takes the list only under an open licence | Nepal Post's answer (request prepared by an agent, sent by the maintainer) |
| Q-016 | Which independent vertical reference can score GEDTM30 (CC BY 4.0), when it was trained on ICESat-2 ATL08 and GEDI, our reference? | Use a source never used in its training: national survey benchmarks or LiDAR where open (USA 3DEP when the rollout reaches it), GNSS on monuments. Until then the lite build uses NASADEM (scored against ATL08 in S6.3) | Research; maintainer for Nepal survey data |
| Q-017 | How are the 301 Nepal places of `tests/golden/np-places.csv` labelled (Q-005, D-062)? Each needs `label_lat`, `label_lon` at the settlement's centre, never from teigeodata output or its inputs; rows marked `second` need a second, independent pass | Delegated to agents (D-209): research a method on imagery whose terms allow deriving a point, then label | Research (3.7) |
| Q-011 | Census 2021 ward-level tables: where, and under what terms? Checked 2026-11-11: `censusnepal.cbs.gov.np` times out from outside Nepal; the NSO data portal (`113.199.192.53:82`, 77 datasets in XLSX and CSV) has none under Population. Terms: the NSO's *Statistics publication, communication and distribution procedure 2082* (§3(ज): statistics as a public good; §17(क): any user who publishes results from its data must cite the NSO as the source) states no other limit on published tables, which makes them attribution-only for the main build | Fetch the ward tables from a machine in Nepal, or ask the NSO for them; main build with the NSO credit; lite only once the terms name redistribution | Maintainer (a fetch from Nepal), or an NSO reply |
| Q-018 | Baseline pack contents and budget: population threshold, simplification tolerance, an 8 MB budget, a smaller variant (countries, timezones, locator)? | Build it, measure with Z1, set the budget from the measurement ([note](../02-research/notes/engine-and-catalog.md)) | Research (4.2) |
| Q-019 | Delta packs (changed records as an extra layer) or section re-download only? | Decide on two consecutive real Nepal releases (sync gate 11); zstd patches save only 3.4% and are dropped ([note](../02-research/notes/download-and-delta.md)) | Two real releases |
| Q-020 | Sync gate 14 (iOS, Android, web downloads) is in M4 but bindings are Phase 5 | Prototype the transports in Phase 4 only to pass gate 14; packaging stays in Phase 5 | Research (4.x) |
| Q-022 | Must importance stay on one world-wide scale so search merges across packs? | Yes, with a check when the second country is built | Research (W1) |
| Q-014 | Which CORS host serves packs to browser apps (ADR-0037)? Deferred (D-212): GitHub Releases only for now | Later, an object store with free egress on its own account or quota, so it never competes with other projects; the settings are in D-212 | Maintainer (when web apps need it) |
| Q-023 | Does Nepal's 2020 official map claim Susta? Natural Earth's Nepal view gives it to Nepal; India's pack must be cut by the project's own claim polygon (D-217) | Check the Survey Department 2020 map when the lite claim area is digitised (D-206) | Research (TG-15) |
| Q-024 | How do we read the Census 2011 town directory (census towns, about 3,892)? LGD lists statutory towns only (measured 2026-11-13: 74 urban villages outside every urban body); data.gov.in's API takes a key, its pages need JavaScript | A data.gov.in API key on the maintainer's account, or another GODL copy; until then the India pack has statutory towns and GeoNames' places of 5,000 or more | Maintainer (account), then TG-15 step 6 |
| Q-027 | Should a name in a language other than the query's weigh less? `Istan` finds India (its Turkish name Hindistan) and Pakistan before Istanbul, `Tor` Turkey (another language's name) before Toronto; the world golden set accepts them 3rd and 5th (measured 2026-11-14) | Research: weigh a name by the search languages (`SearchOptions`), A/B on the three golden sets | Research |
