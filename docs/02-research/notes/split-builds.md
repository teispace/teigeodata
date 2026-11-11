---
title: "Split builds: packs for parts of a country, with their ancestors"
status: accepted
updated: 2026-11-11
owner: forge, engine
revisit: 2027-02-01
---

# Split builds: packs for parts of a country, with their ancestors

## 1. Question

ADR-0010 gives packs deterministic addresses: an admin code (`np`, `ae-du`), split into fixed
grid tiles for very large units (`in-br-t07`). D-143 makes the hierarchy cross packs by
**ancestor stubs**: every pack holds its places' ancestors as rows. The engine side is built and
gated (the metamorphic gate splits packs in memory, D-114, D-143). Phase 4 still owes the forge
side: "split builds (city packs, grid tiles) write their places' ancestor stubs, gated by
check-pack". What must a split build write, where do its addresses come from, and how is it
proved equal to the whole pack?

## 2. Summary and recommendation

A split build writes, from the same places as the country pack, one pack per unit of a chosen
admin level, and tiles a unit only when its pack would pass a byte budget. Each part holds its
own places, every ancestor of them as a row (the stubs), the timezone zones its box meets, its
own units' geometry, the postal codes of its rows and the redirects whose survivor it holds.
IDs come from the country's one registry, so a place has the same ID in the country pack and in
its part.

- **Addresses are data** (P16): `data/countries/<cc>/split.yaml` maps each unit of the split
  level (by the admin source's key) to its ISO 3166-2 subdivision code, with the code's source.
  The forge refuses a unit with no code. No code is derived from a name.
- **Tiles** for a unit over the budget: cells of one fixed world grid, named by the cell, not by
  a count, so an address never changes meaning between releases (**proposal**: decided with
  India's numbers, W1).
- **One code path:** the pack stage builds every section from a slice of place rows; a part is
  the same stage over its rows. No second encoder.
- **Gate:** `check-pack --parts` (or a forge read-back) proves that for every place of the
  country pack, the part holding it gives the same `get`, the same `ancestors` chain by ID, and
  that the union of parts' `children` equals the whole's. A part whose ancestor stub is missing
  fails.

Nepal ships whole (one pack, under every budget), so the first split country is India (W1,
"per state, tiled"). The machinery is built and gated on Nepal by province now, so W1 is data
work.

## 3. Findings

### 3.1 How others split

- Organic Maps cuts countries along hand-kept border polygons (`data/borders/*.poly`), one map
  file per polygon; large countries are split into many regions, and an issue asks for more
  splits of the largest files. Border files are curated, not derived from admin codes.
- Geofabrik offers India whole and in six zones (central, eastern, north-eastern, northern,
  southern, western); states are a do-it-yourself split.
- Neither gives a stable, computable address per point; ours must, because the locator maps a
  point to its pack address offline (ADR-0010).

### 3.2 What the repository already has

- The engine follows IDs across layers: `parent` falls through to the first layer holding the
  ID with a parent; `children` is the union by ID (D-143). Search contexts match ancestors in the
  row's own pack, which holds them as stubs (D-119).
- `missing_packs` already counts a partial pack (`ae-du`) where its box holds the point.
- The forge's pack stage (`forge/crates/forge/src/build/pack.rs::write`) builds every section
  from `rows: &[&Place]` in Hilbert order and a key-to-row map, then signs and reads back.
- Natural Earth admin 1 carries ISO 3166-2 codes (D-150), but a pack country's own units in the
  baseline come from its admin source (its slice), so codes must be joined to that source.
- `format::extract` (D-180) cuts a box with ancestors but leaves out REGION, POSTAL, REDIRECTS,
  COMPONENTS and BLOBS: right for a user's extract, not for a release.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Split in the pack stage, per part rows (ours) | one encoder; every section; IDs and registry shared; read-back per part | the stage runs once per part | **chosen** |
| Split a built pack with `format::extract` | exists | drops five sections; re-encodes from a pack, not from records | refused for releases |
| Border polygons per region (Organic Maps) | flexible | curated files; no address from a point | refused |
| Addresses from names | no data file | unstable, script-dependent | refused (P16, data is checked) |

## 5. What we build ourselves, and why

1. `split.yaml` per country: `level`, `units: { <admin key>: <iso 3166-2 code> }`, `source`.
2. `forge make <cc> --split`: after the country pack, for each unit, rows = places whose
   level-`level` ancestor is the unit, plus every ancestor of those rows; zones meeting the
   part's box; geometry of the unit and its descendants; postal codes linked to its rows;
   redirects whose survivor it holds. Address `<cc>-<code>` lowercased, `BBOX` the part's own
   places, the same release and attribution. Places with no ancestor at that level (if any) go
   to a part named in `split.yaml` (`rest`), never dropped; the forge counts and reports them.
3. The gate of §2, planted faults: a stub left out, a place in two parts, a part missing a zone.
4. Each part goes through `check-pack` like any pack (G-lic, G-wire).

## 5a. What building it on Nepal taught (2026-11-11)

- **191 places reach no province:** border towns whose points are just outside COD-AB (Sunauli,
  Darchula) and former development regions (GeoNames ADM1H), all under the country. They go to
  the unit whose box of own places is nearest. A `rest` part was refused: the locator maps a
  point to the unit holding it, so such a part would never be fetched.
- **Whole zone rings make parts 2.3× the pack:** every part carried the full Asia/Kathmandu,
  Asia/Kolkata and Asia/Shanghai rings (REGION 1.86 MB in a 6,621-place part). Rings are now cut
  to the part's box grown by 0.05°; REGION fell to 156 kB there. Stub outlines (the country's)
  were dropped from parts too. Parts now total 13.49 MB for an 11.59 MB pack.
- **G-tz is a country gate:** a province with border records reached 0.102% against 0.1%; the
  same records pass in the country pack. Parts report it and `check` proves their areas.
- **The gate checks coverage too:** every row of the country pack is in a part, except a zone
  that holds no place (`Etc/GMT-5`, `Etc/GMT-6` at sea).

## 6. Impact on the plan

- Phase 4 §4.1 item "Forge: split builds write their places' ancestor stubs": built by §5 on
  Nepal by province; India's tiles and budget are W1.
- The catalog lists each part as its own target, as any pack.

## 7. Risks and open items

- The tile grid and the budget that triggers tiling (**open**, W1 with India's sizes).
- A unit whose places' ancestors are in another unit (an enclave linked across a border): its
  stubs carry the other unit's rows, which is what D-143 wants; geometry stays with its owner.
- Parts duplicate their ancestors' rows and names: a few rows per part (chains are 3–6 deep).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| ISO 3166-2:NP | provinces P1–P7 | 2026-11-11 | to be pinned in `split.yaml` with its source |

## 9. Sources

- [Organic Maps maps generator](https://git.omaps.dev/pastk/organicmaps/src/branch/master/tools/python/maps_generator)
  and [issue 3977, splitting huge regions](https://git.omaps.dev/organicmaps/organicmaps-tmp/issues/3977),
  accessed 2026-11-11.
- [Geofabrik: India](https://download.geofabrik.de/asia/india.html), accessed 2026-11-11.
- ADR-0010, D-114, D-119, D-143, D-150, D-180 in this repository.
