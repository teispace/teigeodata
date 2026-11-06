---
title: "Vision"
status: accepted
updated: 2026-11-06
---

# Vision

**teigeodata is the place data developers can trust and carry anywhere.** It gives accurate
coordinates, elevation, the full administrative hierarchy down to the lowest level, boundaries,
local names, the IANA timezone, postal codes and population, cleaned and deduplicated from
the best open sources. It ships as small, signed, offline packs that load at compile time or at
runtime, with one API in every major language.

## The problem

- **The common open gazetteer is not trustworthy at local level.** GeoNames, the usual default,
  has duplicates, misspellings, coordinates rounded to arc-minutes (up to ~1.8 km off), stale
  hierarchies (Nepal's 2017 federal restructuring is only partly reflected), and patchy
  elevation and population.
- **The good data is scattered.** OpenStreetMap, Wikidata, official boundaries, censuses,
  elevation models and building footprints each hold part of the truth, under different
  licences, schemas and identifiers.
- **The tools are heavy.** A Postgres gazetteer passes 5 GB before indexes; a planet geocoder
  needs hundreds of GB. Mobile and offline apps get neither.
- **Search fails on local names.** काठमाडौं, Kathmandu, Kathamandu and Kantipur are the same
  place; most tools find only one of them.

## What we deliver

1. **Accuracy first.** Every record has a consensus position with an uncertainty radius and an
   elevation with an uncertainty, checked against elevation models and neighbours. Accuracy
   is measured and published per country, per release.
2. **Depth.** Nepal to ward and settlement level, India to village and town, and the rest of
   the world by population threshold, with the same model everywhere.
3. **Everything a developer needs, nothing rough.** Points, polygons at several levels of
   detail, hierarchy, local names, search, reverse geocoding, exact IANA timezone, postal codes
   (current and former), elevation lookup. Each is optional and tree-shakable, and none ships
   without a source and a gate.
4. **Offline and dynamic.** Bundle a region at build time, download more at runtime ("Dubai"),
   merge them live, update them by delta, verify everything.
5. **One API everywhere.** Rust, C, C++, Node, browsers and edge runtimes through WASM, Dart and
   Flutter, Python, Java and Kotlin, Android, Swift, .NET and Go. Same signatures, same
   answers, proven by a parity gate. An MCP server from day one.
6. **Small and fast.** Compact columnar packs, memory-mapped and zero-copy. Lookups are timed
   in nanoseconds to microseconds and gated against regressions.
7. **Open.** Apache-2.0 code; ODbL data with full credits, plus a CC-BY lite build;
   reproducible builds; contributor-friendly corrections with evidence.

## Who it is for

App developers (mobile, web, backend) who need places, addresses, boundaries or timezones;
data and GIS engineers who need a clean gazetteer; AI agents through MCP; and the people of
the regions covered, who deserve their places spelled and placed correctly.

## What success looks like

- For Nepal: the most accurate open gazetteer that exists, with published P90 errors.
- A developer adds the package, gets Nepal offline in a few MB, and lets their users download
  any other region, in under an hour of integration work.
- Contributors fix a place with evidence in a pull request, and the fix ships in the next data
  release.
