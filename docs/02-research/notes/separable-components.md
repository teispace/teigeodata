---
title: "Separable components: names by language, outlines by level and level of detail"
status: accepted
updated: 2026-11-12
owner: maintainer's agents
revisit: 2027-05-12
---

# Separable components

## 1. Question

D-213 asks that everything be separable: names split by language and outlines by admin level
and level of detail, each its own component with its wire size in the catalog, so a client or
a profile picks exactly the parts it wants. Today a component is a fixed set of section kinds
(D-135), and each kind has at most one section in a pack. How is a pack split further, with one
signed file, verified ranged installs, and no cost to clients that take everything? This feeds
4.9c's "everything separable" step and [ADR-0040](../../03-decisions/adr/ADR-0040-section-parts.md).

## 2. Summary and recommendation

- **Section parts.** A section kind may appear more than once in a pack, each copy a *part*
  named by a key of two `u32`s in the section table entry (today's 8 reserved bytes). Part
  `(0, 0)` is the base part, which is all of today's sections. The table is sorted by kind, then
  part. Everything a part needs to be found and checked (hash, length, block hashes) is
  already per section, so ranged installs, partial installs (D-136) and updates by section
  (D-174) work on parts with no new trust path.
- **What a part key means is fixed per kind**, so the table describes itself and no manifest
  table is needed: for NAMES, SEARCH and SUBSTRING `(language, 0)` with the language packed as
  NAMES' `lang` column packs it; for ARCS `(level group, level of detail)`; for POLYGONS and
  POLY_INDEX `(level group, 0)`; for `ADMIN_REGION` `(deepest level, level of detail)`.
- **Names and search by language.** The base part keeps every place's canonical name and the
  names with no language, and the search keys of those names with the importance, rules and
  unit columns. A language whose names reach 128 KiB of text gets its own NAMES and SEARCH
  part (16 KiB, one hash block, was the first plan; see §7); the rest share one part keyed `*`. A client that wants a language
  takes its part, or the `*` part when the language has none.
- **Outlines by level and level of detail.** Arcs are grouped by the coarsest admin level that
  uses them (an arc counts once), and each group is stored at each level of detail the build
  publishes, as full copies (simplification keeps the topology per level already, D-229).
  A client takes the groups down to the level it wants, at one level of detail.
- **Build in steps**, each measured and gated: (1) the format's parts and the component model
  of parts; (2) names and search by language; (3) outlines by level group; (4) levels of
  detail; (5) the catalog's per-part sizes, `tgd install` of parts, and profiles that pick
  parts.

What it buys, measured 2026-11-12 (wire bytes, zstd 19):

| Choice | Takes | Saves |
|---|--:|--:|
| Nepal lite, names: canonical and English only | 438 kB of 1,369 kB | 0.93 MB |
| Nepal lite, names: canonical and Nepali | 916 kB of 1,369 kB | 0.45 MB |
| `tithipatra`-sized layer, English only, names and search | 2.78 MB of 4.28 MB | 1.50 MB |
| Nepal outlines, down to districts | 0.99 MB of 2.46 MB | 1.47 MB |
| Nepal outlines, down to provinces | 0.47 MB of 2.46 MB | 1.99 MB |
| Nepal outlines at 20 m (21.4% of vertices) | about 0.5 MB of 2.46 MB | about 1.9 MB |

## 3. Findings

**The section is already the unit of download** ([components](components.md) §2): the
transport plans the frames of the sections a set of components needs (`Frames::plan_sections`,
D-112), each section is checked by its hash in the signed head, and sections of 1 MiB or more
by 16 KiB block hashes (D-078). A part that is a section inherits all of it.

**The head has room for a few hundred sections, not thousands.** The head is at most 16 KiB
and a table entry is 80 B, so about 190 sections fit beside the manifest. Nepal lite carries
names in 150 languages; a part per language for NAMES and SEARCH would take 300 entries. The
16 KiB rule exists so that a client reads the head in one small request, and it stays. Hence
the threshold: a language gets a part when its text is at least one hash block (16 KiB), which
on Nepal lite is about ten languages; a part smaller than a block saves no request.

**Names by language, Nepal lite 2026.11.0** (`cargo xtask pack-sizes`, 224,514 records, NAMES
1,344,116 B zstd): `und` 112,416 records on 100.0% of places (1.04 MB UTF-8); `ne` 96,682 on
99.9% (2.16 MB, most filled by the transliterator, D-228); then `mai` 63 kB, `new` 39 kB, `ur`
30 kB, `bho` 25 kB, `bg` 24 kB, `hi` 15 kB, and about 140 languages under 16 kB. Cut to the
canonical name plus `en`: 438,418 B; plus `ne`: 916,299 B; plus both: 926,645 B.

**Search by language**, measured by two temporary profiles over the world part and all of
Nepal (170,882 rows), names and search wire bytes: English only 905,012 + 1,878,189; Nepali
and English 1,452,284 + 2,467,938; `tithipatra` (Nepali, Hindi, English) 1,621,094 + 2,655,792.
Nepali adds 0.55 MB of names and 0.59 MB of keys; Hindi 0.17 MB and 0.19 MB.

**Outlines, Nepal lite** (2,315 arcs, 593,610 vertices, ARCS 2,459,143 B zstd): by the
coarsest level using each arc, country 65,786 vertices, provinces 48,089 more, districts
125,357 more, local levels 354,378 more (cumulative wire estimate 273 kB, 472 kB, 991 kB,
2,459 kB). Levels of detail kept: 5 m 51.0%, 20 m 21.4%, 100 m 7.3%, 500 m 2.2%.

**Precedent for packs per part.** The catalog already publishes a second pack for an address,
`np.dem` (`address.component.tgd`, ADR-0010). That works for one large optional part; for
languages it would mean a signed asset, a catalog entry and a cross-pack row join per language.

**A bug found on the way:** `pack-sizes --langs ne-Deva` matched no Nepali record because it
compares the whole tag to the language column; profiles name languages with a script
(`ne-Deva`, D-215). Fixed with step 2, which needs the same matching.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| (a) Section parts in the one pack, keyed in the table entry | One signed file and asset; ranged, partial and delta installs unchanged; the table describes itself; clients that take all pay a few hundred bytes of entries | Format change (table order, entry layout); readers handle several sections of a kind | **Chosen** |
| (b) A pack per part (`np.names-ne.tgd`, like `np.dem`) | No format change | A signed asset, catalog entry and release per language and level; a new cross-pack row join in the engine; updates must move together | Rejected |
| (c) Profiles only: parts cut at build time, a few fixed variants in the catalog | Already built (D-214) | A client cannot pick any set; every new combination is a new asset | Kept for embedded layers, not for downloads |
| (d) Parts as byte ranges inside one section | No table change | Readers must handle holes inside a section; parts must align to frames and blocks; the section hash no longer proves a partial install | Rejected |

For outline levels of detail:

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Full copies per level of detail | Each part stands alone; readers unchanged; the topology-safe levels exist already | The asset grows by the sum of levels (about +80% of ARCS for 5, 20 and 100 m) | **Chosen**: the asset's size costs hosting, not the client |
| Progressive refinement (coarse vertices, then the vertices each finer level adds) | No duplicated vertices | Readers merge streams at run time; arcs are only usable once every coarser part is in | Rejected for now |

## 5. What we build ourselves, and why

The design reuses our own section, hash and frame machinery; nothing outside it is needed.

Proof each step must give:

- **Format:** a pack with parts round-trips; a reader that is given only some parts answers
  from them; a table out of (kind, part) order, a duplicate key, or a part key on a kind that
  takes none is refused (planted faults).
- **Names and search:** a layer with every part answers every golden case exactly as today
  (G-golden-tithipatra, golden lite r@1); with only the base and `en` parts, Nepali queries
  find nothing and English ones still pass; the parts' wire bytes add up to the asset (G-wire).
- **Outlines:** `admin_at` with groups down to level *k* answers the deepest unit of level at
  most *k*, equal to the full pack's chain cut at *k*, at every place and on a grid (the
  G-tz-layer method).

