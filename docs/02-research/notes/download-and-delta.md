---
title: "Download and delta: transport compression, ranged downloads, updates"
status: research
updated: 2026-11-09
owner: research-agent
revisit: 2027-02-09
---

# Download and delta: transport compression, ranged downloads, updates

## 1. Question

[Offline loading and trust](offline-loading-and-trust.md) chose the architecture:
- a sans-I/O planner and installer in Rust, with platform transports;
- staging, fsync and rename, with a journal as the commit point;
- LRU eviction with pinning;
- section re-download as the primary delta, with zstd patches and delta packs as A/B
  candidates.

[Block hashes](block-hashes.md) gave per-16 KiB verification (D-078). What Phase 4 still needs
before it builds the download manager and delta updates:

1. **Transport compression.** The 11.2 MB Nepal main pack is 7.84 MB under `zstd -19` and
   7.64 MB under `xz -9`, but a compressed file breaks the range requests that section updates
   and resume rely on. What should the published asset be?
2. **Delta sizes, measured on our packs.** What does each delta method actually cost on an
   11 MB pack whose sections change partly? Compare section re-download, zstd `--patch-from`,
   bsdiff, Courgette and Zucchini, RFC 3229 and RFC 9842, chunking (zsync, zchunk,
   casync/desync, OSTree), and delta packs.
3. **Download manager details** that the earlier note left open:
   - resume across frames;
   - per-block integrity while decompressing;
   - the disk peak of an install and a storage-budget formula;
   - mobile background limits that changed since 2026-11-06.

## 2. Summary and recommendation

**Publish one transport asset per pack: the pack in the zstd seekable format.**
- Frames follow the pack's own structure: one frame for the head, then one per section, with
  sections of 1 MiB or more split into frames of at most 1 MiB on 16 KiB boundaries (the
  BLOCK_HASHES grid).
- The decompressed stream is the `.tgd` byte for byte, so the installed file and its BLAKE3
  are the same whether it came compressed or not.
- **Measured on 2026-11-09** on the Nepal main pack (11,213,084 B): whole-file `zstd -19`
  7,840,103 B; one frame per section 7,855,298 B (+0.2 %); fixed 1 MiB frames 7,900,380 B
  (+0.8 %); `xz -9e` 7,641,920 B (−2.5 %).
- The seekable layout costs at most 1 % against whole-file zstd and keeps everything range
  requests give us: section updates, resume, repair, and fetching only some sections.
- The decoder is `ruzstd`, already in the reader behind the `zstd` feature (D-079: 105 KiB raw
  wasm, 26 KiB gzipped), so `sync` adds no compression dependency.
- **Built and measured on 2026-11-09** (D-112, §3.5) on the newer pack of 11,748,636 B:
  +0.99 % over whole-file zstd at level 19 (8,227,234 B in 23 frames), +0.39 % at level 3;
  `zstd -d` and `zeekstd` read it back byte for byte.
- xz is 2.5 % smaller, but it would add a decoder, decodes several times slower, and is no
  better at seeking. Rejected.

**Delta: section re-download stays primary, and binary patches are dropped.**
- **Measured, lite → main as a stand-in pair:** 4 of 14 sections were byte-identical (ARCS,
  POLY_INDEX, ADMIN_REGION, ARCS_OVERVIEW: 3.83 MB of 11.2 MB). Section re-download fetched
  4.59 MB compressed. zstd `--patch-from` per changed section (the better of patch and plain
  zstd) gave 4.43 MB, 3.4 % less; a whole-file patch gave 4.47 MB.
- Why patches barely help: our sections are already column-coded (bit-packed integers, FSST
  text, our own FST, D-073 to D-085). A changed row shifts bit-packed blocks and FST structure,
  so little survives as byte runs. OSTree's docs say the same of already-compressed files, which
  it ships whole instead of as bsdiff.
- **Modelled, not measured:** a *delta pack* of changed records plus tombstones for a release
  that touches 1 % of places is about 30–55 KB. Section re-download of the same change is about
  3 MB, because inserting or moving one place reorders every place-keyed section.
