---
title: "Zone rules in the pack: TZif, POSIX TZ footers, and the TZ_RULES section"
status: accepted
updated: 2026-11-11
owner: maintainer's agents
revisit: 2027-05-11
---

# Zone rules in the pack: TZif, POSIX TZ footers, and the TZ_RULES section

## 1. Question

Consumer request TG-03 ([Tithipatra's brief](../../01-product/consumers/tithipatra.md)) asks for
zone rules in the pack, from the same tzdb release as the zone outlines (`tzdb-2026f` with
`tbb-2026f` in `data/sources.lock`). It wants:

- compiled rules for every zone the pack's outlines name, with the tzdb version in the manifest;
- `tz_offset_at(zone, instant)`: offset in seconds, abbreviation, `is_dst`, and whether the
  instant is before the zone's first transition (tzdb's LMT stub);
- `tz_local_candidates(zone, civil datetime)`: one offset, a gap (offset before and after), or
  an overlap (earlier and later);
- `tz_version()`.

Acceptance: Nepal's history is exact (LMT +5:41:16 before 1920, +05:30 until 1986, +05:45
since); `America/New_York` 2024-03-10 02:30 is a gap; a gate proves every zone in the outlines
has rules.

[ADR-0033](../../03-decisions/adr/ADR-0033-timezone-data.md) already decided "our own TZif
bundle built from the matching tzdb release", loaded through `jiff`. This note asks how the
rules are stored and read: raw TZif or our own columns, `jiff` or our own reader. It feeds
phase 4 §4.9b (TG-03), the pack format, the engine, the forge and a new decision.

## 2. Summary and recommendation

- **Store parsed rules as columns, not raw TZif.** A new section `TZ_RULES` (kind 22; 21 was taken while this was built), in a new
  component `tz-rules` that needs nothing; `timezone` and `timezone-index` need it. The forge
  compiles tzdb with `zic -b slim`, parses each TZif (trusted, build side) and writes:
  sorted zone IDs, deduplicated rule sets, transition times, local time types, an abbreviation
  table, and each POSIX TZ footer pre-parsed into fixed integer fields. Measured on 2026f: the
  444 outline zones need **338 distinct rule sets, 16,628 transitions, 1,590 type rows, 93
  distinct footers**; the columns come to about **60–75 KB** for the whole world (estimate from
  the measured counts), against **202,081 B** of deduplicated slim TZif. A Nepal or India pack
  needs well under 3 KB either way.
- **Own reader in `teigeodata-format`, not `jiff`.** `no_std`, no allocation, never panics,
  fuzzed like every other section. The reader never parses text at run time: the footer arrives
  as integers. Evaluating it takes civil-date arithmetic and the `Mm.w.d` / `Jn` / `n` rules,
  about 150–250 lines; the whole reader with gap and overlap logic is estimated at 500–700
  lines. `jiff` stays a **dev-dependency** for the differential test. This changes the "loaded
  through `jiff`" clause of ADR-0033 and needs a decision row (§6).
- **Lossless:** a `tzif(zone)` function (feature `alloc`) rebuilds slim TZif v2 from the
  columns; a test proves it byte-identical to `zic`'s output for every zone. A consumer that
  wants TZif bytes (for `jiff::tz::TimeZone::tzif`, ICU, or its own provider) still gets them.
- **Build:** `zic -b slim` from the pinned tzdata, no `backzone` by default (TG-08 decides it
  separately), with `zic` itself built from the matching pinned `tzcode` so the output does not
  depend on the host's `zic`. Two runs of the same `zic` on 2026f gave identical bytes.
- **Gate G-tzrules:** every timezone place in the pack has a rule set in `TZ_RULES`, every
  zone ID in `TZ_RULES` resolves in the pinned tzdb, and the manifest's tzdb version equals the
  pinned source's. Differential test: our reader agrees with `jiff` (reading the same `zic`
  bytes) for every zone at every transition ±1 s and at sampled instants 1800–2100, and with
  the host's `/usr/share/zoneinfo` where its `+VERSION` matches.

**Unverified:** whether `zic` 2022g (macOS `/usr/sbin/zic`) and `zic` 2026f produce identical
slim bytes for 2026f data (building tzcode was not possible in this session); the column size
is estimated, not written by a real encoder.

## 3. Findings

### 3.1 TZif (RFC 9636)

