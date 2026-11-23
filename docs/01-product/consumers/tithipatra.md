---
title: "Reply to Tithipatra's brief"
status: living
updated: 2026-11-11
---

# Reply to Tithipatra's brief

Tithipatra's maintainer handed over its first brief to teigeodata on 2026-11-11. The brief was
checked against main at `70561f8`. This page is our reply: one row per request (TG), its place
in our plan, its status, and the commit that closes it. Each request is built as a general
feature (P16), never as a case for one consumer. The plan items live in
[phase 4 §4.9b](../../07-planning/phases/phase-4.md#49b-consumer-requests); the decision to take
the brief is D-193.

We accept the brief's split of concerns (teigeodata: place, height, zone outlines, zone rules
and offsets; teistro: civil-time policy, sky and calendars, and the adapter).

## Status

| TG | Request | Where in our plan | Status | Commit | Notes |
|---|---|---|---|---|---|
| TG-01 | Releases a consumer can pin | 4.9b, then Phase 8 | done: tag `v0.1.0` (D-198) | [releases](../../06-engineering/releases.md), [CHANGELOG](../../../CHANGELOG.md) | A `v0.x` SemVer tag of the workspace with `CHANGELOG.md` once TG-02 to TG-04 land. Before 1.0 a minor version may break, as SemVer allows for 0.x; the changelog names each break. Publishing to crates.io needs the maintainer's account, so the first tags are git tags (`cargo` builds from `git` + `tag`) |
| TG-02 | ISO 3166 codes on admin units | 4.9b (format, forge, engine) | done (D-197) | this change | `Engine::iso_code(&place) -> Result<Option<IsoCode>, Error>` (feature `admin`), called on each entry `admin_at` and `ancestors` return; `IsoCode::as_str()` gives `"NP"` or `"NP-P3"` with no allocation, `country()` the alpha-2 part; `Engine::by_iso(code)` finds the place. Stored in a small `ISO_CODES` section of the `admin` component (604 B for Nepal's 8 codes), so `Place` stays small and points-only builds carry nothing. Codes come from `data/countries/<cc>/iso3166.yaml` with their source, never from names; Nepal's are NP and NP-P1 to NP-P7 (ISO change of 2023-11-23 checked). `tgd` prints the code beside each admin unit. Not yet: the world baseline's codes outside Nepal (its source's codes are stale, so they wait for CLDR's list), and India's pack (its file will carry IN-OD, IN-CG, IN-TS, IN-UK) |
| TG-03 | Zone rules (TZif) in the pack, from the outlines' tzdb release | 4.9b, per ADR-0033 | done (D-196) | this change | Section `TZ_RULES` (kind 22) in component `tz-rules`, which `timezone` and `timezone-index` need; manifest tag `tzdb` names the release (`2026f`). The forge compiles with `zic -b slim`, `zic` built from the pinned tzcode of the same release; gate G-tzrules proves every area has its rules, byte for byte. Engine feature `tz-rules`: `tz_offset_at(zone, unix_seconds) -> Option<Offset>` (seconds, abbreviation, `is_dst`, layer), `tz_local_candidates(zone, Civil) -> Option<Local>` (`Unique`, `Gap { before, after }`, `Overlap { earlier, later }`), `tz_version() -> Option<&str>`, and `tz_offset_at_point(at, unix_seconds)` for a point. What differs from the brief: the rules are stored as integer columns read by our own `no_std` reader, not as TZif blobs (93 kB for the world's 444 zones against 202 kB of TZif; `Zone::tzif` rebuilds the TZif byte for byte if a consumer wants it); a Nepal pack's three zones take 2.5 kB, not under 1 kB; `tz_version` answers the first layer by precedence with rules, not per zone. [ADR-0039](../../03-decisions/adr/ADR-0039-tz-rules.md) |
| TG-04 | A place's extent: `bbox_of`, `boundary(place, lod)` | 4.9b (engine) | done (D-194) | this change | `Engine::bbox_of(&place) -> Option<Rect>` (1e-6° grid, rounded outward) and `Engine::boundary(&place, lod, &mut Outline)` (rings with part and hole flags, level of detail 0 full to 6 coarsest, tolerance in cm). The format already stores each outline's box and arcs at several levels of detail; the engine exposes them |
| TG-05 | Nepal and India releases through sync | Phase 8 data release; W1 | blocked | | Nepal's release waits on maintainer steps: the input mirror (D-127), the signing ceremony, two real releases. India is wave W1 and needs its research note first. Until then, the files by hand: pre-release [data-2026.11.12](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.12) (Nepal lite pack and the `tithipatra`, `baseline` and `points-en` layers, with `SHA256SUMS`; read with `rev = "dbd5df8"` or later, D-247) |
| TG-06 | Search across scripts: substrings of 3+ characters | 4.9b (search) | done (D-199) | this change | `Engine::search` finds a place from 3 or more code points inside a word of any of its names, Latin or Devanagari: "mandu" and "माडौं" find Kathmandu, "ratnagar" and "राटनगर" Biratnagar. The index is the `SUBSTRING` section in an opt-in `substring` component (install it with `search`); engine feature `substring`, on by default. Ranking: exact 60, prefix 30, one edit 25, substring 20, each plus importance (one score, see the question below); on the real Nepal pack all nine golden cases rank within the top 3 (four first; the others behind a same-name district or a one-edit match). Size: 2.56 MB (25 B per place), 2.08 MB to download; whole query p50 7.4 µs, p99 110 µs. Not covered: a typo inside a substring, a substring across a space. [Note](../../02-research/notes/substring-search.md) |
| TG-07 | A height says where it came from | 4.9b (format, engine) | done (D-195) | this change | `elevation_at` returns `Height { meters, source, resolution_m, datum, layer }`; `source` is `Copernicus DEM GLO-30` in main packs and `NASADEM` in lite packs, `resolution_m` the north–south spacing (30.9 m for 1″), `datum` EGM2008 |
| TG-08 | Zones before 1970 (`backzone`) | 4.9b (research) | done (research) | this change | [Note](../../02-research/notes/zones-before-1970.md): tzdb 2026f's `backzone` has no South Asian zone (only a commented-out `Asia/Chennai` placeholder), so a backzone build changes no South Asian instant; the default build already carries `Asia/Kolkata`'s Madras time (+05:21:10) and IST since 1906, and the war-time +06:30. No outline source exists for Bombay or Calcutta time (timezone-boundary-builder issue 171 is open), so a pre-1955 Mumbai instant reads `Asia/Kolkata` and we ship nothing invented. Whether the rules are built with `backzone` limited to `zone.tab` is decided with TG-03 |
| TG-09 | Horizon profile from the DEM | [ideas](../ideas.md) | idea | | Scheduled after the DEM ships in releases |