- Delta packs therefore keep their A/B gate. They are the only candidate with a large expected
  gain, and their cost is query-time merging ([engine and catalog](engine-and-catalog.md)).
  Decide on two real consecutive releases.

**Download manager additions.**
- **Ranged flow:**
  1. Fetch the seek table from the tail of the asset (its length is in the catalog).
  2. Fetch the head frame and verify the head against the signature and the catalog.
  3. Plan the frames to fetch.
  4. Fetch each frame by a single range request (GitHub refuses multi-range, measured 501).
  5. Decompress with a bound equal to the frame's decoded size.
  6. Check every 16 KiB block against BLOCK_HASHES, or small sections whole, **before writing**.
- **Resume** restarts at the first unfinished frame: at most 1 MiB is lost per interruption.
- **Background (iOS, Android whole-file tasks):** download the whole `.tgd.zst` into staging,
  then decompress and verify in the foreground or on the next launch.
- **Storage:** an install needs `new_pack + staged_transport` free bytes (the old pack stays
  until no snapshot maps it). The planner reports this number before starting, and eviction
  runs first.

## 3. Findings

### 3.1 Where the bytes are (Nepal main, built 2026-11-09)

Section sizes from the pack's section table, compressed one section at a time on this Mac
(zstd 1.5.7, xz 5.8.4; a throwaway script, not kept):

| Section | Raw B | `zstd -19` B | Ratio | `xz -9e` B |
|---|---:|---:|---:|---:|
| head (header, table, manifest, signature) | 1,344 | 788 | 0.586 | — |
| IDMAP | 34,164 | 2,526 | 0.074 | 1,208 |
| PLACES | 480,420 | 438,576 | 0.913 | 437,296 |
| HIERARCHY | 372,152 | 115,669 | 0.311 | 101,064 |
| NAMES | 2,097,996 | 856,189 | 0.408 | 815,140 |
| SEARCH | 2,284,420 | 1,467,094 | 0.642 | 1,421,928 |
| PLACE_INDEX | 54,040 | 51,644 | 0.956 | 51,392 |
| ARCS | 2,659,828 | 2,459,088 | 0.925 | 2,440,512 |
| POLYGONS | 21,384 | 20,319 | 0.950 | 20,272 |
| POLY_INDEX | 8,180 | 7,514 | 0.919 | 7,504 |
| REGION (timezones) | 2,007,656 | 1,606,234 | 0.800 | 1,581,388 |
| POSTAL | 10,144 | 7,217 | 0.711 | 7,008 |
| BLOCK_HASHES | 19,968 | 19,981 | 1.001 | 20,028 |
| ADMIN_REGION | 1,066,180 | 719,081 | 0.674 | 669,020 |
| ARCS_OVERVIEW | 94,780 | 83,378 | 0.880 | 81,312 |
| **sum per section** | **11,213,084** | **7,855,298** | 0.701 | 7,655,924 |
| whole file | 11,213,084 | 7,840,103 | 0.699 | 7,641,920 |

Whole file with `--ultra -22 --long=27`: 7,838,116 B, no better than level 19.

What it shows:

- **Per-section compression costs 15 KB (0.2 %).** Sections share almost no long-range
  redundancy, so independent frames lose almost nothing.
- **Fixed frames:** 256 KiB 7,940,953 B (+1.3 %), 1 MiB 7,900,380 B (+0.8 %), 4 MiB
  7,893,081 B (+0.7 %). The seek table costs 8 B per frame plus 9 B.
- **Already dense:** ARCS, PLACES, PLACE_INDEX, POLYGONS and BLOCK_HASHES (0.91–1.0). zstd
  stores incompressible blocks raw, so the overhead there is a few bytes.
- **Still compressible**, which is a hint for the format rather than for transport:
  - IDMAP (0.074): a first build's serials are almost a ramp; the ramp coding (D-075) should
    take more of it.
  - HIERARCHY (0.311): parent rows are local in Hilbert order, so a delta coding may pay.
  - NAMES (0.408): FSST leaves cross-record repetition.
  - SEARCH (0.642).
  - REGION (0.800).
  These feed a later format note; they do not change the transport choice.