## 6. Impact on the plan

- [ADR-0040](../../03-decisions/adr/ADR-0040-section-parts.md): section parts.
- Phase 4, 4.9c "everything separable" gets the five steps of §2.
- The pack format page gains the part key and the table order; the components page and the
  engine's installed set work on parts; the catalog lists each part's wire size.

## 7. Risks and open items

- The `*` part mixes languages: a client that wants one rare language takes all rare ones
  (at most a few hundred kB on Nepal).
- Search ranking across parts: a row found by keys in two parts must score once, at its best
  tier. Step 2's golden runs prove it: golden lite unchanged class by class (D-248).
- **Every part held costs its own lookups** (measured with step 2, D-248, Nepal lite, every
  part held): 8 parts at a 16 KiB threshold, search p50 6.8 → 13.0 µs; 3 parts at 128 KiB,
  7.2 → 11.1 µs. Hence 128 KiB. A per-part filter (skip a part with no key of the query's first
  character) would cut the rest; a client holding fewer parts pays less.
- Full copies per level of detail grow the release asset and the host's storage. Measured
  with step 4 (D-250, Nepal lite): eight parts at 20 m and 100 m add 1.4 MB to an 87.5 MB pack
  and cut a client's outlines from 3.29 MB to 0.64 or 0.26 MB wire; drop a level no profile or
  client asks for.