## Addendum 1: offline first (2026-11-11)

The maintainer handed over addendum 1 (D-275, D-276 on Tithipatra's side): one embedded file,
15 MB on the wire at most (10 MB the target), with the world, Nepal in full and India's towns,
every place named in Nepali and English. Accepted as D-213, and placed before our accuracy
labels. Plan: [embedded layer](../../02-research/notes/embedded-layer.md).

| TG | Request | Status | Notes |
|---|---|---|---|
| TG-11 | Size report per component and language | done | `cargo xtask pack-sizes <pack> --langs ne,en`; tables in [sizes](../../05-data/sizes.md). Finding: only 9.1% of Nepal's places (main) and 0.7% of world cities have a Devanagari name today |
| TG-12 | Embedded layer from a profile | done (D-214) | `forge embed --profile <name>` over `data/embed/<name>.yaml`; `baseline` rebuilds today's file byte for byte; `tithipatra` (world with every city of 5,000 plus all of Nepal, Nepali, Hindi-fallback and English names, search, outlines at 20 m, zone rules for all 376 zones, D-224) is 8.75 MB on the wire, under the 10 MB target; `points-en` 1.18 MB. Zone outlines: the layer answers `tz_at` by the zone of its deepest unit at the point, the same zone Nepal's pack gives within the 20 m tolerance (D-245, gate G-tz-layer); no REGION in the layer |
| TG-13 | World cities of 5,000 with heights | done (D-219) | 69,972 cities (GeoNames cities5000, CC-BY 4.0), each height marked `surveyed` or `model` (`Engine::elevation_source`); the `tithipatra` layer is now 170,892 places, 8.28 MB on the wire |
| TG-14 | Nepali and English for every place | in progress (D-215, D-221, D-227, D-228, D-232) | [note](../../02-research/notes/nepali-names.md). World places: Wikidata's Nepali label, else its Hindi one (tagged `hi`), by GeoNames or Wikidata ID: 13,724 of the world part's places now carry a Devanagari name (`दुबई`, `बोस्टन`, `टोकियो`, `दिल्ली` find their city). Nepal's places: 99.0% now have a Nepali name; 93,758 of them come from our transliterator (gate G-xlit; about half are spelled as a native reader would), each marked as machine-made: `Engine::name_in(place, "ne-Deva")` prefers a name a person wrote, and `NameInfo.origin.is_machine()` tells your UI which to show differently. Official names from CBS now come first for 715 of the 753 local levels and 42 districts (`फक्ताङ्लुङ्ग`, type word cut), marked `official`, and a town named as its municipality shares that spelling; the other 35 districts' names in CBS's published file are shifted one or two rows (DANG = रुपन्देही), which a new build gate caught, so they stay machine-made until a sound source is pinned. Please send the 100-per-kind correction list as data when ready |
| TG-15 | India: towns, then villages | research done (D-216, D-217) | [note](../../02-research/notes/india-sources.md): Census 2011 towns joined to LGD; about 260 kB on the wire for 8,000 towns |
| TG-16 | Outlines only as fine as needed | done (D-229, D-230) | Measured per level ([note](../../02-research/notes/embedded-layer.md) §3.1): Nepal stays at 20 m (0.08% of places answer differently from full detail; 100 m would be 0.36%), its border included. Outlines are now stored only as precisely as their level needs (the world's 1e-4°, 11 m; Nepal's 11 cm), and the admin index uses larger leaves: the layer is **9.21 MB** on the wire (was 10.30), boundaries 2.03 MB, admin index 0.80 MB; `admin_at` about 0.7 µs. 7 of 170,892 places answer differently, all world cities within 8 m of a Natural Earth border; none in Nepal |
| TG-17 | Opens fast, searches fast, small in memory | done (D-218, D-235) | Gate G-phone, estimates for a low-end phone (Helio G85 class) on the `tithipatra` layer: open 0.9 ms, the first query of each kind 35 ms, a keystroke's search 0.5 ms at p99 (one frame is 16 ms), 0.04 MiB of heap; SUBSTRING built on the device in 2.5 s once (54 MiB), checked against the pack's signed hash ([note](../../02-research/notes/phone-class-bench.md)) |
| TG-18 | Search at the MVP's bar, strict-tier option | done (D-220, D-226, D-233, D-236) | Gate G-golden-tithipatra: [60 cases](../../../tests/golden/consumers/tithipatra-search.tsv) written from the MVP's search as your brief describes it (§1.2), run through `search_with` on the `tithipatra` layer with SUBSTRING derived as on the device; all pass. Among them: `काठमाडौँ` and `काठमाडौं` find Kathmandu; `हङ्` lists हंसपुर while typing (in the 40 shown, and in the first 10 with a home-country focus); `क्रिस्ननगर` finds कृष्णनगर; `viratnagar` finds Biratnagar (first with a focus in Nepal); `janakapur`, `fidim`, `goolmi`, `beergunj` find Janakpur, Phidim, Gulmi, Birgunj; `kathmandu` ranks Kathmandu first; `mandu` finds it from inside the word. `search_with` takes a focus point and a strict-tier option (D-226). Run it: `cargo run --release -p teigeodata-engine --all-features --example golden -- tests/golden/consumers/tithipatra-search.tsv <layer.tgd>`. Please send the MVP's own test cases, and we add them to the file |
| TG-19 | `reverse` with distance, bearing, inside | done (D-223, D-237, D-238) | Each `Near` carries `meters` and `bearing_deg` (from the place to the point, clockwise from north); `Engine::mark_inside(at, &mut scratch, &mut near)` sets `inside` for every country and unit among the answers with one `admin_at`. `Engine::reverse_in_country(at, k, …)` keeps the answer to the country holding the point, as the MVP's border rule did: by Birgunj (26.985, 84.880) the plain `reverse` names Raxaul, `reverse_in_country` names Birgunj; a point inside India names Raxaul, not the Nepali village across the line. `tgd reverse --in-country`. Point query p99 on a low-end phone 0.45 ms with it (G-phone). A settlement or locality with no outline holds the point when it is the nearest of its kind and within 750 m or 700 m (`SETTLEMENT_RADIUS_M`, `LOCALITY_RADIUS_M`, measured on building footprints): 10 m from a village is inside, 1.9 km is near |
| TG-10 | Coarse elevation layer | done (D-225) | Component `dem-coarse`: Nepal at 15″ (about 464 m), 518 kB on the wire; error against the 1″ DEM P50 15.9 m, P90 61 m; in the `tithipatra` layer. `Height.resolution_m` says which grid answered |

## Addendum 2: city ranking and run-time packs (2026-11-16, 2026-11-17)

Placed by D-362. TG-24 and TG-25 are P0; the browser comes first.

| TG | Request | Status | Notes |
|---|---|---|---|
| TG-20 | Modelled populations in the lite pack | planned: next Nepal data release | Reported 2026-11-17: lite gives Lumbini Province 2,078 people |
| TG-21 | A city's own population, not its district's | planned: next Nepal data release | Kathmandu's level-3 unit carries the district's count |
| TG-22 | Former units marked as former | planned: next Nepal data release | The old development regions sit at level 1; the former zones ("Bagmati zone") answer as current units (reported 2026-11-17) |
| TG-23 | Names per language in `export --format ndjson` | planned | As `get --names --json` gives them |
| TG-24 | The signed catalog live, and a mirror the catalog trusts | mirror page published; the live catalog waits on the key ceremony (maintainer) | Store, verifier and multi-mirror fetch are built. Mirror layout sent: metadata `N.root.json`, `timestamp.json`, `V.snapshot.json`, `V.targets.json`, `V.<role>.json`; assets `<address>.<component>.<release>.<hash8>.tgd.zst` with `Range` (206); CORS for browsers. The public page: [running a mirror](https://teigeodata.teispace.com/docs/concepts/mirrors) |
| TG-25 | The store in the browser (fetch, Cache Storage or IndexedDB, Worker, the baseline first) | research done ([note](../../02-research/notes/browser-store.md)): OPFS and an async Worker driver | Not built: `teigeodata-wasm` opens bytes only. The verify and install core is `no_std` and sans-I/O; missing are a fetch transport, a storage backend and a Worker entry |
| TG-26 | An app-sized pack: Nepal in full, the world's cities of 5,000+ and first-level units, ne/en/hi/sa | released: data-2026.11.17 (profile `app`, D-362) | 172,339 places, 12.55 MB on disk, 7.04 MB on the wire: Nepal with outlines at 20 m (`admin_at` to the local level: wards are points only, Q-035), the rest by point. With the world's outlines as stored: 15.12 MB, 9.24 MB on the wire (`admin_at` abroad to admin 1; the world part has no coarser level yet). India's districts and towns add 1,457 places, 0.16 MB (0.10 MB on the wire). Sanskrit names on 121 Nepal places, none abroad (no pinned source) |
| TG-27 | Nepal's wards with outlines, so `admin_at` names the ward | released: [data-2026.11.22](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.22) (D-363..366) | `app-np` (Nepal main, 108,995 places, wards at 50 m): 10.24 MB on disk, 6.52 MB on the wire; `app-np-100` (wards at 100 m): 9.19 MB, 5.51 MB on the wire. Beside it `app-world` (the world without Nepal in full, no outlines): 5.48 MB, 3.13 MB on the wire. `admin_at` names the ward OSM names at Kathmandu-1, Kirtipur-1, Pokhara-9 and Biratnagar-11 |
| TG-28 | Nepali and Hindi names for the world's cities | partly there: the app pack carries GeoNames and CLDR/Wikidata labels in ne and hi | Measured 2026-11-17 on the app pack: लण्डन, दिल्ली, टोकियो, न्यूयोर्क, पेरिस find their cities; लन्डन (half न) does not, and लंदन is the Hindi form. Needs Devanagari spelling folds in the search keys (nasal forms, ण/न) from the lexicons, and the Wikidata labels as alternates, not only fallbacks |
| TG-29 | Hindi and Sanskrit names across Nepal's pack | partly there: the app pack has Sanskrit on 121 Nepal places | No pinned source covers Sanskrit widely; Hindi follows Wikidata labels. Coverage grows with sources, measured per release |
| TG-30 | One ID for a place in lite and main | answered (D-368): two lines, bridged | The lines cannot merge without reusing released IDs (ADR-0008), so each stays stable on its own and a pack's `licence` names its line. `np-ids-bridge.tsv` in data-2026.11.22 maps 95,698 of 95,716 live lite IDs to main (16 ties, 2 with no shared key); from the next release, `ids-bridge.tar.gz` holds a bridge for every namespace |
| TG-31 | One reviewed spelling across a place's levels | released (D-377): काठमाडौं on every level, in [data-2026.11.22.2](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.22.2) | Kathmandu is काठमाण्डौ on the settlement and काठमाडौँ on the metropolitan city. The official lists disagree (checked 2026-11-17): the Election Commission's 2079 lookups write the metropolitan city काठमाण्डौ and the district काठमाडौं; CBS writes the local level काठमाण्डौ and the district काठमाडौँ. The chosen form becomes canonical on every level, the others alternates |
| TG-32 | Landmarks as their own class | released (D-381): a landmark is a `locality` with a `class` (`palace`, `temple`, `airport`, …), in [data-2026.11.22.2](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.22.2) | Narayanhiti Royal Palace is a settlement. The schema has seven kinds and no landmark; GeoNames' spot features need their own kind or a mapping to `locality` |
| TG-33 | The coarse terrain grid as an optional layer pack | released: [data-2026.11.22](https://github.com/teispace/teigeodata/releases/tag/data-2026.11.22) | `dem-coarse-np-lite.tgd`, a layer of no rows: 527,424 B on disk, 518,561 B on the wire (NASADEM, from the lite build, so it stacks with either Nepal pack) |
| TG-34 | The app layer under its own name and release | done (73296e3), released in data-2026.11.22 | A profile's `address` names the layer; `forge embed --release` gives its release: `app`, `app-world`, `app-np`, `app-np-100` and `dem-coarse-np`, each release 2026.11.22 |

## Questions back

- TG-02: the codes come from a per-entry call, `iso_code(&place)`, not inside `admin_at`'s and
  `ancestors`'s answers. Does a per-entry call meet the request? It keeps `Place` the same size
  for every user.
- TG-06: the brief asks for strict tiers (exact, then prefix, then substring, then population).
  Our search uses one additive score (D-052): strict tiers lost recall on prefix queries, so an
  important place found by a substring can rank above an obscure exact match. A substring
  match scores below prefix and one-edit matches. Is that acceptable, or does your UI need an
  exact match always first? We can offer a strict-tier option if it is needed. Answered by
  D-226: `SearchOptions::with_strict(true)` ranks by tier first; the default stays the additive
  score.
- TG-06: "3 characters" is read as 3 Unicode code points (काठ is 3 code points, 2 graphemes),
  as the rest of our search counts.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-11 | First reply: every TG placed in the plan | Tithipatra's brief (D-193) |
| 2026-11-11 | TG-06 done; both TG-06 questions stay open | D-199 |
| 2026-11-11 | Addendum 1 accepted and placed; TG-11 done | D-213 |
| 2026-11-17 | Addendum 2 placed: TG-20 to TG-26 | D-362 |
| 2026-11-17 | TG-27 to TG-29 added; the consumer's run-time mirror is its public download bucket | Their message, measured on the app pack |
