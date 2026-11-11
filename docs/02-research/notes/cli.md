---
title: "The CLI `tgd` and the catalog refresh"
status: research
updated: 2026-11-11
owner: cli, sync
revisit: 2027-05-01
---

# The CLI `tgd` and the catalog refresh

## 1. Question

Step 4.9 builds the CLI `tgd`, and M4 asks for it "complete" (Q-021). What must it do, and on
what? Three questions feed the build:

1. Which commands, over which library calls, and which stay in the forge.
2. Which argument parser, measured against its cost in binary size and build time.
3. What the sync commands stand on. `install`, `update` and `find` need a verified catalog
   fetched over the network and kept on disk between runs. The library has every check
   (`sync::trust`, D-159 to D-177) but no driver that fetches the metadata and keeps it: every
   app would have to write one. That driver is the first thing to build, in `sync`, so the CLI
   and every binding share it.

## 2. Summary and recommendation

- **A new crate `teigeodata-cli`, binary `tgd`, on `clap` 4.6 (derive).** clap costs
  369 KB stripped (162 KB gzip) and 14 s of clean build over `lexopt` for the same 16
  subcommands (measured below). The CLI has no size budget (the Z budgets are the libraries'),
  and clap gives what a production CLI is judged by: generated help, typo suggestions, uniform
  errors and shell completions (`clap_complete`). The engine with the embedded baseline is
  several megabytes anyway.
- **Commands (answers Q-021):**
  - packs: `inspect`, `verify`, `diff`, `extract` (by component), `export`;
  - queries: `get`, `reverse`, `search`, `near`, `admin`, `tz`;
  - sync: `refresh`, `catalog`, `find`, `locate`, `install`, `update`, `remove`, `pin`,
    `repair`, `status`;
  - `completions`.

  `merge` (compaction) waits on 4.7: it compacts delta packs, which are built only if they
  beat section re-download on two real releases. Building and spatial extracts stay in the
  forge (ADR-0001: its heavy dependencies never reach the runtime).
- **Outputs (ADR-0038):** text for people, `--json` for native JSON, `--geojson` for an RFC
  7946 FeatureCollection, `--nominatim` for Nominatim's `json` format on `search`, `reverse`
  and `get`. Every mode is mapped from the native result. `export` streams NDJSON or GeoJSON
  features and never holds a country in memory. The mapping lives in a library module, so the
  HTTP server (Phase 6) uses the same code.
- **The catalog refresh, `sync::refresh`:** the TUF client workflow (spec §5) over the store's
  `Fs` and a `Transport`, keeping the verified metadata under `meta/` in the store. Each
  refresh fetches one small file, `timestamp.json`, when nothing changed, and only the roles of
  the countries the app holds or asks for. `Transport` gains `fetch`, a whole-file GET with a
  byte cap, since metadata is not fetched by range.
- **Exit codes:** 0 ok; 1 a refusal (verification failed, not found, out of room); 2 a usage
  error (clap's); 3 the network. A script can tell "the data is bad" from "try again later".

## 3. Findings

### 3.1 The parser, measured (2026-11-11)

The same CLI (16 subcommands, a global flag, a path and two options each), release with thin
LTO, one codegen unit and `strip`, on this Mac, Rust 1.99:

| Parser | Stripped binary | gzip -9 | Clean release build |
|---|---:|---:|---:|
| `clap` 4.6.7, derive | 726,192 B | 323,166 B | 20.7 s |
| `lexopt` 0.3.2, by hand | 357,200 B | 160,881 B | 6.9 s |

`lexopt` leaves help text, suggestions, conflicts and completions to us: a few hundred lines
to write and keep in step with the commands. clap's build cost is paid once (incremental
builds do not rebuild it). `bpaf` (0.9.28) and `argh` (0.1.19) sit between the two. `argh`
follows Fuchsia's conventions and has no completions; `bpaf` is smaller than clap but far less
used (9.9 M downloads against clap's 1.32 G).

### 3.2 The client workflow as python-tuf's updater runs it

From python-tuf 7.0.1's `ngclient/updater.py` (read 2026-11-11):

- Each role is kept locally under an unversioned name (`timestamp.json`, `snapshot.json`,
  `<role>.json`, the role name URL-encoded), written atomically (a temporary file, then
  `os.replace`; no fsync). Roots are kept by version in `root_history/` with `root.json`
  linked to the current one.
- The root walk tries the local `N.root.json`, then the remote one, from the trusted version
  plus one, for up to `max_root_rotations` (256). A 403 or 404 ends it; any other error fails
  the refresh.
- The timestamp is loaded locally first, then always fetched. The same version again is
  normal and keeps the local one.
- The snapshot and each targets role are loaded locally first. Only when the local copy is
  missing or no longer the one the newer metadata names are they fetched, by version under
  consistent snapshots (`V.snapshot.json`, `V.<role>.json`), with the signed length as the
  cap.
- Delegated roles are loaded lazily, as a target lookup walks them.

The specification adds one rule python-tuf leaves to its trusted set: when a root rotation
changes the timestamp or snapshot keys, the stored timestamp and snapshot are deleted (§5.3.11,
recovery from a fast-forward attack). Our `Trusted::keys_rotated` reports it.

### 3.3 What a refresh costs

With the catalog delegated by country (D-176), a refresh that finds nothing new fetches
`timestamp.json` alone (about 1 KB). A new release of one country fetches the timestamp, the
snapshot (13 KB gzip for the world), the top-level targets (3.8 KB) and that country's role,
if the app holds it. Gate 12 bounds each file.

### 3.4 Nominatim's output

Nominatim's `json` output (docs `api/Output.md`, read 2026-11-11): `place_id`, `licence`,
`osm_type`, `osm_id`, `lat` and `lon` as strings, `display_name` (the comma-separated
address), `boundingbox` as four strings (south, north, west, east), `class`, `type`,
`place_rank`, `importance`, `addresstype`, `name`, and `address` with `addressdetails=1`. We
fill what the native result has: `place_id` (our stable ID, which is persistent where
Nominatim's is not), `lat`, `lon`, `name`, `display_name` (the place and its ancestors),
`boundingbox` (the point's box when we hold no extent), `licence` (the pack's attribution) and
`address` (the admin chain by level). The OSM fields are left out (ADR-0038: never faked).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `clap` derive | help, suggestions, completions, uniform errors; the Rust standard | +369 KB stripped, +14 s clean build | MIT OR Apache-2.0 | 4.6.7 (2026-09-14) | **Use** |
| `lexopt` | tiny, quick to build | help, completions and errors by hand | MIT | 0.3.2 (2026-02-28) | Reject for the CLI; fine for `xtask` |
| `bpaf` | combinators, smaller than clap | few users; a second idiom to learn | MIT OR Apache-2.0 | 0.9.28 (2026-09-17) | Reject |
| `argh` | small, derive | Fuchsia conventions, no completions | BSD-3-Clause | 0.1.19 (2026-03-16) | Reject |
| Refresh in each app | none for us | every binding writes the TUF workflow again | — | — | Reject |
| Refresh in `sync` (`sync::refresh`) | one tested path for the CLI and every binding | — | ours | — | **Build** |

## 5. What we build ourselves, and why

**`sync::refresh` (feature `store`).** `refresh(fs, transport, mirrors, shipped_root, now,
want)` returns the `Trusted` metadata with the roles of `want` and of every stored role. It
keeps everything under `meta/` in the store:

- `meta/root.json`: the newest trusted root, stored only after a rotation. The shipped root is
  the starting point the first time, or when it is newer than the stored one.
- `meta/V.snapshot.json`, `meta/V.targets.json` and `meta/roles/V.<name>.json`, named by
  version, so a new release never overwrites the files of the one in use.
- `meta/timestamp.json`: the commit point, written last.

Each file is written to a temporary name, synced, renamed and its directory synced (the
store's discipline, D-171). Ordering is what keeps a crash safe (as built, D-179):

1. Walk the roots (each `N+1.root.json` until a 404 or 403, at most 256). If the walk changed
   the timestamp or snapshot keys, delete the stored timestamp **before** storing the new
   root. Otherwise a crash between the two would leave a new root beside an old
   fast-forwarded timestamp, and the next run would not know to drop it. The delete matters
   even when the old key stays listed, so the old timestamp would still verify.
2. Load the committed timestamp, then fetch `timestamp.json` (capped by the root). If it is
   unchanged, nothing more is fetched or written.
3. Load the committed snapshot as the floor for the targets' versions, then take
   `V.snapshot.json` from the store if a cut-off refresh left it, else from a host (capped at
   its signed length).
4. The same for the top-level targets and each role asked for or held.
5. Write `timestamp.json`: the new catalog is committed. Then remove the files the new chain
   does not name, and the roles not held.

A refresh cut off anywhere before step 5 leaves the old chain whole: offline, `load` gives the
old catalog (with any role it names that was already stored), never a mix of releases.

Metadata comes from its own mirrors (GitHub Pages first). Assets have theirs (the release
host). A mirror that fails is demoted as for packs (D-170). The only failure that is not a
refusal is the network, and the stored metadata then stays as it was: the app works offline
on what it has.

**The pack commands, as built (D-180).** `extract --components` copies sections as stored;
`--bbox` re-encodes the kept rows with `format::extract`, keeping each place's ancestors and
timezone area, and leaves out what is keyed by renumbered rows. The deep check of `verify` is
`teigeodata_format::check::sections`, one function for every section. `export` maps a place
to one record for NDJSON and GeoJSON; the query outputs (4.9c) reuse it.

**`Transport::fetch(url, max, out)`.** A plain GET: status 200 (404 and 403 reported as
`Status`), at most `max + 1` bytes read, more than `max` refused. A platform transport
implements it beside `get`.

**The tests:**

- an in-memory host with a repository written by `sync::repo`: rotations, a fast-forward
  recovery (also with the old key still listed), an unchanged timestamp fetching nothing more
  and writing nothing, a new country release fetching only its role, a lying mirror demoted, a
  dead one passed over, and the last catalog kept offline after a failed refresh;
- a crash at every write, replayed in both crash modes (lost and kept unsynced writes);
- `ureq`'s `fetch` against a server on localhost: the cap after gzip decoding, 404 and 206
  refused;
- the CLI's commands run end to end against a repository written by `tuf release` and served
  over HTTP on localhost.

## 6. Impact on the plan

- **Phase 4, 4.9:**
  - (a) `Transport::fetch` and `sync::refresh`;
  - (b) the crate and the pack commands;
  - (c) queries and output modes;
  - (d) the sync commands;
  - (e) docs, completions and `hyperfine` timings.
- **Decision log:** D-178 (this design), and Q-021 closed.
- **Architecture overview:** the crate `teigeodata-cli` and its commands, replacing "build,
  tiles" (the forge's).

## 7. Risks and open items

- The repository's URLs are not public until the first ceremony (waiting on the maintainer).
  `tgd` takes `--repo` and `--assets` and ships no default until then.
- `merge` waits on 4.7.
- A shared output crate for the CLI and the server is left until the server exists: one module
  now, moved without change.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| clap | 4.6.7 | 2026-11-11 | crates.io API |
| clap_complete | 4.6.11 | 2026-11-11 | crates.io API |
| lexopt | 0.3.2 | 2026-11-11 | crates.io API |
| bpaf | 0.9.28 | 2026-11-11 | crates.io API |
| argh | 0.1.19 | 2026-11-11 | crates.io API |
| python-tuf | 7.0.1 | 2026-11-11 | `ngclient/updater.py` in the oracle's environment |

## 9. Sources

- [The Update Framework specification §5, client workflow](https://theupdateframework.github.io/specification/latest/#detailed-client-workflow), TUF project, accessed 2026-11-11.
- python-tuf 7.0.1, `tuf/ngclient/updater.py`, read locally 2026-11-11.
- [Nominatim, Place Output](https://github.com/osm-search/Nominatim/blob/master/docs/api/Output.md), osm-search, accessed 2026-11-11.
- [RFC 7946, The GeoJSON Format](https://www.rfc-editor.org/rfc/rfc7946), IETF, 2016.
- [clap](https://crates.io/crates/clap), [lexopt](https://crates.io/crates/lexopt), crates.io, accessed 2026-11-11.
