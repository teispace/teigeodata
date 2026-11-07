---
title: "Phase 1: building the core types and the .tgd container"
status: research
updated: 2026-11-07
owner: maintainer's agent
revisit: 2027-02-01
---

# Phase 1: building the core types and the .tgd container

## 1. Question

Phase 1 turns the [pack-format draft](../../04-architecture/pack-format.md) into code. The
[format note](format-compression-indexing.md) settled *what* a pack holds. This note settles how it
is built:

- the crate split;
- how the reader views bytes without `unsafe`;
- the manifest encoding;
- the stable ID and its text form (left open by the [schema](../../05-data/schema.md));
- reproducible bytes;
- the fuzz and benchmark harnesses;
- the order of work.

It feeds [phase 1](../../07-planning/phases/phase-1.md).

## 2. Summary and recommendation

- **Two runtime crates, as the overview planned:**
  - `teigeodata-core` is `no_std` with no dependencies: IDs, grid coordinates, Hilbert order and
    the codecs.
  - `teigeodata-format` is `no_std` + `alloc`: the container reader, and the writer behind a
    `write` feature. Its dependencies are `zerocopy` and `blake3`; `ed25519-dalek` sits behind a
    `signature` feature.
  - Codecs need no crypto, and the container needs no codec, so each can be tree-shaken alone.
- **Byte views through `zerocopy` 0.8 derives** on structs made only of
  `zerocopy::byteorder::little_endian` fields. The layout is checked at compile time, reads are
  unaligned and endian-correct, and the crate keeps `#![forbid(unsafe_code)]`. B13 measures the
  cost against unchecked access.
- **The manifest is our own canonical TLV:**
  - an entry is a tag (u16), a length (u32) and a value;
  - tags are strictly increasing, so there is one encoding per manifest;
  - unknown tags are skipped, and a tag's high bit marks "must understand";
  - nested values (column descriptors) are TLV inside a value.
  - `minicbor` 2.3 is more general than we need, and has more parser surface to fuzz.
- **Stable ID:** a u64 made of a 16-bit allocation namespace and a 48-bit serial.
  - The namespace is the country of allocation (`np`, `in`), computed from the two letters, with
    0 for no country.
  - The text form is `tgd:<cc>/<serial>`, with the serial in lower-case Crockford base32 and no
    leading zeros: `tgd:np/3kq`. It round-trips from the u64 alone.
  - **Kind leaves the ID:** a place's kind changes (a village becomes a municipality) and its ID
    may not (D-070).
- **Reproducible bytes:**
  - `created_unix` comes from `SOURCE_DATE_EPOCH`, else 0;
  - nothing else depends on time, thread count or hash-map order;
  - a test builds every pack twice and compares the bytes.
- **Fuzzing:** **bolero** 0.13.
  - One harness is a property test under `cargo test` on stable, which runs random inputs.
  - The same harness runs as libFuzzer under `cargo bolero test` in a nightly CI job.
  - `cargo-fuzz` would need a second copy of every harness.
- **Benchmarks:**
  - **criterion** 0.8 for wall-clock time, local and CI;
  - **gungraun** 0.20 for instruction counts, CI on Linux only, because it needs Valgrind;
  - both are dev-only and live in `teigeodata-bench`, as [benchmarks](../../06-engineering/benchmarks.md) plans.
- **M1's "every section kind":**
  - every kind of the table has its number, its frame, its validation and a round trip through
    the writer and reader;
  - the codec-level kinds (IDMAP, PLACES, HIERARCHY, NAMES, COMPONENTS, BLOBS, BLOCK_HASHES,
    REDIRECTS) carry real payloads in Phase 1;
  - the index payloads (SEARCH, PLACE_INDEX, POLY_INDEX, REGION, POSTAL) are Phase 2's, ARCS and
    POLYGONS come with B10, and DEM comes with B11.
  - The frame is fixed here, so a later payload never changes the container.
