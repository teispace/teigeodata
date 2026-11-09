---
title: "Attribution in every pack"
status: research
updated: 2026-11-09
owner: forge
revisit: 2027-05-01
---

# Attribution in every pack

## 1. Question

Every pack is a database derived from its sources, so each source's licence terms travel with
it (ADR-0002, ADR-0030, D-031). What must a pack carry, where in the file, and how does a build
know which sources reached it? Feeds D-126 and phase 3 step 3.7.

## 2. Summary and recommendation

Each source in `data/sources.lock` states one `attribution` line (required, at most 512 bytes,
no control characters; `cargo xtask check-data`). The build's licence gate records every source
it lets the build read, the `--core` buildings run included, and the pack carries their notices,
once each in reading order, in the manifest tag `0x0008 attribution` (UTF-8, at most 8 KiB). A
copy is written beside the pack as `attribution-<build>.md`. The manifest is in the signed head,
is read without any section or the `zstd` feature, and `Layer::attribution()` returns it. The
main pack also states `licence` `odbl`, the lite pack `cc-by-4.0` (ADR-0002).

Nepal 2026.11.0: main 10 notices (1,032 B), lite 5 (425 B).

## 3. Findings

What each licence asks of a redistributor of a derived database:

| Source | Licence | What the notice must do |
|---|---|---|
| OpenStreetMap (Geofabrik), timezone-boundary-builder | ODbL 1.0 | Make anyone using the data aware it came from OpenStreetMap under the ODbL (§4.2–4.3); the OSMF asks for "© OpenStreetMap contributors" with a link to the copyright page |
| GeoNames, Google Open Buildings, WorldPop | CC BY 4.0 | Credit the creator, name the licence and link it |
| OCHA COD-AB Nepal | CC BY-IGO 3.0 | As CC BY; HDX credits the Survey Department of Nepal and the UN Resident Coordinator's Office in Nepal |
| Microsoft building footprints | CDLA-Permissive-2.0 | §2.1: make the agreement's text available with shared data; attribution itself is optional. Its FAQ shows a link to the agreement beside the data as one way |
| Copernicus DEM GLO-30 | Copernicus DEM licence | A fixed notice, recorded verbatim |
| Wikidata | CC0 1.0 | Nothing required; credited as a courtesy |

The ODbL's produced-work notice must be "reasonably calculated" to reach whoever sees the data,
so an application shows the notices wherever it shows the data; the pack's job is to carry them
signed and complete.

## 4. Options compared

| Option | Read without | Signed | Cost | Verdict |
|---|---|---|---|---|
| Manifest tag (UTF-8 text) | any section, `zstd` | yes, the head digest | 1.0 KB of the 16 KiB head on Nepal main | **use** |
| A BLOBS entry with a manifest pointer | – | yes, the section hash | needs the `zstd` feature to read; a reader built without it cannot show the notices it must show | reject |
| Only a file beside the pack | – | no | lost when the pack is copied alone | keep as a copy, not the carrier |

## 5. What we build ourselves, and why

The notices come from the licence gate, the one place that decides what a build reads, so a
source the gate refuses can never be credited and a source it reads can never be missed. The
gate now also checks the `--core` buildings run: before, only `forge make` filtered the lite
build's buildings, and `forge build --lite --core <run with OSM>` would have read ODbL
buildings.

## 6. Impact on the plan

D-126. Phase 3 step 3.7 attribution done for the pack; the API and bindings expose
`attribution()` with the rest of a layer's metadata (phases 5–6); a repository-level
`ATTRIBUTION.md` for releases is generated from the same lock fields when releases start.

## 7. Risks and open items

- The `licence` tag was checked as an identifier with no dots, so `cc-by-4.0` (the documented
  lite value) could never be written; no pack carried the tag. It now takes the release rule
  (dots allowed).
- COD-AB's licence is recorded as CC BY-IGO 3.0; HDX shows it on the edge-matched sibling
  dataset, so recheck the cod-ab-npl page at the next pin.
- 8 KiB holds about 80 notices of this length (103 B on average): a pack built from more sources needs shorter
  notices or a BLOBS overflow with the head keeping a summary line.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| CDLA-Permissive | 2.0 | 2026-11-09 | spdx.org, cdla.dev |
| OSMF attribution guideline | adopted 2021-06-25 | 2026-11-09 | osmfoundation.org/wiki/Attribution |

## 9. Sources

- [Community Data License Agreement Permissive 2.0](https://spdx.org/licenses/CDLA-Permissive-2.0.html), SPDX.
- [CDLA compatibility FAQ](https://cdla.dev/faq-resources/compatibility/), Linux Foundation.
- [Attribution guidelines](https://osmfoundation.org/wiki/Attribution), OpenStreetMap Foundation.
- [Licence and legal FAQ](https://osmfoundation.org/wiki/Licence_and_Legal_FAQ), OpenStreetMap Foundation.
- [Nepal – Subnational Administrative Boundaries](https://data.humdata.org/m/dataset/cod-ab-npl), OCHA HDX.
