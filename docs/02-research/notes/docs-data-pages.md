---
title: "Per-country and measured pages of the docs site"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2027-05-15
---

# Per-country and measured pages of the docs site

## 1. Question

Phase 7.5: the site's data pages should say, per country, what the released pack holds (places
by kind and admin level, ISO codes, heights, timezones), how its names are written (languages,
scripts, the lexicon) and which sources build it under which licence; and the measured pages
should show sizes, speed and load with where each number came from. Hand-written figures drift
the day a release or a source changes (the data index already copies a summary's component
table by hand). Where does each figure come from, and what keeps a page equal to it?

## 2. Summary and recommendation

- **Country pages are generated from two inputs only:** the country's own data files
  (`data/countries/<cc>/profile.yaml`, `lexicon.yaml`, the sources in `data/sources.lock` that
  name it) and the **pinned release pack** (`site/data.json`, the pack every tour runs on), read
  through `tgd export` (every place, streamed, deterministic) and `tgd inspect --json`.
- One generator, `cargo xtask data-pages [--check]`, in Rust beside `check-data`, reusing its
  readers of the profile, lexicon and sources lock (no second parser). It writes
  `site/content/docs/data/coverage.mdx` (every country in the pack) and one page per country
  folder; `--check` fails on drift. CI runs it in the job that already fetches the pinned pack
  and builds `tgd` for the command-line tour.
- **Nothing names a country in code (P16):** a place's country is its country ancestor in the
  hierarchy (D-237), keyed by that country's ID code (`tgd:np/1wmc` is `np`), and the pages are
  the folders of `data/countries/`; a new country folder gets a page with no code change.
  Grouping by each place's own ID prefix was tried first and is wrong: an ID keeps the code of
  its first build, so the 20 places of Azad Kashmir and Gilgit-Baltistan (`tgd:pk/…`, under India
  in the Nepal view, D-150) counted as Pakistan's.
- **Measured pages** take sizes that are a function of the code and the pinned pack (the pack's
  components, the WebAssembly packages, library bytes per feature set) from the commands that
  gate them; timings (load, phone class) carry their date, machine and command, because they
  depend on the machine. Done as 7.5b (D-326): each gate writes a record with `--record`
  (`site/data/measured/<name>.json`: its own table plus the day, commit, machine and load
  average) and the same generator renders the records into `data/measured.mdx`.

## 3. Findings

- `tgd export` streams the pinned lite pack's 172,339 places as NDJSON in well under a second
  of the 15.6 MB pack; each line has `id`, `name`, `kind`, `admin_level`, `elevation`,
  `timezone` and `iso_code` (measured 2026-11-15). The ID's prefix is the country
  (`tgd:np/1xhb`, `tgd:in/207`, `tgd:jp/p7`); timezone areas are `tgd:world/…`.
- `tgd inspect --json` gives each component's bytes and the manifest (attribution, address).
- The release's summary (`summary-world-np-lite.md`) holds the gate figures (G-pip, G-tz,
  G-dem-coarse, heights QA) as prose; parsing prose is fragile, so the pages link to it instead
  of copying it until the forge writes the same figures as JSON (an open item, §7).
- The docs workflow has no Rust toolchain; fast-check's tour job has both the pinned pack
  (`docs-examples --fetch`) and a release `tgd`.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Generate from the pinned pack and `data/countries/`, drift-checked in CI | Every figure has one source; a release that moves a figure fails CI until the page is regenerated | Needs `tgd` and the pack in the check job | **Chosen** |
| A checked-in JSON record rendered by a Python script | The docs job needs no Rust | A second parser of the YAML (Python's stdlib has none); the record itself can drift from the pack | Rejected |
| Hand-written pages with the numbers copied | Nothing to build | Drift on every release; the reason this step exists | Rejected |
| Parse the release summary's prose | Has the gate figures | Fragile; the format is for people | Rejected; link to it |

## 5. What we build ourselves, and why

`xtask/src/data_pages.rs`: streams `tgd export` (bounded memory: per-country counters and one
`Vec<i32>` of heights per country for the median, 172 k values for the world pack), renders
MDX with the same escaping rules as the reference (D-323). The profile, lexicon and sources
readers become `pub(crate)` in their modules.

## 6. Impact on the plan

7.5 splits into 7.5a (coverage and country pages, this note) and 7.5b (measured pages).

## 7. Risks and open items

- **Measured records are taken by hand, on a clean tree.** A shared CI runner varies from run
  to run, so timings regenerated in CI would change the page on every run and hide real
  drift; sizes could be regenerated in CI, but one renderer for every record is simpler.
  First records (2026-11-15) were taken under a load average of 7 to 25 from another
  project's builds; the page shows each one's load average. Rerun them on a quiet machine
  before a release.

- The gate figures of a release (G-pip, G-tz, heights QA) should come as JSON from the forge
  beside each summary, so the country pages can show them; until then the pages link to the
  release's summary.
- A world pack with every country in full makes the coverage table long (about 250 rows); it
  stays one table, sortable by the reader's browser search.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Pinned pack | data-2026.11.14.3 `world-np-lite.tgd` | 2026-11-15 | `site/data.json` |

## 9. Sources

- [docs-site research](docs-site.md), §2 and §6 (generated pages, drift-checked).
- [Quality bar](../../06-engineering/quality-bar.md): generated pages are never hand-edited.
