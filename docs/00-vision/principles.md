---
title: "Principles"
status: accepted
updated: 2026-11-06
---

# Principles

Numbered so decisions can cite them (`P3`).

| # | Principle | What it means in practice |
|---|---|---|
| P1 | **Accuracy before breadth** | Coordinates and elevation are right before names are polished; names before population. A field without a source and a validation rule does not ship. |
| P2 | **Measured, not claimed** | Every accuracy, size and speed claim comes from a gate, with a date. Gates are proven red before they are trusted. |
| P3 | **Offline first** | Every query works with no network. The library never makes a network call unless the app asks it to. |
| P4 | **Pay only for what you use** | Capabilities are Cargo features; data comes in components and levels of detail; native and WASM builds come in variants. Defaults are lean. |
| P5 | **One API, every language** | Same signatures, same behaviour, same answers, proven by the parity gate. Types carry units and ranges into every binding. |
| P6 | **Untrusted input is hostile** | Packs, catalogs and queries are validated before use; no panic crosses the FFI; parsers are fuzzed. |
| P7 | **Stable identity** | A place ID is never reused. Merges leave redirects; removals leave tombstones. Developers can store our IDs forever. |
| P8 | **Provenance everywhere** | Every field knows its source, version and licence. That makes the lite build, attribution and corrections exact. |
| P9 | **Reproducible** | The same pinned inputs give byte-identical packs. Every build can be audited. |
| P10 | **Research, compare, then build** | Every implementation starts with a research note and a comparison, and its lessons flow back into the plan. |
| P11 | **Best algorithms, earned** | We use and invent the best techniques (succinct structures, perfect hashing, FST search, exact region indexes, error-bounded compression), and each one is adopted only after it wins a benchmark. |
| P12 | **Developer experience is a feature** | Integration in minutes, errors that say how to fix them, examples that run in CI, docs generated from one source. |
| P13 | **Contributors are first-class** | Small fixtures, one toolchain, data corrections without Rust, clear guides, fast review. |
| P14 | **Respect for places and people** | Local names in local scripts, official spellings, the Nepal perspective declared openly, and no telemetry. |
| P15 | **Built for the hosted future, without depending on it** | Auth, metering and multi-tenancy stay outside the core, so a hosted tier is an operations task, not a redesign. |
| P16 | **World-general by design; countries are data** | Every schema, algorithm, check and tool works for every country, script and language. Country specifics (admin ladders, unit types, abbreviations, lexicons, transliteration, keyboard layouts, sources) live in per-country data files, never in code. Nepal is the first data release, not a special case. |
