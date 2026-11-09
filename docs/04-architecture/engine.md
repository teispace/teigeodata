---
title: "The engine: layered packs"
status: draft
updated: 2026-11-09
---

# The engine: layered packs

`crates/teigeodata-engine` opens any set of packs as **layers** and answers every query across
them as one dataset ([ADR-0011](../03-decisions/adr/ADR-0011-layered-engine.md)). The research
is the [engine and catalog note](../02-research/notes/engine-and-catalog.md). The rules on this
page are decisions D-113 (opening and verification) and D-114 (queries) in the
[decision log](../03-decisions/log.md). Measurements are in the
[Phase 4 benchmarks](../02-research/benches/phase-4.md).

## Opening a pack (D-113)

| Way | What it does | When |
|---|---|---|
| `Layer::open(path, opts)` with `Load::Map` (default) | Maps the file with `memmap2` | Files the library installed |
| `Layer::open` with `Load::Read` | Reads the file into memory | Files the engine does not own |
| `Layer::from_bytes` | Owns the bytes | Downloads, WASM, tests |
| `Layer::from_static` | Borrows `&'static [u8]` | The embedded baseline |

Opening checks the head exactly as `Pack::open` does (lengths, offsets, the head digest, the
section table) and opens every view the queries use. It hashes no section bytes and proves no
block-hash list: it only finds where each covered section's list is (D-142). The first read of
a section that BLOCK_HASHES covers proves its list against the section hash, once per layer, so
later block checks need only the list. A list that does not prove fails every read of that
section with `Integrity` and nothing else; `Verify::Eager` proves every list at open. Open is
O(head): 52 µs warm for a 1 GiB pack, where proving at open took 4.9 ms
([phase-4 benchmarks](../02-research/benches/phase-4.md#open-by-pack-size-d-142)).

**`unsafe`.** `teigeodata-format` and every other runtime crate keep `forbid(unsafe_code)`. The
engine is the one boundary crate: its lint table is the workspace table with `unsafe_code =
"deny"`, and only `src/map.rs` allows it, for two things: `Mmap::map`, and extending the borrow
of the backing bytes to `'static` inside `Owned`, which keeps the views next to the bytes they
borrow. The views are dropped before the backing, never escape with the `'static` lifetime
(`Owned::views` returns them for the borrow of `self`), and a compile-time check proves `Views`
is covariant, so shortening the lifetime is sound.

**SIGBUS.** A mapped file that shrinks under the mapping kills the process on the next read.
The engine does not catch the signal; it avoids it:
- it maps only files that follow the install rule of the
  [pack format](pack-format.md#physical-file-names-and-installs): written once, renamed into
  place, never written over or truncated (a rename leaves the old mapping on the old inode,
  which stays readable);
- a file it does not own is opened with `Load::Read`;
- an empty file is not mapped, and a file shorter than its head says is refused at open;
- `Layer::unchanged()` compares the file's length and modification time with those at open, so
  a storage manager can notice a replaced file and reopen it.

The `mmap` feature (default, not on `wasm32`) gates `memmap2`; without it `Load::Map` reads.

## Installed components (D-136)

A pack may be installed with only some of its components (D-133). `LayerOptions::installed`
names them (`None`: the whole pack). The layer opens no view on, and never hashes, a section of
a component outside the set, so those bytes may be holes in a sparse file; under
`Verify::Eager` only the installed sections are checked. The set must hold what its components
need (`Components::closed`) and `point` whenever the pack has it, or the layer is refused with
`Error::MissingComponent`. `Layer::components()` says what is installed and present. A layer
with `installed` set opens with `Pack::open_head`, which reads only the head, so the sections
left out (`BLOCK_HASHES` among them, when no installed section needs it) may be holes.

A query that needs a section the pack has but the layer was not installed with returns
`Error::MissingComponent { address, component }`, never a wrong or empty answer: every query
asks its layer for each section before reading it (`ensure`). Per query:

| Query | Needs |
|---|---|
| `get`, `reverse`, `within_radius`, `nearby`, `within_bbox` | `point` (always installed) |
| `elevation(place)` | `elevation` |
| `parent`, `ancestors`, `children` | `admin` |
| `name(place)` | `names` |
| `search` | `search` (with `names`); a context that matches rows also `admin` |
| `tz_of(place)` | `timezone`, else `timezone-index`; names `timezone` when neither is installed |
| `tz_at` | `timezone-index` |
| `admin_at` | `admin-index` (with `boundaries`) |

`Place` carries no elevation: `Engine::elevation(&place)` reads it, so a points-only `get`
never touches heights.

## Cargo features per component (D-137)

Each component the engine reads has a Cargo feature, all on by default: `elevation`, `admin`,
`names`, `search` (with `names`), `timezone`, `timezone-index`, `boundaries` and
`admin-index` (with `boundaries`). `point` is always built; `postal`, `dem` and `extra` are
read raw through `Layer::section_bytes`. A build without a feature has neither the query nor
the section reader, and treats the component as not installed: a pack's sections of it are
never read or hashed, `Layer::components()` leaves it out, and a query of a built component
that needs it (a search context needs `admin`) returns `Error::MissingComponent`.

| Query | Feature |
|---|---|
| `get`, `reverse`, `within_radius`, `nearby`, `within_bbox` | always |
| `elevation` | `elevation` |
| `parent`, `ancestors`, `children` | `admin` |
| `name` | `names` |
| `search` | `search` |
| `tz_of` | `timezone` or `timezone-index` |
| `tz_at` | `timezone-index` |
| `admin_at` | `admin-index` |

Measured by `cargo xtask sizes --native` (macOS arm64, release, LTO, 2026-11-09): the engine
with no component features (open, `get`, `reverse`) adds 97.3 KiB (60.9 KiB gzip) to a
library, against 387.8 KiB (212.0 KiB gzip) with every one. Before the features the
points-only build was 145.8 KiB (85.9 KiB gzip). Gate: `tests/built.rs` runs under any feature
set; CI runs it with none.

## Shared snapshots (D-141)

An `Engine` is a snapshot: cloning it shares its layers (`Arc<Layer>`). `Shared` (feature
`shared`, default) holds the current one for every thread:

- `load()` gives a query its snapshot, lock-free and allocation-free (`arc-swap`).
- `update(|engine| …)` builds the next engine from a clone of the current one and publishes
  it in one atomic store. `try_update` publishes only on `Ok`. Writers are serialised by a
  mutex, so no update is lost.
- A query never sees half an update and never waits for a writer.
- A removed layer stays mapped until the last snapshot holding it drops, so a pack file is
  never written over while mapped.

Gate: `tests/shared.rs`. Readers on 4 threads check every snapshot while 2,000 updates swap
one pack for another, and must see exactly one of them. A planted intermediate publish fails
it.

## Lazy verification (D-078)

`Verify::Lazy` (default) hashes a section the first time a query reads it, once per layer:
- a section under 1 MiB is hashed whole against its table hash;
- a larger section is checked block by block against its proved block-hash list (16 KiB
  blocks); a per-section bitmap remembers good blocks, so a block is hashed at most once.
  Queries verify every block of a section on first use; `Layer::section_bytes(kind, range)`
  verifies only the blocks a range touches.

A bad section fails every query that needs it with `Error::Integrity { address, kind }`, every
time; other sections still answer. `Verify::Eager` checks everything at open;
`Verify::Trusted` skips the hashes (for bytes a caller has just verified while downloading).

## Layers and precedence (D-114)

`Engine::add` keeps the layers sorted, highest precedence first, by `Layer::precedence`, which
depends only on the layers (never the order they were added):

1. the baseline (`LayerOptions::baseline`) is always last;
2. the higher developer priority (`LayerOptions::priority`);
3. the newer release (`release_cmp`: parts split at `.` and `-`, numbers by value, before text);
4. the more specific address (more `-` parts: `np-p3` before `np`);
5. the address text, then the component, then the head digest (a total order).

**Visibility.** A place in layer *i* is answered only if no layer above *i* knows its ID as a
current place, a redirect or a tombstone. The winning layer for an ID decides it: its row is
the answer, its redirect sends `get` to the survivor (at most 4 hops, across layers), its
tombstone hides the ID everywhere below. The check first tests each layer's ID range (IDMAP
and REDIRECTS bounds), so layers of other countries cost a comparison.

## Queries (D-114)

Every answer carries the `LayerId` it came from; `Engine::tier(id)` names it (`Baseline`, or the
pack's address, component and release). Hot paths write into caller-owned `Scratch` and output
`Vec`s: a warm `get`, `reverse`, `admin_at`, `tz_at` or `within_bbox` allocates nothing, and
`search` allocates only the folded query (`tests/alloc.rs`, M1).

| Query | Routing | Merge |
|---|---|---|
| `get(id)` | Layers whose ID range holds the ID, in precedence order; the first that knows it decides | none |
| `reverse(at, k)` | Every layer, skipped when a latitude bound proves it cannot beat the current k-th distance | Each layer gives its k nearest visible places (asking its index again for more while hidden places or ties could hide one); k-way merge by distance, then ID. Each layer's index search starts from the k-th distance of the layers before (D-140) |
| `within_radius(at, m, k)` | As `reverse` | `reverse` capped at the radius: no index box beyond it is opened |
| `nearby(place, k)` | As `reverse` | `reverse` from the place's point, without the place |
| `admin_at(at)` | The first layer in precedence order whose ADMIN_REGION holds the point (coverage beats existence) | none; a unit whose ID a higher layer claims is answered from that layer |
| `tz_at(at)` | The first layer whose REGION (timezones) holds the point | as `admin_at` |
| `search(q, k)` | Every layer with SEARCH | Each layer scores its rows and keeps its best k visible; k-way merge by score, then ID. Fuzzy keys share the first letter unless no layer has a visible strong match; a context after a comma matches ancestors in the row's own pack (D-119); the layer's query rules add matches (D-121) |
| `within_bbox(rect, limit)` | Layers whose box meets the rect | Smallest IDs first, k-way merge by ID |
| `parent(place)` | The place's own layer, else the first layer by precedence holding the same ID with a parent | none; the parent is answered by the layer that wins its ID (D-143) |
| `ancestors(place)` | As `parent`, step by step | parent first, at most `MAX_DEPTH`, never a place twice |
| `children(place)` | Every layer holding the place's ID | union by ID; a child is kept only while its winning copy still names this parent |

**Search scores** are the same in every layer, so the merge is exact. The query is folded with
`teigeodata_names::fold`. A row's score is its importance plus the best bonus it earns: exact
60; loose (spaces removed, with its exact twin) 55; one fuzzy edit 25 (queries of 4 to 63
characters); prefix 30 for the best 16 prefix matches by importance across all layers
(queries of 3 characters or more). A layer's query rules (SEARCH column RULES, D-120), read
once at open, add: a trailing unit-type abbreviation that some layer names is cut from the name,
and the rows of that type (column UNIT) gain 20; the query's phonetic key 5; a native generic
the query ends with, replaced by each of its Latin spellings and looked up loosely, 55; a
droppable suffix removed, 50 (D-121). The unit bonus does not count towards a strong match.

## Gates

- **Metamorphic** (`tests/metamorphic.rs`): a synthetic country (and, ignored, a real pack
  through `TGD_PACK`) is split by the format writer into 2, 3 or 5 packs, added in both orders
  and with an overlapping copy; every `get`, `reverse`, `admin_at`, `tz_at`, `search` and
  `within_bbox` answer (searches include "Name, Parent" contexts), and `parent`, `ancestors`
  and `children` of the probes and of every admin unit, matches the unsplit pack. Each part holds the ancestors of its rows (stubs,
  D-143). Also: a newer release wins with its tombstones and redirects; priority and the
  baseline order the layers.
- **Differential:** a delta layer over a base matches the compacted pack built by the writer.
- **Verification** (`tests/verify.rs`): a corrupt small section fails only its queries; a
  corrupt block in a 2.5 MiB section is found by its block hash; a flipped block-hash list
  fails every read of its section (and the open under `Verify::Eager`, D-142); truncated,
  empty, missing and replaced files.
- **Allocations** (`tests/alloc.rs`): a counting allocator on the warm path.
- **Components** (`tests/components.rs`, D-136): the pack opened under eight component sets,
  its uninstalled sections zeroed, eager and lazy; every query answers as the whole pack does
  or names the missing component; a set without what it needs is refused.

Each gate was first shown to fail on a planted bug (D-114).

## Not yet

- The baseline (step 4.2).
- `teigeodata-api` still opens one pack; it moves onto the engine with the baseline.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-09 | First version: opening, verification, precedence, queries, gates | Step 4.1, D-113, D-114 |
| 2026-11-09 | Search: query rules (unit, phonetic, native, drop) | D-120, D-121 |
| 2026-11-09 | Installed components, `MissingComponent`, `Engine::elevation`, `tz_of` | D-133 to D-136 |
| 2026-11-09 | A Cargo feature per component; `Engine::parent` | D-137 |
| 2026-11-09 | `within_radius`, `nearby`, `ancestors`, `children` | D-140 |
| 2026-11-09 | `Shared` snapshots on `arc-swap`; layers held in `Arc` | D-141 |
| 2026-11-09 | Block-hash lists proved on a section's first read, not at open | D-142 |
| 2026-11-09 | `parent`, `ancestors`, `children` across packs by ancestor stubs | D-143 |
