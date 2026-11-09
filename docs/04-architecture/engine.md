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
section table) and opens every view the queries use. It does not hash section bytes, except the
block-hash list: each section that BLOCK_HASHES covers has its list proved against the section's
hash once, at open, so a later block check needs only the list.

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
| `reverse(at, k)` | Every layer, skipped when a latitude bound proves it cannot beat the current k-th distance | Each layer gives its k nearest visible places (asking its index again for more while hidden places or ties could hide one); k-way merge by distance, then ID |
| `admin_at(at)` | The first layer in precedence order whose ADMIN_REGION holds the point (coverage beats existence) | none; a unit whose ID a higher layer claims is answered from that layer |
| `tz_at(at)` | The first layer whose REGION (timezones) holds the point | as `admin_at` |
| `search(q, k)` | Every layer with SEARCH | Each layer scores its rows and keeps its best k visible; k-way merge by score, then ID. Fuzzy keys share the first letter unless no layer has a visible strong match; a context after a comma matches ancestors in the row's own pack (D-119) |
| `within_bbox(rect, limit)` | Layers whose box meets the rect | Smallest IDs first, k-way merge by ID |

**Search scores** are the same in every layer, so the merge is exact. The query is folded with
`teigeodata_names::fold`. A row's score is its importance plus the best bonus it earns: exact
60; loose (spaces removed, with its exact twin) 55; one fuzzy edit 25 (queries of 4 to 63
characters); prefix 30 for the best 16 prefix matches by importance across all layers
(queries of 3 characters or more). There is no phonetic tier yet.

## Gates

- **Metamorphic** (`tests/metamorphic.rs`): a synthetic country (and, ignored, a real pack
  through `TGD_PACK`) is split by the format writer into 2, 3 or 5 packs, added in both orders
  and with an overlapping copy; every `get`, `reverse`, `admin_at`, `tz_at`, `search` and
  `within_bbox` answer matches the unsplit pack. Also: a newer release wins with its tombstones
  and redirects; priority and the baseline order the layers.
- **Differential:** a delta layer over a base matches the compacted pack built by the writer.
- **Verification** (`tests/verify.rs`): a corrupt small section fails only its queries; a
  corrupt block in a 2.5 MiB section is found by its block hash; a flipped block-hash list is
  refused at open; truncated, empty, missing and replaced files.
- **Allocations** (`tests/alloc.rs`): a counting allocator on the warm path.

Each gate was first shown to fail on a planted bug (D-114).

## Not yet

- `within_radius`, `nearby`, `children`, `ancestors` across packs.
- The snapshot swap (`arc-swap` against `RwLock`, L5) and the baseline (step 4.2).
- `teigeodata-api` still opens one pack; it moves onto the engine with the baseline.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-09 | First version: opening, verification, precedence, queries, gates | Step 4.1, D-113, D-114 |
