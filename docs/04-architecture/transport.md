---
title: "The transport asset"
status: draft
updated: 2026-11-09
---

# The transport asset

What a client downloads for a pack: the `.tgd` in the zstd seekable format, framed on the pack's
own structure (D-112). The research and the alternatives are in
[download and delta](../02-research/notes/download-and-delta.md); the pack itself is the
[pack format](pack-format.md). Built in `crates/teigeodata-sync`, module `transport`
(Phase 4 step 4.4).

## The file

```
zstd frame 0        the head: header, section table, manifest, signature, zero padding
zstd frame 1 …      each section with the padding after it; the last one with the footer
                    (a section of 1 MiB or more: one frame per 1 MiB from its start)
skippable frame     the seek table (magic 0x184D2A5E, Frame_Size)
                      per frame: Compressed_Size u32, Decompressed_Size u32 (no checksums)
                      footer: Number_Of_Frames u32, Seek_Table_Descriptor u8 = 0,
                              Seekable_Magic_Number u32 = 0x8F92EAB1 (the file's last bytes)
```

- **The format is the standard one**, v0.1.0 of `contrib/seekable_format` in facebook/zstd
  (checked against the spec on 2026-11-09). Any zstd decoder reads the asset whole and gets the
  `.tgd` back byte for byte; the reference `zstd -d` and the `zeekstd` crate are the test
  oracles.
- **The cuts are canonical.** They follow from the section table alone:
  - a cut at 0 and at each section's start (an empty section shares the next one's start);
  - then every 1 MiB after a section's start, while inside its span up to the next section or
    the end of the file.

  1 MiB is 64 blocks of BLOCK_HASHES (D-078), so an interior cut falls on the block grid and a
  decoded frame of a large section is whole blocks. No frame crosses a section start, and no
  frame decodes to more than 1 MiB. The head frame is at most 16 KiB (plus the footer of a pack
  without sections).
- **No checksums in the table.** Bit 7 of the descriptor stays 0: BLAKE3 covers every block and
  every small section, so the XXH64 checksum would be a second, weaker check. The reader still
  parses tables with checksums (`zeekstd` and `zstd` write them), and refuses reserved bits.
- **Frames are plain zstd frames**: the content size in the header, no frame checksum, a window
  of at most 1 MiB (the frame itself).

## Reading it (sans-I/O, `no_std` with `alloc`)

1. `SeekTable::tail_len(footer)` gives the seek table's length from the last 9 bytes, if the
   catalog does not carry it.
2. `SeekTable::parse(table)` checks the magic numbers, the skippable frame size, the descriptor
   and the frame count (at most 2²² frames).
3. `table.check(asset_len, layout)` checks it against the asset's length and the `Layout`
   derived from the signed head. It accepts the table only if:
   - it lists exactly the layout's frames, each with its cut's decoded size;
   - every compressed size lies between 9 B (the smallest zstd frame) and zstd's
     `ZSTD_COMPRESSBOUND` of its decoded size;
   - the frames and the table fill the asset exactly.

   It returns `Frames`, every frame's byte range in the asset and in the pack.

   A ranged client has no pack yet (D-166): `table.head_frame()` gives frame 0's byte range
   and decoded size, capped at `MAX_HEAD_FRAME` before any request; it decodes that frame, and
   `table.frames_from_head(asset_len, head)` checks the head with `PackHead::parse` against
   the table's decoded length, derives the layout (`Layout::of_head`) and runs `check`. Two
   requests (table, head frame) give the whole plan.
4. `frames.plan(wanted)` turns wanted byte ranges of the pack into fetches: runs of adjacent
   frames, one range request each, sorted and never touching (the GitHub asset host refuses
   multi-range requests). `plan_sections(kinds)` does it for whole sections, and `plan_components(set)` for
   an install by component (D-136): the head frame, the sections of the set and what it needs,
   and `BLOCK_HASHES` when one of them is 1 MiB or more.
5. `Decoder::frame` (feature `zstd`, `ruzstd`) decodes one frame with its output capped at the
   decoded size and its window at 1 MiB. A frame that decodes to more or fewer bytes, or leaves
   bytes over, is refused.

## Installing it (sans-I/O, feature `install`, D-167)

`sync::install::Install` installs a pack, or some of its components, from the asset by range
requests. The caller does all I/O: it fetches the ranges asked for and writes the bytes handed
back into a staging file of the pack's length.

1. `Install::new(entry, asset_len, set)` from the verified catalog entry and the TUF target's
   length; `first()` gives two requests that may run at once: the seek table (its length is in
   the entry) and the head frame (`transport.head`).
2. `start(table, head_frame, write)` checks the head against the signed entry (head hash,
   length, address, component, release) and itself, the table against the head, writes the
   head frame (and the footer when the last section is not installed), and returns the fetches:
   `BLOCK_HASHES` alone first when a section of 1 MiB or more is wanted, then the sections in
   file order (`Frames::plan_install`).
