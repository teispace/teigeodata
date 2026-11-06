---
title: "Glossary"
status: living
updated: 2026-11-06
---

# Glossary

| Term | Meaning |
|---|---|
| **pack** | A `.tgd` file: self-describing, signed data for one coverage area and one or more components |
| **pack address** | The deterministic name of a pack derived from its coverage and component, e.g. `ae-du.places` |
| **component** | An optional group of columns (names, boundaries at a level of detail, population, postal…) that can be included or left out |
| **LOD** | Level of detail of a boundary: `lod0` (coarse) to `full` |
| **baseline** | The small dataset compiled into the library so every query answers offline |
| **pack path** | The ordered list of places the engine looks for packs |
| **layer** | One loaded pack in the engine's pack set |
| **snapshot** | An immutable view of the pack set; swapped atomically when packs change |
| **tombstone** | A record that an ID was removed |
| **redirect** | A record that an ID was merged into another |
| **region index** | The exact point → region structure used for timezone, admin and perspective lookups |
| **perspective** | A view of disputed boundaries (default: Nepal's official) |
| **consensus point** | The position chosen for a place from several sources, with an uncertainty radius |
| **forge** | The data pipeline that turns sources into packs |
| **main build** | The ODbL data build, using every source |
| **lite build** | The CC-BY 4.0 data build, clean-room, with no ODbL or share-alike inputs |
| **provenance** | Per field: source, source version, licence class, and the rule that chose the value |
| **lexicon** | The sourced list of generic place-name terms and their canonical spellings (चोक → Chowk) |
| **phonetic key** | A script-independent key that groups spelling variants of a name for search |
| **IDL** | The API description extracted from the boundary crate, from which bindings are generated |
| **parity gate** | The check that every binding gives identical answers to the same queries |
| **data release** | A CalVer-versioned set of packs, e.g. `2026.12` |
| **palika** | A Nepali local level (metropolitan city, sub-metropolitan city, municipality or rural municipality) |
| **tole** | A Nepali neighbourhood or settlement below ward level |