RFC 9636 (October 2024) obsoletes RFC 8536. A TZif file is a 44-byte header, a v1 data block
with 32-bit times, then for v2+ a second header and data block with 64-bit times, then a footer:
a newline, a POSIX TZ string, a newline.

- Data block, in order: transition times (`timecnt` × i64, strictly increasing), transition
  type indices (`timecnt` × u8), local time types (`typecnt` × {utoff i32, isdst u8, abbr index
  u8}), abbreviation chars (`charcnt`), leap-second records, standard/wall and UT/local
  indicators.
- Before the first transition, local time is type 0. With no transitions, a non-empty footer
  governs; otherwise type 0. On or after the last transition, the footer governs. The footer
  "MUST be consistent with the last version 2+ transition".
- Versions: v2 adds 64-bit data and the footer; v3 lets footer hours run from -167 to 167 and
  allows all-year DST; v4 lets leap-second tables be truncated and carry an expiry.
- Reader guidance: ignore the v1 block in v2+ files; check every count against the bytes
  ("All counts should be checked"); utoff should be in -89999..93599 and never -2^31.
- Our 2026f slim build: every file is version `2`, the v1 block is empty (all counts 0), and
  there are no leap-second records or indicator arrays. 2026f's NEWS says there are no more
  leap seconds and `zic` ignores `Expires` lines.

`Asia/Kathmandu`, slim, is 161 bytes: 2 transitions, 3 types (LMT +20476 s = +5:41:16, `+0530`,
`+0545`), footer `<+0545>-5:45`. `zdump -v` on the 2026f system zoneinfo shows the exact
history, including two cases a consumer must handle:

| UTC instant | Before | After | Local effect |
|---|---|---|---|
| 1919-12-31 18:18:44 | LMT +5:41:16 | +05:30 | clocks go back 11 min 16 s: 1919-12-31 23:48:44 to 23:59:59 happens twice (an **overlap**) |
| 1985-12-31 18:30:00 | +05:30 | +05:45 | clocks jump 15 min: 1986-01-01 00:00:00 to 00:14:59 never happens (a **gap**) |

So Nepal alone exercises the LMT stub, an overlap and a gap.

`America/New_York`, slim, is 1,744 bytes with 175 transitions; the last is 2007-03-11 07:00 UTC
and the footer `EST5EDT,M3.2.0,M11.1.0` covers everything after. The 2024 gap (2024-03-10
07:00 UTC, 01:59:59 EST then 03:00:00 EDT) and overlap (2024-11-03 06:00 UTC, 01:00–01:59
twice) both come **from the footer**, so the acceptance test exercises the footer evaluator,
not the transition table. A fat file materialises transitions to 2037 instead (3,552 B for this
zone on macOS).

### 3.2 The footer: POSIX TZ strings in 2026f

Measured over all 345 distinct slim files (`scripts` in the session scratchpad, not kept):

- 94 distinct footers (93 over the 338 outline rule sets); 105 rule sets use a DST rule.
- Every DST rule uses the `Mm.w.d` form. No footer uses `Jn` or `n`. The RFC still allows them,
  so the reader supports them.
- v3 extended times occur: `/24` (`M10.5.4/24`, `M9.1.6/24`), `/26`
  (`IST-2IDT,M3.4.4/26,M10.5.0`), `/50` (`EET-2EEST,M3.4.4/50,M10.4.4/50`), negative `/-1`
  (`<-02>2<-01>,M3.5.0/-1,M10.5.0/0`) and minutes (`M9.5.0/2:45` on `+1245`).
- Negative DST: `Europe/Dublin` is `IST-1GMT0,M10.5.0,M3.5.0/1` in tzdb's main form, so its
  *winter* type has `isdst = 1`. `is_dst` is tzdb's flag, not "summer time"; the API docs must
  say so.
- Some zones list future transitions explicitly instead of a rule (Morocco's footer is `<+00>0`
  with its last transition in 2026). Two rule sets have transitions after 2038.

Evaluating a footer needs: days from a civil date and back (Hinnant's algorithm, about 20
lines), weekday of a date, "week `w` of month `m`" (week 5 = last), Julian `Jn` (no Feb 29) and
zero-based `n`, transition time in seconds within ±167 h, and the southern-hemisphere case where
the start date is after the end date. Parsing the string adds the grammar (quoted `<...>` names,
signed `hh[:mm[:ss]]` with inverted sign). Line counts, excluding comments and blank lines:

| Code | TZif parse | POSIX parse + evaluate |
|---|---:|---:|
| `jiff-core` 0.1.4 (`tz/tzif/parser.rs`, `tz/posix.rs`) | ≈1,080 (parser) + ≈190 (query) | ≈2,700 in `posix.rs` with its error types and tests |
| `tz-rs` 0.7.3 (`parse/tz_file.rs`, `parse/tz_string.rs`, `timezone/rule.rs`) | ≈175 | ≈150 parse + ≈490 rules |

Pre-parsing the footer in the forge removes the parser from the run-time reader entirely, which
is the part that would take hostile text.

### 3.3 Sizes (2026f, `zic` 2022g on macOS, measured 2026-11-11)

| Build | Files | Bytes | Distinct files | Distinct bytes | gzip -9 | zstd -19 | xz -9e |
|---|---:|---:|---:|---:|---:|---:|---:|
| `-b slim`, no backzone | 598 | 345,682 | 345 | 206,112 | 61,295 | 35,871 | 33,576 |
| `-b fat`, no backzone | 598 | 697,200 | 345 | 405,917 | 136,284 | 62,561 | 56,548 |
| slim, `PACKRATDATA=backzone PACKRATLIST=zone.tab` | 598 | | 440 | 233,650 | | | |
| slim, `PACKRATDATA=backzone` (all) | 599 | | 466 | 249,624 | | | |

The 598 names are 345 zones and 253 links (`main.zi`: 345 `Z` lines, 253 `L` lines); links
compile to identical bytes, so dedupe by content.

The outlines: all **444** `tzid`s in `tbb-2026f` (`timezones-with-oceans`) exist in the slim
build (0 missing). Their files total 236,013 B; deduplicated, 338 files and **202,081 B**
(35,832 B at zstd -19).

Per zone, slim: `Asia/Kathmandu` 161, `Asia/Kolkata` 220, `Asia/Shanghai` 393, `Asia/Dhaka`
231, `Asia/Thimphu` 154, `Asia/Karachi` 266, `Asia/Yangon` 187, `Asia/Colombo` 247,
`Etc/GMT-5` 114, `America/New_York` 1,744, `Europe/London` 1,599. A Nepal pack holds the
zones whose outline meets its coverage (Kathmandu, Kolkata, Shanghai, perhaps one or two more):
under 1 KB. An India pack with its neighbours: about 2–3 KB. Size matters only for the world
and continent packs.

Columnar estimate for the 444 outline zones (338 rule sets), from the measured counts:

| Part | Count | Estimate |
|---|---:|---:|
| transition times as minutes since a section base (27 bits; 16,244 of 16,628 are whole minutes) | 16,628 | ≈56 KB |
| seconds remainder (FOR blocks, width 0 in most blocks) | 16,628 | ≈1–2 KB |
| type index per transition (≤ 4 bits; max 11 types per zone) | 16,628 | ≈6–8 KB |
| type rows (offset, `isdst`, abbreviation index) | 1,590 | ≈4 KB |
| abbreviations (178 distinct, 821 B of text) | | ≈1 KB |
| footers pre-parsed (93 distinct) | | < 1 KB |
| zone IDs (444 names, ≈14 B each) and per-name columns | | ≈7 KB |
| **Total** | | **≈60–75 KB** |

The section bases matter: transition times span -3,944,631,116 to 3,244,921,200 (33 bits),
and `teigeodata_core::bitpack` packs u32 values with widths up to 32, so seconds do not fit a
single column; minutes since the base do (27 bits).

On the wire packs travel zstd-19 framed ([transport](../../04-architecture/transport.md)), where
raw TZif compresses to about 36 KB; the columns will compress less well (bit-packed data is
dense). Installed packs are memory-mapped uncompressed, where the columns are about a third of
the TZif bytes. Either way the world rules are below 0.2 % of a world tz pack (ADR-0033: about
43 MB estimated).

### 3.4 Building with `zic`

- `zic` has defaulted to `-b slim` since tzcode 2020b ("zic now defaults to '-b slim'", NEWS);
  `-b` arrived in 2019b. Fat files exist for old readers with 32-bit-only or no-footer support;
  we need neither.
- macOS ships `/usr/sbin/zic` from tzcode 2022g, and its `/usr/share/zoneinfo/+VERSION` is
  already 2026f (fat files). Linux distributions ship `zic` with glibc (`libc-bin`), with their
  own version lag.