3. `feed(fetch, bytes, write)` takes any run of wanted frames still to do (a fetch of the plan,
   or of `remaining()`), decodes each frame with its output capped, then checks
   it before writing: a section under 1 MiB whole against its BLAKE3, a larger one block by
   block against its block hashes (`blocks::proven_cvs`, proven against the section hash when
   `BLOCK_HASHES` arrives), padding as zeros, the footer as the head's. A covered section fed
   before its block hashes is refused.
4. `finish()` once every wanted frame is done: bytes fetched (equal to the catalog's
   `size_of(set).wire` on a fresh install), written, and kept from staging, and the pack length.

**Resume (D-169).** Progress is tracked per frame, and the staging file is its own record. On a
new attempt `fetch::install` fetches the seek table and head again (one `ETag` per attempt; the
asset host ignores `If-Range`, so a changed asset is refused, never mixed), feeds `BLOCK_HASHES`
first, then reads back every wanted frame the old file could hold and offers it to
`Install::staged`, which checks it exactly as a fetched frame and keeps it if it passes. Only
`remaining()` is fetched. A torn write, a stale file from another release or a foreign file
costs just its bad frames; no journal or resume record can disagree with the bytes, because
there is none. `BLOCK_HASHES` is never kept (it is 0.2% of what it covers), so a stale list
cannot vouch for stale frames. The caller cuts the file to `Installed::pack_len` and syncs it
before the rename. Tests cut an install at every request and resume it (the pack, byte for byte,
never more fetched than fresh), flip a byte inside NAMES (only its frame is fetched again), stale
the list, and resume over a foreign, longer file.

Nothing is written before it is bound to the signed chain (catalog entry → head hash → section
hashes → block hashes). Tests in `xtask` (`tuf::install_tests`) install every component set
from the entry and asset `entry_of` publishes and compare with the pack, and forge each frame,
each padding byte and the footer (validly recompressed, so only the hashes can tell): every
forgery fails and no forged byte reaches the staging file.

**Moving the bytes (feature `fetch`, `http`; D-168).** `sync::fetch::Transport` is one ranged
GET; `fetch::install` drives `Install` over one into a `Write + Seek` staging file. Every
response is held to its request: status 206, a `Content-Range` naming exactly the requested
bytes and the catalog's asset length, at most those bytes read, and one `ETag` for the whole
install. `Ureq` (feature `http`) is the default transport: `ureq` 3.5.1 with rustls and the
platform's trust store, no gzip, redirects followed with `Range` kept. Platform apps bring their
own transport behind the trait, or drive `Install` directly. Tests run the driver against a
local host that answers honestly, ignores `Range`, shifts `Content-Range`, swaps the asset
midway, or sends a byte too many: the honest host installs the pack byte for byte, each other
one is refused for its own reason.

**Trust.** The seek table is not signed and does not need to be. A lying table either fails
`check`, or (sizes shifted between frames, total kept) makes a frame fail to decode. In every
case a frame's output is bounded by what the signed head implies, and the bytes are then checked
against BLAKE3. A lying table can waste a request; it cannot install wrong bytes. The fuzz
harness `transport_asset` checks exactly that property on writer-made packs.

## Writing it (feature `write`)

`transport::write(pack, compress, sink)` streams the asset frame by frame with bounded memory.
The compressor is the caller's, so C zstd stays out of the runtime crate (the release tooling
uses level 19). Each frame must be one zstd frame within the bound; it is decoded again with the
client's decoder and compared with the pack before it reaches `sink`. Then the seek table is
written and passed through the reader's `check`. `transport::encode` does the same in memory and
then reads the whole asset back as a client does (`transport::verify`). A planted bug in either
path fails the tests (D-112).

`transport::store` wraps bytes in a zstd frame of raw blocks, for data that does not compress,
tests and fuzzing.

## Measured (Nepal main, 2026-11-09)

On the 11,748,636 B pack (14 sections), zstd 1.5.7 through the `zstd` crate:

| Level | Whole-file zstd | Asset | Over whole-file | Frames | Seek table |
|---:|---:|---:|---:|---:|---:|
| 3 | 8,620,266 | 8,653,556 | +0.39 % | 23 | 201 B |
| 9 | 8,446,881 | 8,518,810 | +0.85 % | 23 | 201 B |
| 19 | 8,146,743 | 8,227,234 | +0.99 % | 23 | 201 B |

Decoding the whole level-19 asset frame by frame with `ruzstd`: 258–320 MB/s (C zstd 1.0–1.3
GB/s) on a loaded Mac, about 40 ms for the pack. Every section is one request; the bytes per
section and the method are in [download and delta](../02-research/notes/download-and-delta.md#35-the-transport-as-built-measured-2026-11-09).

## Open

- The ranged install flow, resume and verify-then-write (step 4.5) build on `Frames` and
  `Decoder`.
- Head-only parsing: a client has the head frame, not the whole pack, so `Layout::new` takes the
  file length and section spans directly; a head parser in `teigeodata-format` comes with 4.5.