### 3.2 Transport options

| Form | Download, Nepal main | Range requests | Decoder | Notes |
|---|---:|---|---|---|
| raw `.tgd` | 11.21 MB | yes, on sections | none | simplest; 43 % more bytes |
| whole-file zstd | 7.84 MB | no | `ruzstd` | updates and resume need a second, raw asset |
| whole-file xz | 7.64 MB | xz has a block index, but our tools do not use it | a new crate | slow decode |
| **zstd seekable, frames = head + sections, ≤ 1 MiB** | 7.86–7.90 MB | yes, on frames | `ruzstd` | **chosen** |
| raw `.tgd` + whole zstd, two assets | 7.84 first, raw for updates | yes | `ruzstd` | twice the assets (the 1,000-per-release limit) and two hashes per pack |
| HTTP `Content-Encoding` from the host | n/a | n/a | n/a | the GitHub asset host sends no `content-encoding` (measured: `application/octet-stream`, 206 on a range) |

**The zstd seekable format** (v0.1.0, in `contrib/seekable_format` of facebook/zstd):
- the data is ordinary independent zstd frames;
- a final skippable frame (magic `0x184D2A5E`) holds entries of compressed size and decompressed
  size (4 + 4 bytes each, plus an optional XXH64-low-32 checksum);
- a 9-byte footer: frame count, a descriptor byte and the magic `0x8F92EAB1`, as the file's last
  bytes.

Any zstd decoder reads such a file whole, because skippable frames are skipped. We need no
seekable library:
- the seek table is about 40 lines to parse;
- each frame is a standard frame for `ruzstd`;
- the optional checksum is redundant, because BLAKE3 covers every block.

`zeekstd` 0.6.3 (BSD-2-Clause, 2026-10-19) implements the format in Rust, but over C zstd
(`zstd-safe`). At most it is a test oracle for the writer; `zstd-seekable` 0.1.23 is from 2023.

**Trust of the seek table.** The seek table is not signed, and it does not need to be:
- it only says where to read;
- each frame's output is bounded by the decoded length it claims, which must match the section
  table, and the section table is signed through the head digest;
- the output is checked block by block.

A lying seek table can waste a request. It cannot install wrong bytes.

**TUF target.** The asset a client downloads is the target, so `length` and `hashes` are the
transport file's, and that keeps TUF's endless-data and wrong-file checks. The raw pack's length
and BLAKE3 and the section-table hash travel in `custom`
([engine and catalog](engine-and-catalog.md)). A ranged client never downloads the whole target,
so its trust comes from the signed head (pack signature plus `custom.sections`); this is the
same extension the earlier note made for section updates.

### 3.3 Delta methods, measured and published

**Our measurement (2026-11-09).** There are no two releases of the same pack yet, so main and
lite of the same day stand in. They differ in sources and in about 6 % of places, which makes
this a large change, and Phase 4 must repeat it on two real releases:

| Method | Bytes to fetch | Against full (7.84 MB) |
|---|---:|---:|
| Full download, seekable | 7,855,298 | 100 % |
| Section re-download (4 sections reused, 3.83 MB raw) | 4,585,449 | 58 % |
| Per-section `zstd -19 --long=27 --patch-from` old → new, smaller of patch or plain | 4,427,914 | 56 % |
| Whole-file `--patch-from` lite → main | 4,470,692 | 57 % |

Per section, the patch saved:
- 92 % on POLYGONS (21 KB → 1.6 KB) and 37 % on BLOCK_HASHES;
- 7 % on SEARCH and 2 % on NAMES;
- under 1 % on PLACES and PLACE_INDEX, and nothing on REGION.