- The tzdata tarball's file list fed straight to `zic` (`africa antarctica asia australasia
  europe northamerica southamerica etcetera backward factory`) and the Makefile's `main.zi`
  gave identical output trees. Two runs of `zic -b slim` on `main.zi` gave identical bytes
  (determinism within one `zic`).
- `backzone`: the Makefile's `PACKRATDATA=backzone PACKRATLIST=zone.tab` adds pre-1970 history
  for zones that 2022b turned into links (95 more distinct files, +27 KB). It is "out-of-scope
  and often-wrong" in tzdb's own words; TG-08 researches it. The section must record whether a
  build used it, because it changes answers for the same version string.
- Writing our own `zic` is not an option: `jiff`'s author left an unfinished `zic.rs`, noting
  that turning the source format into rules "has proven quite difficult".

### 3.5 Libraries

| Crate | Version (crates.io, 2026-11-11) | Package size | Licence | TZif reading |
|---|---|---:|---|---|
| `jiff` | 0.2.42 (2026-11-10) | 682,974 B | Unlicense OR MIT | `TimeZone::tzif(name, bytes)` needs `alloc` (it builds an `Arc`); parsing lives in `jiff-core` |
| `jiff-core` | 0.1.4 (2026-11-08) | 129,786 B | Unlicense OR MIT | `no_std`; TZif parser, POSIX TZ, `AmbiguousOffset` |
| `jiff-tzdb` | 0.1.9 (2026-10-06) | 62,876 B | Unlicense OR MIT | a bundled concatenated tzdb; older than 2026f |
| `tz-rs` | 0.7.3 (2026-01-29) | 48,104 B | MIT OR Apache-2.0 | `forbid(unsafe_code)`, TZif + POSIX TZ |
| `tzif` | 0.5.0 (2026-04-01) | 26,769 B | Unicode-3.0 | parser only |
| `chrono-tz` | 0.10.4 (2025-07-11) | 794,377 B | MIT OR Apache-2.0 | compiles tzdb into Rust code at build time; embeds 2025b ([earlier note](timezone-and-elevation.md)) |
| `tzfile` | 0.1.3 (2021-03-21) | 11,741 B | MIT | unmaintained since 2021 |

`jiff`'s query side returns `AmbiguousOffset::{Unambiguous { offset }, Gap { before, after },
Fold { before, after }}`. `java.time.zone.ZoneRules.getValidOffsets(LocalDateTime)` returns an
empty list in a gap, one offset normally, two in an overlap; `getTransition` then gives
`offsetBefore` and `offsetAfter`. Both name the gap's offsets by the transition: before = the
offset in force before it (EST, -5 h), after = after it (EDT, -4 h). An adapter in teistro (a
`TimeZoneProvider`-like trait) maps our answer to either shape without loss if we return the
same three cases with both offsets.

### 3.6 How zone names reach the rules

- Timezone places are `PlaceKind::TimezoneArea` with key `tz:<tzid>` and the `tzid` as their
  official name (`forge/crates/forge/src/build/zones.rs`), both for TBB (main) and tzdb's
  `zone.tab` (lite, [lite timezones](lite-timezones.md)).
- `Engine::tz_at(point)` returns the area's `Place` through REGION's `PLACE` column, or through
  admin units and `PLACE_TZ` in lite packs; `tz_of(place)` goes through `PLACE_TZ`.
- Getting from a `Place` to the string `Asia/Kathmandu` today needs the `names` component. A
  client with `timezone-index` and `tz-rules` but without `names` must still get offsets, so
  `TZ_RULES` carries the PLACES row of each zone's area.
- TBB keeps country IDs such as `Europe/Amsterdam`, which tzdb makes a link; dedupe by content
  makes the alias free, and the zone ID returned stays the TBB one.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| (a) raw slim TZif blobs, deduplicated, with a sorted name index | simplest forge step (copy `zic` output); bytes hand straight to `jiff`/ICU; 202 KB world, 36 KB zstd | the run-time reader parses TZif and the POSIX string on every open or query (hostile-input parser in the hot path); big-endian i64 arrays do not fit our column tables, so it needs its own validation and fuzz target; 3× the installed bytes of (b) | public-domain data | RFC 9636 | no |
| (b) own columns: transitions, types, abbreviations, pre-parsed footers, zone IDs, area rows | integer-only reader on the shared, fuzzed column table; canonical encoding like every section; ≈60–75 KB world; no text parsing at run time; lossless (rebuilds TZif byte-identically) | forge must parse TZif and footers (trusted side); one more section spec | public-domain data | ours | **yes** |
| (a) + `jiff` as the engine's reader | no rule code of ours | `jiff` 683 KB package + `jiff-core`; `TimeZone::tzif` needs `alloc` and an `Arc` per zone; ties our `no_std` format crate and MSRV to `jiff`; against "engine stays small, tree-shakable" | Unlicense OR MIT | 0.2.42 | no (dev-dependency only) |
| (a) + `tz-rs` | small, `forbid(unsafe)` | still a run-time text parser; allocates (`Vec`) | MIT OR Apache-2.0 | 0.7.3 | no |
| Embed a crate's tzdb (`jiff-tzdb`, `chrono-tz`) | zero build work | stale (ADR-0033); not the outlines' release | | | no (rejected in ADR-0033) |

## 5. What we build ourselves, and why

### 5.1 Section `TZ_RULES` (kind 22), component `tz-rules`

A column table (encoding 1). All columns are u32 bit-packed unless noted; "set" is a
deduplicated rule set (identical TZif content).

| ID | Column | Values |
|---:|---|---|
| 1 | `META` | [base minute as two u32 halves of an i64, flags: bit 0 = built with `backzone`] |
| 2 | `NAME_TEXT` | bytes (codec 2): every zone ID, concatenated, in byte order |
| 3 | `NAME_ENDS` | per name: end offset in `NAME_TEXT`; names strictly increasing, ASCII per tzdb's theory rules |
| 4 | `NAME_SET` | per name: its set |
| 5 | `NAME_PLACE` | per name: 1 + the PLACES row of its timezone area, or 0 |
| 6 | `SET_TRANS_ENDS` | per set: end index into the transition columns |
| 7 | `SET_TYPE_ENDS` | per set: end index into the type columns |
| 8 | `SET_FOOTER` | per set: index into the footer columns |
| 9 | `TRANS_MINUTE` | per transition: minutes since the base, strictly increasing within a set |
| 10 | `TRANS_SECOND` | per transition: 0–59 |
| 11 | `TRANS_TYPE` | per transition: type index within its set |
| 12 | `TYPE_OFFSET` | per type: utoff + 93,600 (so -93,599..93,599 fits unsigned) |
| 13 | `TYPE_FLAGS` | per type: bit 0 `isdst`; abbreviation index << 1 |
| 14 | `ABBR_TEXT` | bytes: the distinct abbreviations, concatenated |
| 15 | `ABBR_ENDS` | per abbreviation: end offset |
| 16 | `FOOT_STD` | per footer: standard type (offset + abbreviation, as in 12/13) |
| 17 | `FOOT_DST` | per footer: 1 + DST type, or 0 for no DST |
| 18 | `FOOT_RULES` | per footer with DST: start date, start time, end date, end time (4 values; date = kind 2 bits + `M` month/week/day or `J`/`n` day; time + 604,800 within ±167 h) |

**As built (D-196):** `SET_FOOTER` is 1 + the footer, 0 for an empty footer; `FOOT_STD` and
`FOOT_DST` hold two values per footer (offset + 93,600 and abbreviation; 1 + DST abbreviation
or 0, and the DST offset or 0), `FOOT_RULES` four per footer, zeros without DST; and column 19
`SET_TZIF` holds each set's TZif version − 2 (bits 0–1) and, above them, the type whose
abbreviation `zic` laid out first (`zic` swaps the default type into place 0 before laying out
the characters), which makes the TZif rebuild byte-identical. The specification is
[pack format § TZ_RULES](../../04-architecture/pack-format.md#tz_rules).

Rule sets are ordered by first use in name order, so the encoding is canonical. Type 0 of each
set is TZif's type 0 (the LMT stub where tzdb has one). The open check proves: names sorted and
unique; every end column non-decreasing and ending at its column's length; every index in
range; transitions strictly increasing within a set; offsets within range; footer fields
within their ranges. `check` (the deep check) also proves each footer consistent with its set's
last transition, as RFC 9636 requires. Reads never panic.

`timezone` (`PLACE_TZ`) and `timezone-index` (REGION) gain the need `tz-rules`: the bytes are
negligible and a zone without rules is a broken answer. The component table, `check-pack`, the
planner and the bindings take the new row from `teigeodata_format::component`.

### 5.2 Manifest

A new tag `TZDB` (0x0009, text, not critical), e.g. `2026f`, present exactly when `TZ_RULES`
is. As built, the value is always the plain release; `backzone` is only the `META` flag. The tzdb version also stays with the TBB version in
the pack's source list ([ADR-0022](../../03-decisions/adr/ADR-0022-timezone-and-postal.md)).

### 5.3 Format reader API (`teigeodata_format::tz_rules`, `no_std`, no `alloc`)

```rust
pub struct TzRules<'a>;                 // open(section) -> Result<Self, Error>; check()
pub struct Zone<'a>;                    // one name and its rule set
pub struct LocalType<'a> { pub offset: i32, pub abbr: &'a str, pub is_dst: bool, pub before_first: bool }
pub struct Civil { pub year: i32, pub month: u8, pub day: u8, pub hour: u8, pub minute: u8, pub second: u8 }
pub enum Local { Unique(i32), Gap { before: i32, after: i32 }, Overlap { earlier: i32, later: i32 } }