- **A missing kind:** ADR-0008 says packs carry redirects and tombstones; the draft table had no
  section for them. Kind 16, REDIRECTS, is added: a retired ID maps to its survivor, or to
  nothing for a tombstone.

## 3. Findings

### 3.1 Reading bytes without `unsafe`

- `zerocopy` 0.8.63 (2026-10-13):
  - derives `FromBytes`, `IntoBytes`, `KnownLayout`, `Immutable` and `Unaligned`;
  - `Ref::from_prefix` returns a typed view and the rest of the slice, or an error;
  - with `little_endian::{U16, U32, U64}` fields, a struct has alignment 1, so a view never fails
    on alignment and reads the same on a big-endian host;
  - the derive checks at compile time that the struct has no padding.
- That gives a 128-byte header struct whose size is asserted at compile time, and a
  section-table slice view `Ref<&[u8], [Entry]>` in one bounds-checked call.
- **The alternative** is hand-written `u32::from_le_bytes(buf[o..o+4])` parsing. That needs no
  dependency, but every offset is written by hand twice, once in the writer and once in the
  reader, which is where format bugs hide. `zerocopy`'s derive is compile-time only; its runtime
  part is small. **Unverified:** the library-size difference, which Z2 measures.

### 3.2 The manifest

- Needs:
  - a few dozen fields;
  - read once at open;
  - one canonical encoding, because it is signed and hashed;
  - forward compatibility, so a newer builder can add fields;
  - no general parser in the runtime (the format note).
- **Own TLV:**
  - about 100 lines;
  - canonical by construction, with sorted unique tags and minimal lengths;
  - fuzzed alone.
  - The high bit of the tag (`0x8000`) means "a reader that does not know this tag must refuse
    the pack". This is PNG's critical-chunk idea, so a future field that changes meaning cannot be
    ignored silently.
- **CBOR (`minicbor` 2.3.1, 2026-10-18):**
  - standard, and a reader in every language;
  - but canonical CBOR (RFC 8949 §4.2) has to be enforced by us anyway;
  - and the decoder accepts far more shapes than we use. Our bindings read the manifest through
    the Rust core, so another language never needs to parse it.

### 3.3 The stable ID

- **Who's On First** IDs are opaque integers from a central allocator ("brooklyn integers").
  **Overture** GERS IDs are 128-bit UUIDs. **GeoNames** IDs are sequential integers.
  **OSM** IDs are per-type sequences: they are not stable across edits of type, and a node and a
  way can share a number.
- **Our builds are sharded by country**, so a central sequence would make every country build
  wait on one counter. A namespace per country lets each build allocate on its own, with no
  collision, and the registry stays per country.
- **The layout:**
  - `namespace = (c1 − 'a') × 26 + (c2 − 'a') + 1` gives 1 to 676 for any two lower-case letters,
    so no table is needed and new ISO codes such as `xk` work;
  - 0 is "no country" (oceans, the world);
  - 677 to 65,535 are reserved.
- **48-bit serials** give 2.8 × 10¹⁴ IDs per country.
- **The namespace is where an ID was allocated, not where the place is now.** A boundary
  correction that moves a place keeps its ID; the text form names the allocating country only.
  That is the price of no coordination, and it is documented.
- **Crockford base32** (Crockford, "Base32", crockford.com):
  - case-insensitive, and excludes `i l o u`;
  - used by ULID;
  - we emit lower case and parse either case, but reject `i l o u` and leading zeros, so each ID
    has exactly one text.
  - RFC 4648 base32 has ambiguous letters; base62 is case-sensitive, which is bad for IDs people
    type.

### 3.4 Reproducibility

- `SOURCE_DATE_EPOCH` is the reproducible-builds.org convention for "the time a build says it
  was made". The forge sets it from `sources.lock`'s newest pin.
- **The pack UUID cannot be random**, or bytes differ. It is the first 16 bytes of
  BLAKE3("teigeodata pack uuid" ‖ address ‖ component ‖ release).
