---
title: "Embedded layer: the most place data in the fewest bytes"
status: research
updated: 2026-11-11
owner: teigeodata maintainers
revisit: 2027-05-11
---

# Embedded layer: the most place data in the fewest bytes

## 1. Question

Tithipatra's addendum ([consumer page](../../01-product/consumers/tithipatra.md), D-213) asks for
one embedded file that works offline from the first launch: the world's countries,
first-level units and every city of 5,000 or more; Nepal in full; India's towns; each place
with a Nepali and an English name, a height, a zone and its areas. The budget is 15 MB on the
wire, with 10 MB the target. The maintainer's bar is higher than the consumer's: as much
accurate data as possible, in the fewest bytes, and fetched on demand. What goes in the file,
what is built on the device, and what downloads later?

## 2. Summary and recommendation

- **Ship sources, derive indexes.** An index that is a pure function of shipped data is built on
  the device on first use and kept on disk, never downloaded. SUBSTRING is the first: rebuilt
  from NAMES and SEARCH's importance it is **byte for byte the shipped section**, in 0.38 s for
  Nepal's 95,726 places on the development Mac (§3). That saves 1.82 MB of Nepal's 8.43 MB.
  SEARCH (1.31 MB) and the point-to-unit indexes (REGION and ADMIN_REGION, 1.09 MB) are
  candidates next; each is measured the same way before it moves.
- **Every index reaches a device one of three ways**, chosen per profile (D-213): embedded,
  an add-on component download (`search` and `substring` are separate components today), or
  derived on the device. What a first-launch feature needs (prefix search) is embedded or
  derived; the rest (SUBSTRING) is derived or downloaded when wanted.
- **Everything separable** (D-213): names split by language, outlines by admin level and level
  of detail, each its own component, so a client or a profile picks exactly the parts it
  wants and the catalog states each one's wire size.
- **Outlines at the tolerance the questions need** (TG-16): Nepal's local levels at 20 m keep
  21% of the vertices, and at 100 m 7%; the country border stays exact (the Nepal perspective,
  ADR-0006).
- **Names cut to the profile's languages** (TG-14): canonical plus Nepali and English is 61% of
  NAMES' wire bytes for Nepal; the other languages download with the pack.
- **Nepali names for every place** are the largest data gap: 9.1% of Nepal's places (main) and
  0.7% of world cities carry a Devanagari name today ([sizes](../../05-data/sizes.md)). The
  fallback chain: official name, Wikidata `ne`, Wikidata `hi` in India, then a tested
  Latin-to-Devanagari transliteration marked as such.
- **Projected tier 1** with these four: Nepal about 2 MB, the world part with cities of 5,000
  about 3–4 MB, India's towns under 1 MB: 6–7 MB against the 10 MB target, before
  dictionary-trained name compression (TG-17).

## 3. Findings

Measured on 2026-11-11 with `cargo xtask pack-sizes` and the ignored test
`breakdown::derive::derive_substring_from_names` (release build, Apple silicon, machine load
about 25 from other projects):

| Step | Nepal lite |
|---|--:|
| fold every name and collect the word-suffix keys (136,794 keys) | 267 ms |
| encode the SUBSTRING section | 114 ms |
| derived section equals the shipped one | yes, 2,231,188 B |

The keys are gathered in a `BTreeMap<String, BTreeSet<u32>>`, one allocation per key and row;
a sorted vector of (key, row) pairs is the obvious next step and is expected to halve the time.
A phone-class core is about 2–3 times slower: about 1 s, once, in the background after the
first launch. Until it is built, prefix search answers and a substring query waits for the
index.

As built (D-218): `Store::derive` plus the engine's `derive::substring`, end to end on Nepal
lite, derived and installed in 0.50 s with the file equal to the shipped pack byte for byte.

## 4. Options compared

| Option | Wire | First search | Verdict |
|---|---|---|---|
| a. Ship every index (today) | 8.43 MB for Nepal | instant | the fallback |
| b. Derive pure-function indexes on the device, cache on disk | −1.82 MB now (SUBSTRING), up to −4.2 MB with SEARCH and REGION | substring after about 1 s once | **chosen**, index by index, each measured |
| c. Drop the indexes and scan | −4.2 MB | 100k names scanned per keystroke: misses the 16 ms frame (TG-17) | rejected |

## 5. What we build ourselves, and why

A derive step in the engine (feature `derive`, needs `alloc`): given NAMES and the importance
column, it writes SUBSTRING into the store beside the pack, verified by hash against the
manifest's expected digest (so a derived index is as trusted as a downloaded one). The forge
already builds the same bytes; the code is shared, not copied (one code path, §11).

## 6. Impact on the plan

- Phase 4 §4.9b: TG-10 to TG-19 placed; TG-11 done (`pack-sizes`, the sizes page).
- New items: derive SUBSTRING on the device; Nepali names for every place (TG-14) with a
  transliteration research note; the profile-built embedded layer (TG-12).

## 7. Risks and open items

- A derived index must equal the forge's byte for byte, or the manifest digest check fails
  safe (rebuild or download). The digest makes drift visible rather than silent.
- First-launch CPU on low-end phones: measured on the phone-class bench (TG-17) before release.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| zstd (C, wire measure) | 0.14.3 crate | 2026-11-11 | xtask/Cargo.toml |

## 9. Sources

- [Tithipatra's addendum 1](../../01-product/consumers/tithipatra.md), 2026-11-11.
- [Sizes](../../05-data/sizes.md), measured 2026-11-11.
