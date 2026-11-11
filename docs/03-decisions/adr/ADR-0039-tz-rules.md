---
title: "ADR-0039: Zone rules as pack columns, read by our own reader"
status: accepted
updated: 2026-11-11
---

# ADR-0039: Zone rules as pack columns, read by our own reader

Status: accepted, 2026-11-11. Refines [ADR-0033](ADR-0033-timezone-data.md).

## Context

ADR-0033 decided that the rules come from our own TZif bundle built from the same tzdb release
as the zone outlines, "loaded through `jiff`". Consumer request TG-03 asks for the rules inside
the pack with three queries: the offset at an instant, the offsets of a civil time (one, a gap
or an overlap), and the tzdb version. The [research note](../../02-research/notes/tz-rules.md)
compared raw TZif blobs, our own columns, and `jiff` or `tz-rs` as the run-time reader.

## Decision

- The rules travel as a section of their own, `TZ_RULES` (kind 22), in a component of their
  own, `tz-rules`, which needs nothing. `timezone` and `timezone-index` need it: a zone without
  its rules is a broken answer, and the rules are a few kB.
- The section stores **integer columns, not TZif bytes**: transitions (minutes since a base and
  a second), local time types, a sorted abbreviation table, each POSIX TZ footer parsed into
  integers, and the zone names with the PLACES row of each zone's area. Identical files share
  one rule set.
- The reader is **our own `no_std` code** in `teigeodata-format`: no allocation, no panic, all
  arithmetic checked or bounded, no text parsing at run time. `jiff` is a **dev-dependency
  only**, for the differential test.
- The forge compiles tzdb with `zic -b slim`, `zic` itself built from the pinned tzcode of the
  same release, and parses the output with a strict build-side TZif and TZ-string parser that
  lives in the format crate's `write` feature, where the round trip and the fuzz harness reach
  it.
- The section is lossless: `Zone::tzif` (feature `alloc`) rebuilds the file `zic` wrote, byte
  for byte.
- The manifest tag `tzdb` (`0x0009`) names the release; it is present exactly when the pack has
  `TZ_RULES`.

## Consequences

- The engine answers offsets without a time library and without `alloc` on the query path.
- A new section spec and a fuzz case; the forge gains a C compile of `zic` (system `cc`).
- Gate G-tzrules: every timezone area of a pack has rules, and the release equals the pin.
- Small packs pay a fixed cost per column (a partly filled 128-value block costs as much as a
  full one): 2.5 kB for three zones, against 0.8 kB of TZif. The world's 444 outline zones
  take 93 kB, against 202 kB of distinct TZif.

## Alternatives considered

Raw TZif blobs with a name index (a hostile-input text parser on every query, 2× the installed
bytes for the world); `jiff` or `tz-rs` as the reader (an allocating dependency in a `no_std`
crate); a crate's bundled tzdb (stale, rejected in ADR-0033).

## Evidence

[Zone rules in the pack](../../02-research/notes/tz-rules.md), 2026-11-11: every one of the 598
names of tzdb 2026f rebuilds byte for byte and agrees with `jiff` 0.2.42 at 805,684 instants.