- `build_id` is already a hash (builder version + `sources.lock`).

### 3.5 Harnesses

- **bolero** 0.13.7 (2026-10-03):
  - `bolero::check!().with_type::<T>()` or `.with_generator(...)` runs under `cargo test` with
    random inputs, and replays a corpus directory;
  - `cargo bolero test <name>` runs the same harness under libFuzzer, AFL or honggfuzz;
  - it uses `arbitrary` 1.5.
  - Linux needs `binutils-dev` and `libunwind-dev` for cargo-bolero.
- **cargo-fuzz** 0.13.2 (2026-06-09) is nightly-only and its harnesses are separate `fuzz/`
  binaries.
- **criterion** 0.8.2 (2026-02-04) does wall-clock statistics and comparison against a saved
  baseline.
- **gungraun** 0.20.0 (2026-09-26) uses Valgrind's Callgrind, Cachegrind and DHAT, or Linux
  perf. Its counts are stable on noisy machines, which suits a PR gate. Valgrind does not support
  current macOS, so its gates run in CI.
- **Miri** (nightly) runs the reader tests to catch undefined behaviour in dependencies; our
  crates have no `unsafe`.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `zerocopy` derives on LE fields | compile-time layout checks, unaligned, endian-safe, no `unsafe` in our code | proc-macro compile time | BSD-2 / Apache-2.0 / MIT | 0.8.63, 2026-10-13 | **use** |
| hand-written `from_le_bytes` | no dependency | every offset written twice by hand | — | — | no |
| `bytemuck` | simple | alignment required; no endian types | Zlib / Apache / MIT | 1.25.2 | no |
| own canonical TLV manifest | tiny, canonical, critical-bit | ours to fuzz | ours | — | **use** |
| CBOR via `minicbor` | standard | canonical form ours to enforce; wide decoder | BlueOak-1.0.0 | 2.3.1, 2026-10-18 | no |
| ID: namespace + serial | no coordination; text from u64 alone | namespace is the allocating country | — | — | **use** |
| ID: opaque central sequence | simplest | one allocator for every build | — | WOF | no |
| ID: UUID | no coordination | 128 bits; unreadable | — | Overture | no |
| bolero | one harness for tests and fuzzing | Linux libs for cargo-bolero | MIT | 0.13.7, 2026-10-03 | **use** |
| cargo-fuzz | standard | nightly; harness copies | MIT / Apache | 0.13.2 | no |
| criterion + gungraun | wall clock and stable counts | gungraun Linux-only | Apache / MIT | 0.8.2; 0.20.0 | **use both** |

## 5. What we build ourselves, and why

- **The container reader and writer:** no crate reads a layout we define in place (the format note
  §2).
- **The TLV manifest:** §3.2.
- **The ID and its base32 text:** about 80 lines, round-trip tested on every edge (0, 1, the
  largest serial, every namespace).
- **The codecs, each against its benchmark:**
  - B1, integer blocks;
  - B3 and B4, names;
  - B5, ID to row;
  - B6, rank and select;
  - B12, integrity;
  - B13, checked access.

## 6. Impact on the plan

- **New:** [phase 1](../../07-planning/phases/phase-1.md), with these steps and their order.
- **[pack format](../../04-architecture/pack-format.md):**
  - kind 16 REDIRECTS;
  - the manifest TLV rules (canonical order, critical bit);
  - deterministic UUID and `created_unix`;
  - M1's reading of "every section kind".
- **[schema](../../05-data/schema.md):** the ID's text form is `tgd:<cc>/<serial>`, with no
  kind (D-070).
- **[overview](../../04-architecture/overview.md):** the dependencies of `core` and `format`, and
  the `write` and `signature` features.
- **[benchmarks](../../06-engineering/benchmarks.md):** the harness choice, criterion locally and
  gungraun in CI.

## 7. Risks and open items