**Expected sizes for typical releases of an 11 MB pack** (derived from the per-section sizes
above; which sections change follows from the format's dependencies):

| Release changes | Sections that change | Section re-download |
|---|---|---:|
| Names only (spelling fixes, new aliases) | NAMES, SEARCH, BLOCK_HASHES, head | 2.34 MB (30 %) |
| Places added, removed or moved (rows reorder) | IDMAP, PLACES, HIERARCHY, NAMES, SEARCH, PLACE_INDEX, POSTAL, POLYGONS (holds place rows), BLOCK_HASHES | 2.98 MB (38 %) |
| Boundaries only | ARCS, POLYGONS, POLY_INDEX, ADMIN_REGION, ARCS_OVERVIEW, BLOCK_HASHES | 3.31 MB (42 %) |
| Timezone data only | REGION, BLOCK_HASHES | 1.63 MB (21 %) |
| Everything | all | 7.86 MB |

**Delta pack (modelled).** The place-keyed sections hold 5.32 MB raw (2.93 MB compressed) for
96,847 places: 55 B raw, 30 B compressed per place. A release touching 1 % of places (about
970) gives a delta pack of roughly 30–55 KB plus a head and small fixed costs, against
2.98 MB by section re-download. That is about 50 times smaller, before counting the query cost
of one more layer.

**Published results for the other methods.**

| Method | What it is | Published result | Fit for us |
|---|---|---|---|
| bsdiff | suffix-sort diff plus bzip2 | Chrome update of about 10.4 MB: 704,512 B patch (Chromium blog, 2009) | slow and memory hungry; same byte-shift problem as zstd patches |
| Courgette | disassembles x86, diffs symbolic labels | same update: 78,848 B | executables only |
| Zucchini | Chromium's successor patcher; understands PE, ELF and DEX | patches made once on the server (about 15–20 min and 1 GB RAM for Chrome); client falls back to the full file if a patch fails | executables only; its fallback rule is worth copying |
| zstd `--patch-from` | old file as a dictionary | zstd wiki (2020): patch size comparable with bsdiff at level 19, much faster | measured above: 3.4 % on our sections |
| RFC 3229 (2002) | delta encoding in HTTP: `A-IM`, `IM`, status 226, instance manipulations such as `vcdiff` | needs the server to make or pick a diff per request | GitHub serves static files; rejected |
| RFC 9842 (2025-09) | compression dictionary transport: `Use-As-Dictionary`, `Available-Dictionary` (SHA-256), `dcb` and `dcz` encodings | `dcz` windows up to 128 MB; same-origin or CORS-readable only; HTTPS only | a future own mirror or hosted tier could serve `dcz` with the old pack as the dictionary; nothing in v1 needs it |
| zsync | rolling checksums against the old file, fetch missing blocks by range | needs many ranges; multi-range gets 501 on GitHub (measured 2026-11-06) | rejected in the earlier note |
| zchunk | a file of independently compressed chunks, with a header listing chunk checksums; any range-capable HTTP server | BSD-2-Clause; tag 1.5.5 | the closest relative of our design; our sections and block hashes already give chunk boundaries and checksums |
| casync / desync | content-defined chunk stores and an index | casync's last tag `v2` (2017-07-26); desync v1.1.5 (2026-10-17) | chunk stores break the 1,000-assets-per-release limit; rejected for v1 |
| OSTree static deltas | precomputed `from → to` parts with bsdiff for similar files and *fallback objects* fetched whole for files bsdiff cannot help (its example: an already-compressed initramfs) | v2026.6 (2026-10-30) | confirms: diff tools do not help on already-encoded data |

### 3.4 Download manager details

**Ranged install flow** (foreground and desktop):

1. Catalog says: target `ae-du.places.2026.12.0.<hash8>.tgd.zst`, its `length`, and
   `custom.seek_len`.
2. Range `length − seek_len … length − 1` → the seek table. Validate it:
   - the frame count is within a cap;
   - the compressed sizes sum to `length − seek_len`;
   - every decompressed size is at most 1 MiB, and the head frame's at most 16 KiB.
3. Range of frame 0 → decompress → the head. Run `Pack::open`'s head checks, the Ed25519
   signature, and the catalog's `pack_blake3` prefix and `sections` hash.
4. **Plan:** for an update, compare the section hashes with the installed pack's head. A section
   with an equal hash is copied from the old file. Otherwise its frames are fetched. Map frames
   to sections by cumulative decompressed offsets, which must match the section table's offsets
   and lengths. A mismatch rejects the asset.
5. Fetch each frame by a single range request over one HTTP/2 connection. Decompress with the
   output capped at its stated size, then verify:
   - small sections whole, once all their frames are in;
   - large sections per 16 KiB block against BLOCK_HASHES. BLOCK_HASHES is fetched early,
     right after the head, because it sits near the end of the file.
6. Write verified bytes at their final offsets in the staging file.
7. Once every section is present, check the whole-file BLAKE3 against `pack_blake3`. Then
   fsync, rename, update the journal and swap the snapshot ([offline loading and
   trust](offline-loading-and-trust.md)).

**Resume.** *As built (D-169), there is no resume record: the staging file is checked frame by
frame against the signed hashes on the next attempt, so the paragraph below is the original
plan.* The resume record holds the asset name, ETag, total length and the set of finished
frames. The GitHub asset host ignores `If-Range` (measured 2026-11-06), so every resumed request
re-checks the ETag and the `Content-Range` total. A frame is either finished and verified, or
fetched again, so at most 1 MiB is lost. A whole-file download that gets interrupted resumes
from its last finished frame boundary in the same way.

**Background transfers.**
- **iOS:** a background `URLSession` hands back whole files. The planner gives it the whole
  `.tgd.zst` for a first install, or one ranged task per changed frame run for an update (a
  `URLRequest` with a `Range` header; **unverified** that background sessions honour it with
  GitHub's redirect). The finished file is moved into staging at once, then decompressed and
  verified when the app next runs code.
  - iOS 26 adds `BGContinuedProcessingTask` for user-started work that continues in the
    background with system progress UI. It might cover the decompress-and-verify step
    (**unverified**: the Apple page did not render for us).
- **Android 14+:** user-initiated data transfer jobs remain the right home. Android 16 puts
  regular and expedited jobs under runtime quota when they start in the top state or run beside
  a foreground service, and its notes point transfers to UIDT (fetched 2026-11-09).
- **Web:** `fetch` with `Range` into OPFS through a `FileSystemSyncAccessHandle` in a worker,
  with `ruzstd` in WASM. A browser app needs a CORS mirror (Q-014).

**Storage budget and the disk peak.**

| Phase of an install | Extra disk |
|---|---|
| Ranged download, foreground | the staging `.tgd` (raw size of the new pack) |
| Whole-file background download | the `.tgd.zst` plus the staging `.tgd` while decompressing |
| After commit, before the old snapshot drops | old and new pack together |

So the planner asks for `new_raw + (whole-file transfer ? transport_len : 0)` free bytes. The
old file is deleted when no snapshot maps it. For Nepal main that is 11.2 MB, or 19.1 MB for
a background first install. Eviction (LRU with pinning and dependency protection) runs before
the download. An install that still does not fit fails with `InsufficientStorage { needed,
available }`.

### 3.5 The transport as built (measured 2026-11-09)

Built in `crates/teigeodata-sync`, module `transport` (step 4.4, D-112,
[transport](../../04-architecture/transport.md)). The pack was rebuilt with place elevations
(D-111) since §3.1, so the numbers are on the newer Nepal main pack: 11,748,636 B, 14 sections,
SHA-256 `c037e178…e292347`, read in place. Compressed with C zstd 1.5.7 through the `zstd`
crate, decoded with `ruzstd` 0.9.1; `cargo run --release -p teigeodata-sync --example
transport_measure -- <pack> 3 9 19`, on this Mac at load average 17–18.

| Level | Whole-file zstd B | Asset B | Over whole-file | Frames | Write with self-check | `ruzstd` decode | C zstd decode |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 3 | 8,620,266 | 8,653,556 | +0.39 % | 23 | 0.14 s | 284 MB/s | 1,225 MB/s |
| 9 | 8,446,881 | 8,518,810 | +0.85 % | 23 | 0.25 s | 320 MB/s | 1,330 MB/s |
| 19 | 8,146,743 | 8,227,234 | +0.99 % | 23 | 1.6–2.8 s | 258–277 MB/s | 1,040–1,059 MB/s |

- The `zstd -19` CLI gives 8,144,897 B for the whole file (it adds a checksum but tunes
  slightly differently), so the asset costs +1.0 % against it as well.
- **The cost is the two effects of §3.1 together:** sections as frames (+0.2 %) and 1 MiB
  pieces of the six large sections (NAMES, SEARCH, ARCS in 3; REGION, ADMIN_REGION in 2). It
  stays within the 1 % the summary allowed, just. Larger pieces would save a few tenths of a
  percent (fixed 4 MiB frames: +0.7 % in §3.1) at four times the loss per interruption and the
  decoder's memory, so 1 MiB stays.
- **The seek table** is 201 B: 8 B per frame plus 17 B.
- **Decode:** the whole pack in about 40 ms with `ruzstd`, four times slower than C zstd and
  still far faster than any download.
- **Round trip:** `zstd -d -c` of the level-19 asset equals the pack byte for byte (`cmp`), and
  `zstd -l` lists 24 frames (23 data frames and the skippable seek table).

Bytes to fetch one section at level 19 (each is one range request):

| Section | Raw B | Frames | Fetch B |
|---|---:|---:|---:|
| head | 1,344 | 1 | 785 |
| IDMAP | 35,784 | 1 | 2,647 |
| PLACES | 683,020 | 1 | 602,351 |
| HIERARCHY | 388,948 | 1 | 118,901 |
| NAMES | 2,227,168 | 3 | 917,602 |
| SEARCH | 2,466,500 | 3 | 1,603,881 |
| PLACE_INDEX | 56,688 | 1 | 54,263 |
| ARCS | 2,659,828 | 3 | 2,462,395 |
| POLYGONS | 21,400 | 1 | 20,335 |
| POLY_INDEX | 8,180 | 1 | 7,509 |
| REGION | 2,007,656 | 2 | 1,605,839 |
| POSTAL | 10,144 | 1 | 7,284 |
| BLOCK_HASHES | 20,544 | 1 | 20,554 |
| ADMIN_REGION | 1,066,180 | 2 | 719,289 |
| ARCS_OVERVIEW | 94,780 | 1 | 83,398 |

A section's fetch includes its trailing padding (under 64 B), and BLOCK_HASHES's the 32-byte
footer.

**What the building taught.**
- The planner lives in `sync::transport` beside the seek table, not in a separate `sync::plan`:
  the install planner of step 4.5 (which sections to fetch, copy or verify) builds on it.
- `Layout` takes the file length and section spans rather than a whole `Pack`, because a ranged
  client holds only the head frame. A head-only parser in `teigeodata-format` is needed for 4.5.
  Built (D-166): `PackHead::parse(head, file_len)`, which `Pack::open_head` now runs, and
  `SeekTable::{head_frame, frames_from_head}`, so the table and the head frame (two requests)
  give the plan. The file length comes from the table's decoded sizes and must equal the
  head's own.
- The ranged install flow of §3.4 is built as a sans-I/O state machine (D-167,
  `sync::install`). Two refinements: with the catalog entry the table's and the head frame's
  lengths are known, so the two first requests run at once; and `BLOCK_HASHES` is planned as
  its own first fetch, because a run of adjacent frames can hold covered frames before it in
  file order, which could not be checked on arrival.
- The writer takes the compressor as a closure (as BLOBS does), so C zstd never reaches the
  runtime crate, and checks each frame with `ruzstd`, the decoder clients run.
- A lying table that keeps the total but shifts compressed sizes between frames passes the
  table check by construction. It is caught when its frame fails to decode, never by luck: the
  fuzz harness asserts that any accepted output under a table lie is the pack itself.

## 4. Options compared

**Transport form.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| zstd seekable, frames on head and sections (≤ 1 MiB) | 0.2–0.8 % over whole zstd; ranges, resume, partial and section updates all work; decoder already in | seek table parser and frame writer to build (small) | BSD-3 / GPL-2 (zstd); MIT (`ruzstd`) | format v0.1.0; zstd 1.5.7 (2025-02-19); `ruzstd` 0.9.1 | **Use** |
| Raw `.tgd` | no decoder; simplest | 43 % more bytes on Nepal | n/a | n/a | Allowed per target (`transport: raw`), e.g. for DEM tiles that do not compress |
| Whole-file zstd | smallest zstd form | no ranges | as above | as above | Reject |
| xz | 2.5 % smaller | new decoder; slow; no ranges in practice | 0BSD / public domain (xz) | 5.8.4 | Reject |
| Two assets (raw + zstd) | simple client | doubles assets and hashes | n/a | n/a | Reject |

**Delta.**

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Section re-download over seekable frames | no extra assets; same path as repair and resume; measured 30–42 % of full for one-area releases | every place change reorders all place-keyed sections | n/a | n/a | **Primary** |
| Delta packs as layers | modelled ~50× smaller for sparse place changes | query merge cost; compaction policy; forge must emit them | n/a | n/a | **A/B on two real releases** |
| zstd `--patch-from` per section | small code (decoder with dictionary) | measured −3.4 % only; extra assets per version pair | BSD-3 | zstd 1.5.7 | **Drop** (evidence §3.3) unless the real-release A/B disagrees |
| bsdiff, Courgette, Zucchini | strong on executables | not executables here; slow | BSD | Chromium | Reject |
| RFC 3229, RFC 9842 | standard HTTP | need server logic; RFC 9842 same-origin | IETF | 2002; 2025-09 | Reject for v1; 9842 noted for a hosted tier |
| zsync, casync/desync, zchunk | proven chunking | multi-range or asset-count limits on GitHub; our sections already chunk | various | see §3.3 | Reject |

## 5. What we build ourselves, and why

1. **Seekable writer** in the release tooling:
   - frames on the head, on each section, and on 1 MiB pieces of large sections at 16 KiB
     boundaries;
   - the seek table without checksums;
   - a self-check that decompresses the whole file and compares it with the pack, byte for byte.

   Proof: `zeekstd` and the reference `zstd` CLI both read our files (built, §3.5).
2. **Seek-table reader and frame planner** in `sync::transport` (sans-I/O; built, §3.5).
   Proof: fuzzing of the seek table and of frame-to-section mapping; a lying seek table, a frame
   that decodes past its size, and a frame across a section boundary are all rejected.
3. **Streaming verify-then-write** using `Pack::blocks` (D-078) on decoded frames.
   Proof: a planted bad byte in each frame is caught before it is written.
4. **Delta pack emission** in the forge, for the A/B only: changed rows plus tombstones, as a
   pack of the same layout.
   Proof: base + delta answers equal the new full pack on every golden query.

## 6. Impact on the plan

- **Phase 4** ([phase file](../../07-planning/phases/phase-4.md)): transport and delta steps.
- **ADR-0012 / ADR-0037:** the transport asset is a seekable zstd of the pack. The TUF target is
  that asset, and `custom` carries the raw pack's hash. zstd patch assets are dropped from the
  candidates on this evidence, pending the real-release A/B. Record both as decision-log rows
  when the A/B runs.
- **Benchmarks:**
  - S2 and S3 gain the frame-size row (256 KiB, 1 MiB, per section) and the bytes per release
    type of §3.3;
  - S2 adds a background whole-file case;
  - Z1 gains the transport size beside the raw size.
- **Pack format** (later note): IDMAP 0.074, HIERARCHY 0.311 and NAMES 0.408 under zstd say
  those codecs leave redundancy. Candidates for a format pass, not a Phase 4 item.
- **Forge:** to make section reuse more frequent, keep section bytes stable where possible. For
  example, a timezone or boundary release should not reorder places.

## 7. Risks and open items

- **The delta numbers come from a stand-in pair.** Main and lite differ more than two monthly
  releases will. The real A/B (gate 11 in [offline loading and
  trust](offline-loading-and-trust.md)) decides between section re-download alone and delta
  packs.
- **Hilbert reordering** makes any place change touch every place-keyed section. If real
  releases change places every month, section re-download saves little for place data. That
  makes delta packs, or splitting place sections into stable sub-ranges (a format change),
  worth their cost.
- **iOS background ranged tasks** through GitHub's redirect: **unverified**.
- **`BGContinuedProcessingTask`** behaviour and limits: **unverified**.
- **Decompression bombs:** every frame's output is capped at its declared size, which must
  equal what the section table implies. A fuzz target covers it.
- **Measurements were on a loaded Mac** (load average 10–16), sizes only. Sizes do not depend on
  load. No speed numbers are claimed.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| zstd (library and CLI) | 1.5.7 (2025-02-19), latest release | 2026-11-09 | GitHub releases API; local `zstd -V` |
| zstd seekable format | v0.1.0 | 2026-11-09 | facebook/zstd `contrib/seekable_format` |
| xz | 5.8.4 (local) | 2026-11-09 | `xz --version` |
| zstd crate / ruzstd | 0.14.3 / 0.9.1 | 2026-11-09 | crates.io API |
| zeekstd (dev-dependency of `teigeodata-sync`, the oracle) | 0.6.3 (2026-10-19), over `zstd-safe` 8.2.0 | 2026-11-09 | crates.io API |
| zeekstd / zstd-seekable | 0.6.3 (2026-10-19, BSD-2-Clause, over `zstd-safe`) / 0.1.23 (2023-06-12) | 2026-11-09 | crates.io API |
| qbsdiff | 1.4.4 (2025-08-24) | 2026-11-09 | crates.io API |
| zchunk | tag 1.5.5 | 2026-11-09 | GitHub tags API |
| casync / desync | tag v2 (2017-07-26) / v1.1.5 (2026-10-17) | 2026-11-09 | GitHub API |
| OSTree | v2026.6 (2026-10-30) | 2026-11-09 | GitHub releases API |
| RFC 9842 | September 2025, Standards Track | 2026-11-09 | rfc-editor.org |
| GitHub release asset encoding | no `content-encoding`; `206` on range | 2026-11-09 | curl against `BurntSushi/ripgrep` 14.1.1 asset |

## 9. Sources

- [Zstandard seekable format](https://github.com/facebook/zstd/blob/dev/contrib/seekable_format/zstd_seekable_compression_format.md), facebook/zstd, v0.1.0, accessed 2026-11-09.
- [zstd releases](https://github.com/facebook/zstd/releases), accessed 2026-11-09.
- [Zstandard as a patching engine](https://github.com/facebook/zstd/wiki/Zstandard-as-a-patching-engine), facebook/zstd wiki, 2020.
- [RFC 9842: Compression Dictionary Transport](https://www.rfc-editor.org/rfc/rfc9842.html), Meenan and Weiss, September 2025.
- [RFC 3229: Delta encoding in HTTP](https://www.rfc-editor.org/rfc/rfc3229.html), Mogul et al., January 2002.
- [Zucchini README](https://chromium.googlesource.com/chromium/src/+/HEAD/components/zucchini/README.md), Chromium, accessed 2026-11-09.
- [Smaller is faster (and safer too)](https://blog.chromium.org/2009/07/smaller-is-faster-and-safer-too.html), Chromium Blog, 2009-07-15 (bsdiff and Courgette sizes).
- [zchunk](https://github.com/zchunk/zchunk), accessed 2026-11-09.
- [OSTree formats: static deltas](https://ostreedev.github.io/ostree/formats/), accessed 2026-11-09.
- [folbricht/desync](https://github.com/folbricht/desync) and [systemd/casync](https://github.com/systemd/casync), accessed 2026-11-09.
- [Android 16 behaviour changes: all apps](https://developer.android.com/about/versions/16/behavior-changes-all), Android Developers, accessed 2026-11-09.
- [BGContinuedProcessingTask](https://developer.apple.com/documentation/backgroundtasks/bgcontinuedprocessingtask), Apple, accessed 2026-11-09 (page body did not render; facts marked unverified).
- Internal: [offline loading and trust](offline-loading-and-trust.md) (GitHub measurements, platform rules, gates), [block hashes](block-hashes.md), [pack format](../../04-architecture/pack-format.md), [phase 1 benches](../benches/phase-1.md) (`ruzstd` size).
- Measurements: Nepal main and lite packs of 2026-11-09 (`data/build/pack/np/`), section tables parsed with a throwaway Python script, compressed with zstd 1.5.7 and xz 5.8.4 on this Mac, 2026-11-09.
