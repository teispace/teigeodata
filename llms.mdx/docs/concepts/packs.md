# Packs and components (/docs/concepts/packs)

A **pack** is one file (`.tgd`) of places and everything known about them: names, heights, the
hierarchy, boundaries, timezones, postal codes and the search index. Packs are built by the
forge from open sources, signed, and published in releases.

## Built to be mapped [#built-to-be-mapped]

A pack is laid out to be read where it lies: the engine memory-maps the file and reads the bytes
it needs, with no parsing step and no copy into the heap. Opening a 15 MB pack reads its head
(at most 16 KiB) and proves the block hashes of its large sections, and nothing else; a query touches only the pages it reads, which the operating
system shares between processes and drops under memory pressure.

- **Verified as it is read.** Every section carries a BLAKE3 hash in the head, and every 16 KiB
  block of a large section has its own. By default a section, or a block, is checked the first
  time a query reads it (`Verify::Lazy`); `Eager` checks everything when the pack opens.
- **Signed.** Released packs are signed with Ed25519 over the head, which holds every section's
  hash. `tgd verify --key` checks a signature; the store checks every pack it installs against
  the signed catalog. A pack downloaded by hand from a data release is checked by the SHA-256
  its release lists.
- **Forward compatible.** A reader skips sections it does not know, so older code keeps reading
  newer packs, and a pack states the oldest reader that can open it.

## Components [#components]

A pack is cut into **components**, each a set of sections. Install, download or link only the
ones your app uses; a query that needs a component you left out fails with `MissingComponent`
instead of answering wrongly.

| Component | What it holds | Needs |
|---|---|---|
| `point` | IDs, coordinates, kinds, the spatial index and retired IDs | |
| `names` | Every name of every place | `point` |
| `search` | The name search index | `names` |
| `substring` | Search from three or more characters inside a word | `search` |
| `admin` | Parents, children and ISO 3166 codes | `point` |
| `boundaries` | The outlines of admin units, as shared arcs | `point` |
| `admin-index` | The admin units at any point | `boundaries` |
| `elevation` | Each place's height | `point` |
| `dem`, `dem-coarse` | Elevation grids: a country's terrain, and a coarse grid of the world | |
| `timezone` | Each place's timezone | `point`, `tz-rules` |
| `timezone-index` | The timezone at any point | `point`, `tz-rules` |
| `tz-rules` | The tzdb rules: offsets at any instant, local times | |
| `postal` | Postal codes, current and former | `point` |
| `extra` | Optional fields and cold data | `point` |

The world lite pack, by component (disk and compressed wire bytes, `data-2026.11.14.3`):

| Component | Disk | Wire | Download with what it needs |
|---|--:|--:|--:|
| `point` | 1.60 MB | 1.06 MB | 1.06 MB |
| `elevation` | 0.32 MB | 0.23 MB | 1.29 MB |
| `admin` | 0.71 MB | 0.28 MB | 1.34 MB |
| `names` | 4.60 MB | 1.83 MB | 2.91 MB |
| `search` | 4.47 MB | 2.91 MB | 5.81 MB |
| `timezone` | 0.06 MB | 0.01 MB | 1.14 MB |
| `boundaries` | 2.19 MB | 2.03 MB | 3.11 MB |
| `admin-index` | 1.07 MB | 0.80 MB | 3.91 MB |
| `tz-rules` | 0.08 MB | 0.06 MB | 0.07 MB |
| `dem-coarse` | 0.53 MB | 0.52 MB | 0.52 MB |

So an app that only finds the nearest named place to a point needs `point` and `names`: 2.9 MB
on the wire instead of 9.8 MB. The libraries are cut the same way: each language's build can leave out the code of
components it does not use (the WebAssembly package has a smaller `-lookup` build without name
search).

A pack also splits **by language** (install the names of up to eight languages besides the base
names) and **by outline detail** (install a country's boundaries down to a level, simplified to
a tolerance in metres). `OpenOptions` and the store's install options take the same choices.

## Layers [#layers]

An engine answers from several packs at once; each is a **layer**. Where layers hold the same
place, one rule decides which answers: the higher `priority` you give, then the newer release,
then the more specific pack (a country over the world), then the pack's address as a last
tie-break, and the baseline always last. So a
detailed country pack overrides the world pack inside that country, while the world pack still
answers everywhere else. The answers do not depend on the order the packs were added (a CI gate
splits packs and compares). Typical stacks:

- **One world pack** bundled with the app: every country, its first-level units and its cities,
  and the full detail of the countries you serve.
- **A world baseline plus country packs** downloaded on demand: the user opens "Dubai", the
  app installs the UAE pack, and the engine answers from both.
- **Terrain packs** (`dem`) added over the coarse world grid where heights must be precise.

Layers can be added and removed while other threads query: each change publishes a new snapshot,
and queries already running finish on the one they started with.

## Where packs come from [#where-packs-come-from]

The [store](/docs/concepts/store) finds, downloads, verifies and updates packs for you from the
live signed catalog, which lists each pack's releases and the deltas between them. Every pack is
also in a data release on GitHub, with checksums, to download and open by hand.