- **Step 4 lesson:** the coarse outline path already existed (`extract::merge` for the embedded
  layers, with snapping, labels moved inside and boxes from the coarse arcs). Exposing it as
  `extract::outlines` made the parts without a second simplifier. A coarser level changes
  `admin_at` only near borders: Nepal at 100 m, 1,032 of 427,014 points for municipalities, all
  within 100 m of a border the whole pack draws.
- The head limit caps parts at about 190 sections per pack; the writer and the reader already
  refuse a head over 16 KiB (`HEAD_MAX`).

## 7a. Step 3 design: outlines by level group (2026-11-12)

**Superseded by what was built (D-249):** each outline part is a whole copy over levels 0 to
*k*, re-encoded on only the arcs those levels use, and a reader takes one part. The analysis
below is kept because it shows why: a reader joining arc parts would dispatch on every vertex
read, through ADMIN_REGION's global vertex numbering and the `Copy` `Arcs` inside ring
iterators. Measured (Nepal lite, wire): all levels 3.29 MB, to districts 1.31 MB, to provinces
0.64 MB, the country 0.36 MB; the asset +2.31 MB.

The arc-group design as first written:

- POLYGONS refers to arcs by their global index (`2·arc + reversed`), and ADMIN_REGION numbers
  its edges by global ARCS vertex (edge `e` joins vertices `e` and `e + 1`). So the arcs are
  **ordered by group** in the build, coarsest first (group = the coarsest admin level using the
  arc), and ARCS part `g` holds group `g`'s arcs: parts 0 to *k* are then a prefix of today's
  numbering, and every index a coarser part stores stays valid when finer parts are absent.
  Ordering changes the bytes of every pack with outlines once; the arcs' content does not.
- **Readers join parts**: an `ArcSet` over the ARCS parts present maps a global arc (and
  vertex) to its part by a binary search on each part's first arc (and vertex), so
  `Polygons::ring`, `Region::open_on_arcs` and `PolyIndex::check` take the set where they take
  `Arcs` today. With one part it is today's reader.
- **POLYGONS and POLY_INDEX by level**: part `g` holds the polygons of level group `g` (POLYGONS
  already groups them coarse to fine, so a part is one group); POLY_INDEX part `g` indexes
  that part alone (the tree already restricts queries to one group's range).
- **ADMIN_REGION per deepest level**: part `k` is a whole region over the polygons of levels 0
  to *k* on arcs 0 to *k* (its sets differ from the full region's, so it cannot be cut from it).
  A client takes the one part for its deepest level; the base part stays the full region, so
  a client that takes everything is unchanged. Measure the copies' sum against the 977 KB of
  Nepal's full region before choosing.
- **Gate:** `admin_at` with parts down to level *k* equals the full pack's chain cut at *k*, at
  every place and on a 0.01° grid (the G-tz-layer method); golden and layer gates unchanged.
- **ARCS_OVERVIEW** follows ARCS's parts, and levels of detail (step 4) add the `lod` byte.

## 7b. Step 5: choosing parts (2026-11-13, D-251)

- **One selection type for every reader** (`component::Parts`), beside `Components`: the engine,
  the installer and the catalog's size sums take the same value, so what is downloaded is what
  is read. Fixed size and `Copy` so the no_std installer can hold it.
- **Languages resolve against the pack**: a client asks for languages, not parts, and a
  language without its own part lives in `*`. The table names every part, so resolving needs
  only the head.
- **One outline part, chosen outright**: parts are whole copies, so a client installs exactly
  one; the engine reads that one whatever its level and tolerance options say.
- Measured on Nepal lite: English only keeps every Latin-script class of golden lite and saves
  31% of the names, search and substring wire.
- **Choose outlines by bytes, not by key** (D-254): a client asks for a level and a tolerance;
  of the parts that hold it, the smallest on the wire wins, the base included. A part with more
  levels can be the smaller (it is simplified further), so matching the exact level would
  fetch more. Levels are numbers on the command line; their local names are country data.
- **The catalog carries what a choice needs** (D-253): each part's bytes and an outline's
  tolerance, so a client picks and predicts an install from the signed metadata alone, before
  any pack byte. Compact arrays keep Nepal lite's 28 parts at 1,484 B per entry.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Pack format | 0.1 | 2026-11-12 | [pack format](../../04-architecture/pack-format.md) |
| Nepal lite pack | 2026.11.0 | 2026-11-12 | `data/build/pack/np/` |

## 9. Sources

- [Components](components.md), [embedded layer](embedded-layer.md),
  [download and delta](download-and-delta.md): this repository.
