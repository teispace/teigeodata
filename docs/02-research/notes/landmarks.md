---
title: "Landmarks: a class, not a kind"
status: accepted
updated: 2026-11-22
owner: agents
revisit: 2027-05-22
---

# Landmarks: a class, not a kind

## 1. Question

TG-32: Narayanhiti Royal Palace reads as a settlement. How should a palace, temple or airport
be told apart from a settlement, without breaking packs or readers that 1.0 promised
([semver](../../06-engineering/semver.md))? Feeds D-381.

## 2. Summary and recommendation

Keep landmarks as `locality` and give them a **class** from one growable list in
`teigeodata_core::class` (palace, temple, monastery, shrine, religious_site, monument, museum,
historic_site, castle, ruin, pagoda, mosque, church, airport). The class column already exists
in PLACES (format 1.0) and in the extract, so no format change, no new kind, and every 1.x reader
still opens the packs. `Engine::class(&place)` answers it, and every binding's place blob gets a
nullable `class` column.

## 3. Findings

- The kind is a closed list of seven codes; a 1.x reader refuses a kind code above 7 (the 1.0
  freeze, D-335). A new kind would therefore be a 2.0 change.
- PLACES carries an optional CLASS column of dictionary codes (schema.md, `class`), unused until
  now. A pack without it reads every class as none; a code a reader's list does not have reads
  as none, not as an error. A newer list can grow without breaking an older reader.
- GeoNames' spot features (`S`) carry the codes that decide it. Nepal, measured 2026-11-22 on
  `NP.zip`: 238 rows under the mapped codes, of them TMPL 159, AIRP 46, MSTY 10, SHRN 4, PAL 4,
  RLG 3, FT 3, RUIN 2, CTRR 2, and one each of HSTS, ANS, MUS, MNMT and CH (Kathmandu Durbar
  Square, the National Museum, Narayanhiti Royal Palace among them). Hotels (HTL 141), schools,
  bridges and posts are not landmarks and stay out.
- A cluster with an administrative record, or a populated place other than a locality (PPLL,
  PPLX), is not a landmark: a palace's name shared with a village must not turn the village into
  a landmark.

- GeoNames repeats many landmarks as an unpopulated locality (`P.PPLL`) of the same name
  nearby. Nepal, measured 2026-11-22: 55 same-name pairs of a landmark and such a locality, all
  within 929 m (Narayanhiti Royal Palace 210 m, Swayambhunath 247 m, Meghauli Airport 480 m);
  the next pairs are 2 km and more apart and are other places. Conflation never merges two
  records of one source, so CI's first build had both: the palace as a settlement and as a
  locality. A place held only by such localities now joins the nearest same-name landmark
  within 1 km (`dedupe::join_spots`) and its ID resolves to it. A populated `P.PPL` of the
  landmark's name (Pashupatinath, 356 m from the temple) is a settlement and stays.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A new kind `landmark` | one field to read | 1.x readers refuse the packs: a 2.0 change | no |
| `locality` plus a class | no format change; one list for every country; grows | callers read a second field | **chosen** |
| A separate landmark layer pack | apps that skip it save bytes | a second ID space; search across two packs for one place | no |

## 5. What we build ourselves, and why

The class list and the GeoNames mapping (`read::geonames::landmark_class`), and the cluster rule
(`build::cluster::landmark`). Tested in the forge (cluster rule), the engine (`tests/class.rs`:
known codes, an unknown code, a pack without the column) and the API (the blob's column list in
`bindings/parity.json`). The count per country is read from the next data release's report.

## 6. Impact on the plan

TG-32 closes in phase 8 (D-381); the landmarks reach users in the next Nepal data release. The
planned settlement classes (`city`, `town`, `village`) and terrain classes (`peak`, `pass`) join
the same list later, with no format change.

## 7. Risks and open items

- Main's Overture places carry their own categories; mapping them to the list is open until
  main's world part is rebuilt.
- GeoNames' spot rows are sparse outside a few countries, so most countries start with few
  landmarks. OSM's `historic`, `tourism` and `amenity` tags are the next source.
- A locality spelled apart from its landmark does not join it: GeoNames' "Tribhuwan
  International Airport" (`P.PPLL` 7962278) stays a settlement beside the airport "Tribhuvan
  International Airport" (`S.AIRP` 1282644), 600 m apart. A `same-as` correction joins such a
  pair (ADR-0036); measured after data-2026.11.22.2, in the next Nepal release.
- A shrine or temple sharing a village's point and name stays a village (the cluster rule), so a
  few landmarks are missed rather than villages lost.

## 8. Versions checked

- teigeodata pack format 1.0 (D-335), PLACES' class column.
- GeoNames `NP.zip`, downloaded 2026-11-22 (feature codes as of that day).

## 9. Sources

- GeoNames feature codes: https://www.geonames.org/export/codes.html
- [Schema: `class`](../../05-data/schema.md)
- [What 1.0 promises](../../06-engineering/semver.md)
