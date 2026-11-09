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
4. `frames.plan(wanted)` turns wanted byte ranges of the pack into fetches: runs of adjacent
   frames, one range request each, sorted and never touching (the GitHub asset host refuses
   multi-range requests). `plan_sections(kinds)` does it for whole sections.
5. `Decoder::frame` (feature `zstd`, `ruzstd`) decodes one frame with its output capped at the
   decoded size and its window at 1 MiB. A frame that decodes to more or fewer bytes, or leaves
   bytes over, is refused.

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
