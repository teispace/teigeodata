---
title: "Suite C: the competitive report"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-05-15
---

# Suite C: the competitive report

## 1. Question

The benchmarks page promises a competitive report with each release
([benchmarks](../../06-engineering/benchmarks.md#c-competitive-published-with-each-release)),
and phase 8.3 asks for it on GitHub runners, by dispatch (D-331). Against which rivals, on
which questions, measured how, so that the numbers are fair, repeatable and checkable by the
rivals' maintainers?

## 2. Summary and recommendation

One workflow, `competitive.yml`, by dispatch (and on a release), on one `ubuntu-latest`
runner. It sets up each rival from pinned inputs, runs one harness (`tools/competitive`, its
own Cargo workspace) over the same query files, and uploads a Markdown report with every
rival's version, configuration, setup time, size and memory.

| Rival | Version (2026-11-15) | What it answers | Set up from |
|---|---|---|---|
| teigeodata, in process | this commit | everything below | the embedded `world-np` pack of the newest data release (the world part and Nepal in one file; main once published, else lite, and the report says which) |
| teigeodata, `tgd serve` | this commit | search, reverse over HTTP | the same pack |
| Nominatim | 5.3.3 (`mediagis/nominatim:5.3`, by digest) | search, reverse | the pinned Geofabrik Nepal extract, from our mirror (the same bytes the main build reads) |
| Photon | 1.3.0 (jar by SHA-256) | search, reverse | GraphHopper's Nepal dump (`asia/nepal/photon-dump-nepal-1.0-latest.jsonl.zst`, its SHA-256 in the report), names in English and Nepali |
| GeoNames | the pinned `NP.txt` | where a GeoNames-backed library puts a place | the source the sample was drawn from |
| `tzf-rs` | 2.3.0 | timezone at a point | its bundled data |

The questions, each on files already in the repository:

1. **Where is the place?** The 222 labelled places of
   [`tests/golden/np-places.csv`](../../../tests/golden/np-places.csv) (D-209, D-339): each
   rival is asked for the name in Nepal, and its first answer is measured against the label.
   Found (first answer within 5 km), and P50 and P90 distance over the found. This is the
   question users ask, scored on independent truth.
2. **Search recall:** the 521 queries of
   [`tests/golden/np.yaml`](../../../tests/golden/np.yaml) by class (exact, prefix, typo,
   native script, variant, ambiguous …): recall@1 and @5, where a hit is an answer within 2 km
   of the expected place's point. A distance, not an ID, because rivals have their own IDs.
3. **Timezone at a point:** 20,000 seeded random points, uniform on the sphere; agreement of
   teigeodata's `tz_at` with `tzf-rs` over the points teigeodata answers (`tzf-rs` answers
   the sea too), the commonest disagreements, and ns per call for each.
4. **Latency:** median and P95 per query kind, in process for libraries, over loopback HTTP
   for servers (teigeodata's own server included, so the comparison is like for like), one
   client, after a warm-up pass.
5. **Footprint:** setup time, size on disk (pack bytes; Nominatim's database; Photon's index),
   resident memory after the run.

## 3. Findings

- Nominatim's Docker image takes `PBF_URL` or a mounted `PBF_PATH`; a country extract imports
  in tens of minutes on 4 cores. Mounting our mirrored extract makes the import reproducible
  and the same data as the main build's OpenStreetMap input
  (`mediagis/nominatim` README, 2026-11-15).
- Photon 1.x imports a JSON dump: `zstd -d dump | java -jar photon.jar import -import-file -`,
  then `serve` on port 2322 (Photon `docs/usage.md`, 2026-11-15). GraphHopper publishes a
  Nepal dump of 28.6 MB; only `latest` files are kept, so the report records its SHA-256 and
  date.
- GeoNames has no query service to run offline; the libraries built on it (for example
  `reverse_geocoder`) return GeoNames' own point. The source point is therefore what such a
  library answers, and is scored as one rival.
- GitHub's runners vary; one runner class, medians of repeated runs, and the runner's CPU in
  the report.

### 3a. The first local run (2026-11-15)

In process and against `tgd serve`, on Nepal lite and the world baseline (the rivals run on
GitHub only; Nominatim's import alone takes tens of minutes):

| Measure | teigeodata, in process | `tgd serve` |
|---|--:|--:|
| Labelled places found by name (first answer within 5 km) | 140 of 222 (63%) | 140 (63%) |
| P50 / P90 over the found | 0.43 / 1.23 km | 0.43 / 1.23 km |
| Golden recall@1 / @5 (450 queries resolved) | 88% / 96% | 88% / 96% |
| Search median / P95 | 53 / 529 µs | 243 µs / 1.3 ms |

- **The two agree exactly** once in-process hits are kept to the country's namespace, as the
  server's `countrycodes` keeps them (asked for 40, the server's most, then filtered). Before
  that filter the in-process run found 131: names shared with places abroad. The agreement is
  a differential check of the server path.
- **By identity** (the same place's own record): teigeodata P50 0.26 km, P90 0.94 km, 65%
  within 500 m; GeoNames' own point P50 0.39 km, P90 0.89 km, 61%. By name, a third of the
  places are not found within 5 km: Nepal's place names repeat (the sample alone has several of its names more than once), and the
  first answer is the most important of the same name. The same question to every rival.
- **Timezones:** the lite world part answers 5,675 of 20,000 random points (land) and agrees
  with `tzf-rs` on 86%. The disagreements are units a zone border crosses, as D-151 and D-245
  intend for a layer without a timezone index: Antarctica (one zone for the continent),
  Nunavut and the Northwest Territories, Xinjiang (`Asia/Urumqi` against `Asia/Shanghai`).
  The comparison only measures: `tzf-rs`'s data comes from ODbL boundaries and never feeds
  the lite build ([source licences](source-licences.md)).
- `zone_name` needs the zone rules; a layer without them still names each timezone area by its
  tzdb name, which the harness reads instead.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| a. One workflow, rivals in Docker or as jars, one harness | free on a public repository; reproducible from pinned inputs; nothing on the shared machine | shared runners vary: medians, and the runner named | **Chosen** (D-331) |
| b. Each rival's numbers from its own publications | no setup | different machines, data and questions: not comparable | Rejected |
| c. Score rivals by ID agreement | exact | rivals have their own IDs; would need a crosswalk per rival | Rejected: distance to the expected point instead |
| d. Pelias and Overture through DuckDB too | the catalogue lists them | Pelias needs Elasticsearch and four importers; Overture has no place search | Later, as the report grows |

## 5. What we build ourselves, and why

`tools/competitive`: a small Rust harness, its own workspace so that its dependencies
(`tzf-rs`, an HTTP client) never reach the runtime graph. It reads the query files, asks each
rival, and writes the report. Every rival is behind one trait (`search(name) → points`,
`reverse(point)`, `tz(point)`), so adding Pelias later is one implementation.

## 6. Impact on the plan

- Phase 8.3's last item; `docs/06-engineering/ci.md` gains the workflow; the release attaches
  the report.
- The report is not a gate: a rival improving is not our failure. Our own regressions are
  caught by the Q, L and golden gates.

## 7. Risks and open items

- Photon's dump is replaced in place; a run months later reads a different dump. The report
  names the SHA-256 it used.
- A rival's maintainers may configure it better than we do. The report states the
  configuration and invites corrections (the benchmarks page's promise).

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Nominatim (Docker) | 5.3.3, image `mediagis/nominatim:5.3` (2026-10-26) | 2026-11-15 | Docker Hub |
| Photon | 1.3.0 (2026-08-07) | 2026-11-15 | GitHub releases |
| Photon Nepal dump | `photon-dump-nepal-1.0-latest.jsonl.zst`, 28.6 MB | 2026-11-15 | download1.graphhopper.com |
| `tzf-rs` | 2.3.0 (2026-11-02) | 2026-11-15 | crates.io |

## 9. Sources

- [mediagis/nominatim-docker](https://github.com/mediagis/nominatim-docker), README, accessed 2026-11-15.
- [Photon usage](https://github.com/komoot/photon/blob/master/docs/usage.md), accessed 2026-11-15.
- [GraphHopper Photon dumps](https://download1.graphhopper.com/public/), accessed 2026-11-15.
- [tzf-rs](https://crates.io/crates/tzf-rs), crates.io, accessed 2026-11-15.
- [Competitive analysis](competitive-analysis.md), the rivals and why each matters.