- **zerocopy's derive and the `indexing_slicing` lint:** the reader uses `get` and
  `Ref::from_prefix`, never `[]`. B13 measures what that costs in hot loops.
- **bolero's random mode under `cargo test` must be proven** to find a planted bug before it is
  trusted (AGENTS.md §6).
- **The critical-tag rule** must be tested both ways: an unknown non-critical tag is skipped, and
  an unknown critical one refuses the pack.
- **ID namespace exhaustion is not a risk:** 676 countries of 2.8 × 10¹⁴ serials each.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| zerocopy | 0.8.63 (2026-10-13) | 2026-11-07 | crates.io API |
| blake3 | 1.8.7 (2026-08-20) | 2026-11-07 | crates.io API |
| ed25519-dalek | 3.0.0 (2026-07-06) | 2026-11-07 | crates.io API |
| memmap2 | 0.9.12 (2026-10-21) | 2026-11-07 | crates.io API |
| ruzstd | 0.9.1 (2026-10-11) | 2026-11-07 | crates.io API |
| bolero | 0.13.7 (2026-10-03) | 2026-11-07 | crates.io API |
| arbitrary | 1.5.0 (2026-10-06) | 2026-11-07 | crates.io API |
| libfuzzer-sys | 0.4.13 (2026-06-04) | 2026-11-07 | crates.io API |
| cargo-fuzz | 0.13.2 (2026-06-09) | 2026-11-07 | crates.io API |
| criterion | 0.8.2 (2026-02-04) | 2026-11-07 | crates.io API |
| gungraun | 0.20.0 (2026-09-26) | 2026-11-07 | crates.io API |
| tango-bench | 0.9.0 (2026-10-16) | 2026-11-07 | crates.io API |
| fastlanes | 0.7.2 (2026-09-02) | 2026-11-07 | crates.io API |
| bitpacking | 0.9.3 (2026-01-08) | 2026-11-07 | crates.io API |
| sucds | 0.11.0 (2026-10-25) | 2026-11-07 | crates.io API |
| vers-vecs | 1.10.2 (2026-08-10) | 2026-11-07 | crates.io API |
| ptr_hash | 2.1.3 (2026-10-21) | 2026-11-07 | crates.io API |
| fsst-rs | 0.7.0 (2026-10-27) | 2026-11-07 | crates.io API |
| lz4_flex | 0.14.0 (2026-07-14) | 2026-11-07 | crates.io API |
| zstd | 0.14.3 (2026-11-01) | 2026-11-07 | crates.io API |
| proptest | 1.12.0 (2026-10-10) | 2026-11-07 | crates.io API |
| minicbor | 2.3.1 (2026-10-18) | 2026-11-07 | crates.io API |

## 9. Sources

- [zerocopy documentation](https://docs.rs/zerocopy/0.8.63/zerocopy/), Google, accessed 2026-11-07.
- [bolero README and book](https://github.com/camshaft/bolero), accessed 2026-11-07.
- [gungraun README](https://github.com/gungraun/gungraun), accessed 2026-11-07.
- [Crockford, "Base32"](https://www.crockford.com/base32.html), accessed 2026-11-07.
- [ULID specification](https://github.com/ulid/spec), accessed 2026-11-07.
- [SOURCE_DATE_EPOCH](https://reproducible-builds.org/specs/source-date-epoch/), reproducible-builds.org, accessed 2026-11-07.
- [PNG specification, chunk naming (critical bit)](https://www.w3.org/TR/png-3/#5Chunk-naming-conventions), W3C, accessed 2026-11-07.
- [RFC 8949 §4.2, deterministically encoded CBOR](https://www.rfc-editor.org/rfc/rfc8949#section-4.2), IETF, 2020.
- [Who's On First IDs](https://whosonfirst.org/docs/principles/), accessed 2026-11-07.
- [Overture GERS](https://docs.overturemaps.org/gers/), accessed 2026-11-07.
- [Format, compression and indexing](format-compression-indexing.md), this repository, 2026-11-06.