impl<'a> TzRules<'a> {
    pub fn zone(&self, name: &str) -> Option<Zone<'a>>;      // binary search on the names
    pub fn zone_of_place(&self, row: u32) -> Option<Zone<'a>>;
    pub fn names(&self) -> impl Iterator<Item = &'a str>;
}
impl<'a> Zone<'a> {
    pub fn name(&self) -> &'a str;
    pub fn offset_at(&self, unix_seconds: i64) -> Result<LocalType<'a>, Error>;
    pub fn local(&self, civil: Civil) -> Result<Local, Error>;
    pub fn transitions(&self) -> impl Iterator<Item = (i64, LocalType<'a>)>; // stored ones only
    #[cfg(feature = "alloc")] pub fn tzif(&self) -> Vec<u8>;            // slim TZif v2
}
```

- `offset_at`: binary search the set's transitions; before the first, type 0 with
  `before_first = true`; at or after the last (or with none), the footer. `before_first` is
  false for zones with no transitions (`Etc/GMT-5`).
- `local`: a civil time `L` (as local seconds) is valid with offset `o` exactly when
  `offset_at(L - o).offset == o`. The candidates are the offsets in force over the UTC window
  `[L - 93,599, L + 89,999]` (at most a few); 0 valid means a gap (before and after taken from
  the transition inside the window), 2 means an overlap (earlier = the pre-transition offset,
  which gives the earlier instant). `Unique`, `Gap` and `Overlap` map one-to-one to `jiff`'s
  `AmbiguousOffset` and to `java.time`'s `getValidOffsets` plus `getTransition`.
- Range: civil years -9999..=9999 and the matching instants; outside is `Error::Range`. All
  arithmetic is checked; an invalid `Civil` (Feb 30, hour 24) is an error, not a panic.
- Instants are Unix seconds without leap seconds (the `right/` zones are out of scope).

### 5.4 Engine API (Cargo feature `tz-rules`)

```rust
impl Engine {
    pub fn tz_offset_at(&self, zone: &str, unix_seconds: i64) -> Result<Option<Offset>, Error>;
    pub fn tz_local_candidates(&self, zone: &str, civil: Civil) -> Result<Option<Local>, Error>;
    pub fn tz_version(&self) -> Option<&str>;
    pub fn tz_offset_at_point(&self, at: Point<E7>, unix_seconds: i64) -> Result<Option<Offset>, Error>;
}
```

`Offset` owns its abbreviation (at most a few bytes) so it does not borrow a layer snapshot.
A zone name is looked up in the layers in precedence order; the first `TZ_RULES` holding it
answers. `tz_version` is the `TZDB` value of that same winning order (the first layer with
`TZ_RULES`). `tz_offset_at_point` goes `tz_at` → area row → `zone_of_place`, so it works without
the `names` component. `None` means no installed pack has rules for that zone.

### 5.5 Forge step (`build/tz_rules.rs`)

1. Read the pinned tzdata tarball (`tzdb-2026f`) and a pinned `tzcode` (a new `sources.lock`
   entry, same release), both SHA-256 checked.
2. Build `zic` from tzcode once into the scratch directory; run `zic -b slim -d <scratch>` on the
   data files in the Makefile's order (`backward` last); with the recipe option `backzone`, use
   the Makefile's `PACKRATDATA=backzone PACKRATLIST=zone.tab` instead.
3. For each zone the pack's timezone places name: read its TZif with a strict build-side
   parser (refuse v1-only, leap records, a footer it cannot parse), hash it, dedupe, parse the
   footer, and push columns.
4. Fail the build (G-tzrules) if any area's zone has no file.
5. Write `TZ_RULES` and the `TZDB` manifest tag; delete the scratch tree.

The build-side TZif and footer parsers live in the format crate's `write` feature, so the
round-trip test and fuzzing reach them.

### 5.6 Gate and tests

- **G-tzrules** (forge and `check-pack`): every `PlaceKind::TimezoneArea` place in the pack has
  a `TZ_RULES` name whose `NAME_PLACE` is that place; every name resolves in the pinned tzdb;
  `TZDB` equals the pinned version. Prove it fails on a planted violation (a zone removed from
  the section, a wrong version).
- **Nepal history** (exact, golden): `offset_at` at 1919-12-31 18:18:43 UTC = +20,476 `LMT`
  `before_first`; at 18:18:44 = +19,800 `+0530`; at 1985-12-31 18:29:59 = +19,800; at 18:30:00 =
  +20,700 `+0545`; at 2026-11-11 = +20,700. `local`: 1919-12-31 23:50 is `Overlap { earlier:
  +20,476, later: +19,800 }`; 1986-01-01 00:10 is `Gap { before: +19,800, after: +20,700 }`;
  1986-01-01 00:15 is `Unique(+20,700)`.
- **New York** (footer path): 2024-03-10 02:30 is `Gap { before: -18,000, after: -14,400 }`;
  2024-11-03 01:30 is `Overlap { earlier: -14,400, later: -18,000 }`; 2024-07-01 12:00 is
  `Unique(-14,400)` with `EDT`, `is_dst`.
- **Footer edge cases**, each a unit test from real 2026f footers: `/24`, `/26`, `/50`,
  `/-1`, `M9.5.0/2:45` on `+1245`, a southern-hemisphere rule, Dublin's negative DST, and `Jn`
  and `n` forms (not in 2026f, written by hand).
- **Differential, every zone**: for each of the 598 names, compare our `offset_at` and `local`
  with `jiff` (dev-dependency, `TimeZone::tzif` on the same `zic` bytes) at every stored
  transition ±1 s, at 1,000 seeded instants from 1800 to 2100, and at the footer transitions of
  2000–2100; and with `/usr/share/zoneinfo` when its `+VERSION` equals the pinned version.
- **Round trip**: `tzif()` rebuilds every zone's slim file byte-identically.
- **Property**: for random instants, `local(civil(offset_at(t)))` contains `offset_at(t).offset`.
- **Fuzz**: a `tz_rules` target (open, check, random queries) and the build-side TZif/footer
  parser on arbitrary bytes; both added to the format crate's `fuzz` payloads.
- **Size**: `SIZES.md` gains `TZ_RULES` for Nepal, India and the world; the world must come in
  under the raw deduplicated TZif bytes (202,081 B for 2026f), or option (a) wins on simplicity.
- **Speed**: a bench for `offset_at` and `local` (expected well under 1 µs; to be measured).

## 6. Impact on the plan

- A decision row: rules are stored as `TZ_RULES` columns and read by our own `no_std` reader;
  `jiff` is a dev-dependency only. ADR-0033's "loaded through `jiff`" no longer holds; record
  it with a new ADR that refines ADR-0033 (ADRs are not edited) or a decision row naming the
  change, as the maintainer's process prefers.
- [Pack format](../../04-architecture/pack-format.md): kind 22 `TZ_RULES`, component
  `tz-rules`, the manifest tag `TZDB`; the kinds test checks the new row.
- Engine page: the four queries and the `tz-rules` feature.
- [Phase 4](../../07-planning/phases/phase-4.md) §4.9b TG-03: steps in the order format →
  forge → engine → gate → bindings; TG-08 (`backzone`) stays a separate note and only flips
  the flag.
- `sources.lock`: a `tzcode-2026f` entry; the tz freshness check of ADR-0033 covers both.
- [Versions](../../06-engineering/versions.md): `jiff` 0.2.42 as a dev-dependency.

## 7. Risks and open items

- **`zic` version drift (resolved 2026-11-11):** the outputs differ. `zic` 2022g (macOS) and
  `zic` 2026f give different slim files for 40 of the 598 names, so the forge builds `zic`
  from the pinned tzcode 2026f.
- **Size (measured 2026-11-11, the encoder in D-196):** 97,260 B for all 598 names (345
  sets, 17,085 transitions) against 207,069 B of distinct slim TZif; 93,232 B for the 444
  zones the outlines name, against 202,081 B. That is larger than the 60–75 KB estimated
  from counts, but under half the raw TZif, so option (b) stands. Small packs pay a fixed
  cost per column (a partly filled 128-value block costs as much as a full one, about 16 B
  per bit of width): Kathmandu, Kolkata and Shanghai take 2,492 B against 774 B of TZif, not
  the < 1 KB estimated.
- **Mixed versions across layers:** two installed packs could carry different tzdb releases.
  The release process should keep one tzdb per data release; `tz_version` reports the
  answering layer's. Whether the engine should prefer the newest tzdb over layer precedence is
  open.
- **Abbreviations** are tzdb's, often numeric (`+0545`); they are not localised names.
- **`is_dst` semantics** follow tzdb's main form (Dublin's winter is DST). A consumer wanting
  rearguard semantics would need a separate build; not planned.
- **Pre-1970 accuracy** is tzdb's: zones merged into links since 2022b lose their own history
  unless `backzone` is used (TG-08).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| tzdata | 2026f (2026-10-27), SHA-256 `01b4a91c…3d3d` matches `sources.lock` | 2026-11-11 | data.iana.org |
| macOS `zic` | tzcode 2022g | 2026-11-11 | `/usr/sbin/zic --version` |
| macOS zoneinfo | 2026f (fat) | 2026-11-11 | `/usr/share/zoneinfo/+VERSION` |
| timezone-boundary-builder | 2026f, 444 `tzid`s | 2026-11-11 | `data/cache/tbb-2026f` |
| `jiff` | 0.2.42 (2026-11-10) | 2026-11-11 | crates.io |
| `jiff-core` | 0.1.4 (2026-11-08) | 2026-11-11 | crates.io |
| `jiff-tzdb` | 0.1.9 (2026-10-06) | 2026-11-11 | crates.io |
| `tz-rs` | 0.7.3 (2026-01-29) | 2026-11-11 | crates.io |
| `tzif` | 0.5.0 (2026-04-01) | 2026-11-11 | crates.io |
| `chrono-tz` | 0.10.4 (2025-07-11) | 2026-11-11 | crates.io |
| `tzfile` | 0.1.3 (2021-03-21) | 2026-11-11 | crates.io |

## 9. Sources

- [RFC 9636: The Time Zone Information Format (TZif)](https://www.rfc-editor.org/rfc/rfc9636.html), IETF, October 2024, accessed 2026-11-11.
- [tzdata2026f.tar.gz](https://data.iana.org/time-zones/releases/tzdata2026f.tar.gz), IANA, 2026-10-27: `NEWS`, `Makefile` (`PACKRATDATA`, `PACKRATLIST`, `-b slim`), `zone.tab`, `backzone`.
- [Theory and pragmatics of the tz code and data](https://data.iana.org/time-zones/theory.html), IANA, accessed 2026-11-11.
- [`zic` manual](https://man7.org/linux/man-pages/man8/zic.8.html), accessed 2026-11-11.
- [`jiff` 0.2.42 source](https://crates.io/crates/jiff/0.2.42) (`src/tz/timezone.rs`, `src/tz/ambiguous.rs`, `src/tz/zic.rs`) and [`jiff-core` 0.1.4](https://crates.io/crates/jiff-core/0.1.4) (`src/tz/tzif/parser.rs`, `src/tz/posix.rs`), read 2026-11-11.
- [`tz-rs` 0.7.3 source](https://crates.io/crates/tz-rs/0.7.3), read 2026-11-11.
- [`java.time.zone.ZoneRules`](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/time/zone/ZoneRules.html) (`getValidOffsets`, `getTransition`), Oracle, accessed 2026-11-11.
- [timezone-boundary-builder 2026f](https://github.com/evansiroky/timezone-boundary-builder/releases/tag/2026f), Evan Siroky, 2026-11-01.
- Local measurements, 2026-11-11: `zic -b slim|fat` on 2026f, `zdump -v` for `Asia/Kathmandu` and `America/New_York`, a TZif scan of every compiled file, sizes with gzip 9, zstd 19, xz 9e.
