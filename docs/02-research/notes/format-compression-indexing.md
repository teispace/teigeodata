---
title: "Format, compression and indexing techniques for .tgd packs"
status: research
updated: 2026-11-06
owner: research-agent
revisit: 2027-02-06
---

# Format, compression and indexing techniques for .tgd packs

## 1. Question

What are the best-known techniques and the best Rust crates for each part of a `.tgd` pack: integer columns, strings, nullable columns, ID maps, the text index, spatial indexes, the exact region index, polygons, DEM rasters, zero-copy parsing, and integrity? For each one we either use a crate or build our own. The answer feeds:

- [ADR-0009](../../03-decisions/adr/ADR-0009-pack-format.md): the section layout and codecs of `.tgd` v1.
- [ADR-0013](../../03-decisions/adr/ADR-0013-region-index.md): the exact region index.
- [ADR-0014](../../03-decisions/adr/ADR-0014-names-and-search.md): the search index.
- [ADR-0015](../../03-decisions/adr/ADR-0015-tree-shaking.md): which Cargo feature each codec sits behind.
- The future `docs/04-architecture/pack-format.md` specification.

Constraints that drive every verdict:

- The reader core is `no_std` + `alloc`, small in WASM, and zero-copy over a memory map or an `ArrayBuffer`.
- Reads of untrusted files are safe and bounds-checked.
- Builds are reproducible: the same input gives the same bytes.
- Speed targets: reverse lookup in about 100–200 ns, kNN in µs, search under 100 µs.

## 2. Summary and recommendation

**Rule of thumb.** The reader (runtime) side stays small and owned by us. Each runtime decoder is a few hundred lines of safe Rust over `&[u8]`. Heavy crates run in the **builder** (std, native only) or act as **test oracles**. Most good Rust crates own their memory in `Vec`s and serialise in their own way. They do not read in place from a byte slice we define, and that rules them out as the runtime for a specified, stable file format.

**Use as-is (runtime):**

- `zerocopy` 0.8: endian-safe, unaligned, validated struct views.
- `blake3` 1.8 (with `default-features = false`): checksums.
- `ed25519-dalek` 3.0: signatures.
- `fst` 0.4: term dictionary. It reads in place over any `AsRef<[u8]>`. Note that it is std-only.
- `ruzstd` 0.9: pure-Rust, `no_std` zstd decoder, for cold sections only and only if A/B shows zstd pays off.
- `memmap2` 0.9: native mapping only.

**Use in the builder or as oracles:**

- `zstd` (C), `fsst-rs`, `ptr_hash` / `ph`, `geo`, `robust`, `i_overlay`, `polylabel`, `rstar`, `kiddo`, `geo-index`, `tzf-rs`, `h3o`.
- Fuzzing: `cargo-fuzz` + `arbitrary` (or `bolero`).

**Build ourselves (runtime readers):**

1. Frame-of-reference bit-packing in FastLanes-style blocks, plus delta/zigzag.
2. The FSST decoder and a block string pool.
3. A rank/select directory and Elias–Fano.
4. A PtrHash-style MPHF reader, if it beats a sorted-ID search (A/B).
5. Top-k completion as a prefix range plus an RMQ over scores.
6. A flatbush-layout packed Hilbert R-tree reader with integer boxes and best-first kNN.
7. An S2ShapeIndex-style quadtree region index with clipped edges and exact integer crossing tests.
8. A TopoJSON-style arc store with per-vertex LOD thresholds.
9. A near-lossless DEM codec: ±1 m quantisation, MED prediction and an entropy coder.
10. A minimal TUF-style catalog trust scheme.

**Key design choice for exactness.** All geometry is quantised once to a fixed integer grid. The proposal is i32 at 1e-7°, about 1.1 cm (the same scale as PMTiles positions). That integer geometry is the "source of truth". With integer coordinates and cell-local offsets, orientation and crossing tests are exact in i64, so the runtime needs no floating-point robust predicates. "Exactly equal to the source polygons" then means exactly equal to the quantised source. This is a stated, testable contract. **It must be accepted in ADR-0007/0013.**

**Unverified:**

- PHast availability inside the `ph` crate.
- `no_std` status of `bitpacking`, `fastlanes`, `geo-index`, `bao-tree`, `vers-vecs`.
- All DEM bytes-per-km² estimates.

## 3. Findings

### 3.1 Integer coding

**Techniques and papers**

- **Bit-packing with frame of reference (FOR).**
  - SIMD-BP128 and FastPFor (Lemire & Boytsov, *Decoding billions of integers per second through vectorization*, SPE 2015) pack blocks of 128 integers with one bit width per block. They decode at billions of integers per second. PFor/FastPFor add exceptions for outliers.
  - **FastLanes** (Afroozeh & Boncz, VLDB 2023) uses a 1024-value "virtual 1024-bit register" interleaved layout. It auto-vectorises on any ISA from plain scalar code, so it suits WASM SIMD128 and NEON without intrinsics. It also defines a delta transposition that keeps delta decoding data-parallel.
- **StreamVByte** (Lemire, Kurz, Rupp, IPL 2018) separates control bytes from data bytes for SIMD byte-oriented varints. It is good when values are not clustered by width, but it is weaker than bit-packing for narrow, homogeneous columns, and random access needs block offsets anyway.
- **Varint / LEB128.** Simple and compact for small headers and directories. PMTiles uses it for its whole directory. It is serial to decode and has no random access.
- **Delta + zigzag** (zigzag as in protobuf). This is the standard way to make signed deltas small. It is used by TWKB, Geobuf and MVT, and it is what our polygon arcs and sorted IDs need.
- **Random access.** Fixed-width-per-block bit-packing gives O(1) access to element *i*: read the block header (base, width), then shift and mask. Delta-coded blocks need a prefix sum inside the block, so they need small blocks (128) or stored per-block anchors.

**Hilbert vs Morton.** Moon, Jagadish, Faloutsos & Saltz (*Analysis of the clustering properties of the Hilbert space-filling curve*, IEEE TKDE 2001) show that Hilbert ordering gives fewer clusters per query region than Z-order. Morton has long jumps at quadrant boundaries. Morton is cheaper to compute (bit interleave; BMI2 `pdep`), and its prefixes are exactly quadtree cells.

Recommendation:

- **Hilbert** for row order (places, polygons) and the packed R-tree. Locality there drives page-ins and compression.
- **Morton** for region-index cell keys. Prefix = ancestor, which keeps descent trivial.

**Crates (checked 2026-11-06)**

| Crate | Version / date | Downloads | no_std / WASM | Notes |
|---|---|---|---|---|
| `bitpacking` | 0.9.3, 2026-01-08 | 29.4 M | No `no_std` attribute in `lib.rs` (std). SIMD on x86 only; scalar elsewhere (**unverified** for wasm SIMD) | Used by tantivy. Block sizes 32/128/256. MIT |
| `fastlanes` | 0.7.2, 2026-09-02 | 2.9 M | **unverified** | SpiralDB (Vortex). Apache-2.0. MSRV 1.91 |
| `stream-vbyte` | 0.4.1, 2023-05-23 | 0.85 M | x86 SIMD features only | Licence field "non-standard". Stale |
| `integer-encoding` | 4.1.0, 2025-10-28 | 120 M | std I/O traits | varint + zigzag. MIT |
| `leb128` | 0.2.7, 2026-06-15 | 88 M | std I/O | |
| `fast_hilbert` | 2.1.0, 2026-02-12 | 0.47 M | likely `no_std` (**unverified**) | Builder only. MIT |
| `morton-encoding` | 2.0.1, 2021-03-26 | 0.19 M | | Stale. A Morton encoder is about 10 lines anyway |

**Verdict: build our own** FOR + bit-packing codec in FastLanes-style 1024-value blocks (or 128-value blocks, to be decided by A/B), written as plain loops that auto-vectorise, `no_std`, safe. Reasons:

- The core needs the decoder only, and it is about 200 lines.
- It must read in place from our own layout.
- `bitpacking` is std and x86-SIMD-centred.
- `fastlanes` has an unverified no_std status and a fast-moving 0.x API.

`bitpacking` and `fastlanes` become A/B baselines. Varint only in the header, manifest and directories. Hilbert curve code is ours (about 40 lines, builder only), with `fast_hilbert` as a cross-check.

### 3.2 String compression with random access

**FSST** (Boncz, Neumann, Leis, *FSST: Fast Random Access String Compression*, PVLDB 13(11), 2020):

- A static table of up to 255 symbols, each 1–8 bytes. Codes are single bytes; 0xFF escapes a literal.
- It compresses each string independently, so random access to string *i* costs only the decoding of that string. It is about as fast as LZ4 to decode and gives better ratios on short strings.
- It suits place names: millions of short strings that must be decoded one at a time.
- The decoder is about 30 lines plus a 2 KB table. The encoder (symbol-table training) is the hard part, and it belongs only in the builder.

**Front coding** (Witten, Moffat, Bell, *Managing Gigabytes*). It stores sorted strings as (shared-prefix length, suffix) in buckets of *k* with a full string at each bucket head. It is the classic dictionary for sorted keys. The FST already does better for search keys, so front coding is only a fallback for a sorted postal-code list.

**zstd with trained dictionaries.**

- Small records (under 1 KB) compress poorly alone. A trained dictionary (COVER algorithm, `zstd --train`) recovers much of the ratio.
- Random access needs block-level framing: compress blocks of N records with the dictionary and index the blocks. Decoding a block costs µs, which suits cold data (attribution, long descriptions, metadata), not hot names.
- `zstd` (0.14.3, 2026-10-16, BSD-3) wraps C libzstd through `zstd-sys` (2.1.2+zstd.1.5.7). It is the best encoder.
  - For `wasm32-unknown-unknown` it needs a C toolchain (clang targeting wasm), and it adds about 100+ KB to the binary (**unverified** size).
- `ruzstd` (0.9.1, 2026-10-11, MIT) is pure Rust and `#![no_std]` with `alloc`. Its decoder is production-quality. It has a `dict_builder` feature, and its encoder is limited, so we encode with C zstd in the builder.
- **Verdict:** if a section uses zstd, readers decode with `ruzstd` everywhere (one implementation, WASM-clean) and the builder encodes with `zstd`. A/B must check that the ruzstd decode speed is acceptable.

**LZ4.**

- `lz4_flex` (0.14.0, 2026-07-14, MIT) is pure Rust, `no_std` (block format), and has `safe-decode` (no unsafe on decode). It is very fast to decode and gives a lower ratio than zstd.
- `lz4` (1.28.1, 2025-01-07) wraps C.
- **Verdict:** `lz4_flex` with `safe-decode` is the candidate for warm blocks where zstd decode latency is too high.

**FSST crates.**

- `fsst-rs` (0.7.0, 2026-10-27, Apache-2.0, SpiralDB; MSRV 1.86) has `unsafe` in `lib.rs`. It has no `no_std` attribute and supports little-endian only. Its README says it is "still in-progress and is not production ready".
- `fsst` (14.0.2, 2026-10-29) is Lance's implementation, versioned with Lance (MSRV 1.94).
- **Verdict: build our own FSST decoder** (safe, `no_std`, bounds-checked over our symbol-table layout). Train symbol tables in the builder, either with our own implementation of the paper's algorithm or with `fsst-rs`, provided its output is deterministic (**unverified**). Use `fsst-rs` as a round-trip oracle.

**Multi-script note.** Devanagari in UTF-8 is 3 bytes per code point. FSST symbols up to 8 bytes capture common aksharas and suffixes, so expect better gains than on Latin text (**unverified**). Train one table per script or per language group, and measure against a single shared table.

### 3.3 Succinct structures (rank/select, Elias–Fano)

**Techniques**

- **Rank/select bitvectors.**
  - Jacobson 1989 introduced them.
  - Vigna's *Broadword implementation of rank/select queries* (WEA 2008) gives rank9 and select9.
  - Zhou, Andersen, Kaminsky (*Space-efficient, high-performance rank and select structures on uncompressed bit sequences*, SEA 2013) give "poppy", with 3.125% overhead.
  - These serve nullable component columns: presence bit + `rank1(i)` = index into the dense value array. This is how "absent data costs about 1 bit" in ADR-0009 is done.
- **Elias–Fano.** Elias 1974 and Fano 1971. Vigna's *Quasi-succinct indices* (WSDM 2013) is the practical engineering. Partitioned EF (Ottaviano & Venturini, SIGIR 2014) adapts to clustering.
  - EF takes 2 + log(U/n) bits per element and gives O(1) access and fast successor (`next_geq`).
  - It serves sorted ID lists (posting lists, child lists, per-cell region lists) and monotone offset arrays (string offsets).

**Crates**

| Crate | Version / date | no_std | Zero-copy from `&[u8]` | Notes |
|---|---|---|---|---|
| `sucds` | 0.11.0, 2026-10-25 | Yes (disable `std`) | No (own serialisation into owned types; **unverified**) | MIT/Apache. Rank9Sel, DArray, EF, wavelet matrix |
| `vers-vecs` | 1.10.2, 2026-08-10 | **unverified** | No (**unverified**) | Very fast rank/select, EF, RMQ, BP trees. Some modules `#[forbid(unsafe_code)]`, SIMD feature |
| `succinct` | 0.5.2, 2019-08-29 | | | Stale |
| `sux` + `epserde` | 0.15.4 / 0.13.2, 2026 | std | ε-copy mmap through epserde (unsafe, own format) | Vigna's group. Best-in-class, but brings its own format |
| `bitm` | 0.6.0, 2026-10-29 | | | Used by `ph` |

**Verdict: build our own** reader-side rank directory (rank9 or poppy layout), select hints (sampled) and Elias–Fano, as views over `&[u8]`. Use `sucds` / `vers-vecs` as oracles and speed baselines. None of the crates reads a layout we specify in place without unsafe.

### 3.4 Minimal perfect hashing (stable u64 ID → row)

**Need.** Rows are in Hilbert order for locality, so the ID → row map cannot be "rows sorted by ID". The options:

- (a) A sorted (id, row) permutation searched with Eytzinger or interpolation search.
- (b) An MPHF over the IDs plus a check against the stored ID column for membership. Packs merge, so unknown IDs must be rejected, and the check is required.

**Techniques (bits/key at the paper's settings)**

- **PtrHash** (Groot Koerkamp, *PtrHash: Minimal Perfect Hashing at RAM Throughput*, SEA 2025).
  - About 3.0 bits/key by default; 2.15 compact.
  - About 12 ns/key in a query loop at n = 10⁹, and 8 ns streaming. At our n (10⁵–10⁷) the table fits in cache, so queries should be a few ns (**unverified**).
  - Construction takes about 350 ns/key, single-threaded by default.
  - It is a PTHash derivative (Pibiri & Trani, SIGIR 2021) with 8-bit pilots and a CachelineEF remap.
- **PHast** (Beling & Sanders, arXiv 2504.17918, ALENEX 2026). Under 2 bits/key, claimed fastest evaluation, fast construction. A Rust implementation is by Beling in `bsuccinct-rs`; whether `ph` 0.11.0 ships it is **unverified**.
- **FMPH / FMPHGO** (Beling, *Fingerprinting-based minimal perfect hashing revisited*, ACM JEA 2023). About 2.8 / 2.1 bits/key, in the `ph` crate.
- **BBHash** (Limasset et al., SEA 2017). About 3 bits/key, slower queries (several random accesses). The `boomphf` crate is at 0.6.0 (2023-07-26), stale.
- **CHD** (Belazzougui, Botelho, Dietzfelbinger, ESA 2009). About 2.07 bits/key, slower queries. `phf` (0.14.0) is a compile-time map for static Rust tables, not runtime data.

**Determinism.**

- All of these use a seed and retry on failure. With a fixed seed and single-threaded or deterministically partitioned construction, the output is deterministic. Multi-threaded construction (rayon) is a risk.
- PtrHash's and ph's serialisation (epserde / own) is not our format.
- **Verdict: build a PtrHash-style reader** (bucket function, u8 pilot array, Elias–Fano remap) over our layout, with deterministic construction in our builder (fixed seed recorded in the section header). Use the `ptr_hash` crate (2.1.3, 2026-10-21, MIT, std) and `ph` (0.11.0, 2026-02-12) as references and speed baselines.
- **A/B** against option (a): it costs no extra structure beyond 4 bytes/row, but it costs about log₂(n) cache misses (~50–100 ns at 10⁷). It may be good enough, since ID lookups are rarer than spatial queries.

### 3.5 Text index: FST, fuzzy and top-k autocomplete

**fst** (BurntSushi; 0.4.7, 2021-06-06; Unlicense/MIT; 33 M downloads):

- Daciuk-style minimal acyclic transducers (Daciuk et al., *Incremental construction of minimal acyclic finite-state automata*, CL 2000). Keys must be inserted in sorted order, and each key carries a u64 output.
- `Fst<D: AsRef<[u8]>>` reads in place over an mmap. It supports prefix, range, the `Automaton` trait (regex via `regex-automata` in older `fst-regex`), and Levenshtein behind a feature.
- The crate is **std-only** (no `no_std` attribute; uses `std::io`). All three targets have std, so the cost is only `no_std` purity.
- The format has been stable since 2021. "Last release 2021" reflects maturity, not abandonment (**unverified** maintenance stance).

**Levenshtein.**

- `fst`'s built-in Levenshtein builds a DFA eagerly and can blow up.
- `levenshtein_automata` (0.2.1, 2021, MIT; tantivy) implements Schulz & Mihov's universal/parametric automata (*Fast string correction with Levenshtein automata*, IJDAR 2002). The DFA is built once per (distance, transposition) pair and applied per query, which is much faster.
- `tantivy-fst` (0.5.0, 2023-11-21) is tantivy's fork with this integration.

**Top-k autocomplete.**

- FST outputs are sums along the path. Storing per-subtree max scores in outputs does not compose with sharing.
- The standard solution is Hsu & Ottaviano, *Space-efficient data structures for top-k completion* (WWW 2013):
  - Map each key to its **lexicographic ordinal** (FST output = ordinal).
  - A prefix maps to a contiguous ordinal range [lo, hi), found by two FST walks.
  - A **range-maximum query** (RMQ) over the score array gives the top-k by repeated RMQ with a heap, in O(k log k).
- RMQ options: a sparse table (n log n words, too big), a block sparse table, or a succinct RMQ (Fischer & Heun, SICOMP 2011: 2n + o(n) bits). `vers-vecs` has an RMQ to benchmark against.
- A precomputed top-k for all 1–2-character prefixes removes the worst case (huge ranges for short prefixes).

**Verdict.**

- **Use `fst`** for v1 behind the `search` feature, with ordinals as outputs.
- **Use `levenshtein_automata`** for fuzzy search if it works with `fst` 0.4's `Automaton` trait (**unverified**; it targets tantivy-fst). Otherwise vendor the Schulz–Mihov construction.
- **Build our own** top-k layer: scores (bit-packed) + block RMQ + a short-prefix cache.
- Keep a `no_std` FST reader fork as a later option: the read path is a small part of the crate.
- Postings (ordinal → place IDs, for names shared by many places) are Elias–Fano lists.

### 3.6 Spatial indexes, region index and predicates

**Packed Hilbert R-tree.**

- Static bulk-loaded R-tree with Hilbert-sorted leaves (Kamel & Faloutsos, *Hilbert R-tree*, VLDB 1994). Flatbush (Agafonkin) and FlatGeobuf's index use it: an implicit tree with node size 16, levels stored contiguously, no pointers.
- kNN is best-first search with a priority queue on min-distance (Hjaltason & Samet, *Distance browsing in spatial databases*, TODS 1999). It visits nodes in distance order and is optimal in node accesses.
- Crates:
  - `static_aabb2d_index` 2.1.0 (2026-08-08): flatbush port, `visit_neighbors` kNN, `#![forbid(unsafe_code)]` unless `unsafe_optimizations`. It owns `Vec`s; there is no from-bytes view.
  - `geo-index` 0.4.0 (2026-09-15, Kyle Barron): **flatbush/kdbush ABI-compatible, zero-copy from a byte buffer, safe Rust**, Hilbert and STR sort, `neighbors_with_callbacks`. no_std is **unverified**.
  - `flatgeobuf` 6.0.1 (BSD-2): reads FGB files.
  - `rstar` 0.14.0 (2026-11-01): dynamic R*-tree, `no_std`, forbid(unsafe), owns memory. Best as an oracle.
  - `kiddo` 6.3.0: k-d tree kNN, as an oracle.
- **Verdict: build our own reader** of the flatbush layout, but with **i32 quantised boxes**: 16 B per box instead of 32 B with f64, or relative u16 boxes per node as an A/B. Add best-first kNN with haversine or equirectangular lower bounds. Use `geo-index` as a test oracle; it can read our layout byte-for-byte if we keep the flatbush ABI with f64.
- **A/B: flatbush-ABI f64 (ecosystem compatibility) vs i32 (half the size).** For point places, also A/B a KDBush layout.

**Exact region index (point → admin / timezone).**

- **Google S2ShapeIndex:**
  - It stores, per S2 cell, the edges of each shape *clipped* to that cell (padded), plus a "contains center" bit per shape.
  - Point containment: start from the cell center, whose containment is known, and count edge crossings along the segment center → point. Only edges in that cell are tested.
  - Cells subdivide until each has at most about 10 edges. Point queries are O(1) amortised with exact predicates.
  - The `s2` crate (0.2.0, 2026-08-19) is a partial port and does not include a ShapeIndex (**unverified**). Adopt the design, not the crate.
- **tzf-rs** (2.3.0, 2026-11-02, MIT; data `tzf-rel` is ODbL):
  - A preindex of tiles answers most queries in about 100 ns. The fallback is ray casting accelerated by "YStripes".
  - Measured on an Apple M3 Max with lite data: DefaultFinder ~163 ns/query on a 154k-city sweep, 434 ns at an edge city; EmbeddedFinder (in place) ~950 ns.
  - **The default "lite" data is simplified, with up to 111.7 m boundary displacement.** Only the 14 MB `full` dataset is exact.
  - This confirms that ~150 ns is achievable and that exactness needs full-resolution edges at borders.
- **timezonefinder** (Python):
  - A shortcut grid index; cells inside one zone return without geometry, and border cells run exact PIP on the listed zones.
  - It never simplifies polygons and stores coordinates at about 1 cm (integer scaling).
- **Verdict: build our own**, as ADR-0013 already proposes, with these refinements:
  1. Morton-keyed adaptive quadtree on the quantised i32 grid, plus a top-level dense directory (for example 2¹⁰ × 2¹⁰ root cells, or a per-pack bbox grid) so descent is 1 directory read + ≤3 node reads.
  2. Leaves are either **full(region_id)** or **border(center_region, clipped_edges[])**. This follows S2: a known center label plus a crossing count, not full polygon PIP.
  3. Edges are stored as cell-local u16 or i32 offsets. They are delta-coded only if the decode cost fits the 200 ns budget.
  4. Crossing and orientation tests use **exact integer arithmetic** (i64 on cell-local differences; i128 only if differences exceed 31 bits). No floating-point predicates at runtime.
  5. "Unclaimed" (ocean, disputed) is an explicit region ID, so the result is always defined.
  6. Points exactly on an edge or vertex need a deterministic tie-break rule: half-open "simulation of simplicity" style, lower-left owns.
- **H3** (`h3o` 0.11.1, 2026-11-02, pure Rust, `no_std`, BSD-3): useful as an optional *component* (H3 cell IDs per place) and for analytics export. Not for the exact region index: hexagons do not nest exactly, and clipping is awkward.

**Robust predicates.**

- Shewchuk, *Adaptive precision floating-point arithmetic and fast robust geometric predicates* (DCG 1997). The `robust` crate (1.2.0, 2025-05-10, has a `no_std` feature) implements orient2d and incircle.
- `geo` 0.33.1 uses `robust` for its predicates and offers simplification, PIP, area and others.
- **Verdict:** at runtime, integer exact predicates (no crate). In the builder, `geo` + `robust` + `i_overlay` (9.1.0, integer-based boolean ops) for clipping edges to cells and for validation.

### 3.7 Polygons

- **TopoJSON** (Bostock): shared arcs between adjacent polygons are stored once. Rings are lists of signed arc indices (negative = reversed). Quantisation plus delta encoding. Removing shared borders roughly halves the vertex count for admin layers (**unverified** for our data; to be measured).
- **Topology-preserving simplification.**
  - Visvalingam & Whyatt (*Line generalisation by repeated elimination of points*, Cartographic J. 1993) gives an effective-area ranking.
  - Douglas–Peucker (1973) is error-bounded but not topology-safe.
  - Saalfeld (1999) and de Berg, van Kreveld & Schirra (1998) give topology-preserving variants.
  - **mapshaper's approach:** build a topology first; simplify each arc with junction (shared) vertices fixed; store a removal threshold per vertex so any level is a filter; then repair the segment intersections introduced.
- **Per-vertex LOD threshold** (mapshaper and Cesium-style progressive meshes) lets **one arc store serve all levels of detail**: vertex *v* is kept at LOD *k* if `level[v] ≥ k`. The cost is a few bits per vertex instead of 3–5 duplicate geometries. Lower LODs then lose compression locality (they must skip vertices), so A/B this against separate per-LOD arc sets.
- **Snap rounding** (Hobby 1999; Hershberger, *Stable snap rounding*, CGTA 2013) is the correct way to quantise without creating self-intersections. Run it in the builder on the integer grid, then validate rings.
- **polylabel** (Agafonkin, Mapbox 2016): pole of inaccessibility for label points. The `polylabel` crate (3.3.1, 2026-11-06, licence BlueOak-1.0.0, which is permissive) runs in the builder; the result is stored as one point per polygon.
- **Encodings for comparison:**
  - **TWKB:** varint + zigzag deltas, precision per geometry.
  - **Geobuf:** protobuf, delta ints.
  - **FlatGeobuf:** f64 coordinates, no compression, packed Hilbert R-tree, streamable.
  - **MVT:** tile-local ints, zigzag command stream.
  - Our arcs: i32 quantised → delta → zigzag → bit-packed blocks (random access to arc start through an offsets array), with varint + zstd as the A/B alternative.
  - We expect delta-coded arcs at 1 cm precision to need about 10–16 bits per coordinate for dense borders (**unverified**).
- `topojson` crate 0.5.1 (2022): stale; reference only.

### 3.8 DEM raster compression at a ±1 m bound

- **LERC** (Esri; Lerc2):
  - Bounded max error (MaxZError) per pixel.
  - The image is split into blocks. Each block is quantised with step 2·MaxZError relative to the block minimum and then bit-packed (with Huffman for 8-bit).
  - Measured by LINZ on 1 m lidar DEMs (float32, 3,456 MB input):

    | Compression | Size |
    |---|---|
    | LERC at 1 mm max error | 1,319 MB (2.62:1) |
    | ZSTD level 17 + predictor 2 | 1,859 MB (1.86:1) |
    | DEFLATE level 9 + predictor 2 | 1,982 MB (1.74:1) |

    LINZ chose LERC for its DEM COGs. That was at 1 mm; at ±1 m the ratio is far higher.
  - The `lerc` crate (0.2.1, 2025-06-20) wraps C++ via `lerc-sys`, so it is not WASM-friendly.
- **Near-lossless predictive coding.**
  - The JPEG-LS near-lossless mode (Weinberger, Seroussi, Sapiro, *LOCO-I*, IEEE TIP 2000) and CALIC (Wu & Memon 1997).
  - Predict each pixel from *reconstructed* neighbours (MED predictor), quantise the residual with step 2δ+1, and entropy-code it. This gives a **guaranteed |error| ≤ δ** with much better prediction than LERC's per-block min.
  - With δ = 1 m on SRTM-like terrain, most residuals fall in {−2..2}, so we expect about 1.5–3 bits/pixel after entropy coding (**unverified estimate**).
- **Terrarium / Mapbox terrain-RGB:**
  - Lossless PNG of elevation packed into RGB. Terrain-RGB has a 0.1 m step; Terrarium has 1/256 m.
  - They are easy to serve but waste precision and use PNG's weak predictor and deflate. They are far larger than ±1 m near-lossless coding.
  - Export formats only.
- **Quantized-mesh** (Cesium): a TIN with an error bound per tile. It is excellent for rendering (few vertices in flat areas) but costs triangle location per point query and is hard to bound per pixel. Not for v1.
- **COG:** a container (tiled TIFF with overviews) whose codecs are LERC, ZSTD or DEFLATE. Useful as an *export* and as a *source* format, not for our runtime.

**Bytes per km² estimate (unverified, to be measured).**

| Grid | Pixels/km² | Bytes/km² at 2–3 bits/px | Global land (~1.49 × 10⁸ km²) |
|---|---|---|---|
| 1″ (~30 m) | ~1,100 | ~275–410 B | ~40–60 GB |
| 3″ (~90 m) | ~123 | ~30–46 B | ~4.5–7 GB |

**Verdict: build our own** near-lossless codec:

- 256×256 tiles, int16 quantised at the chosen step.
- MED prediction in the reconstructed domain, zigzag residuals.
- Entropy coding: A/B (i) a static per-tile Huffman or rANS (small `no_std` decoder) vs (ii) zstd via ruzstd vs (iii) LERC-style block bit-packing.
- Tiles are directly addressable through a tile directory; a point query decodes one tile, so cache recently decoded tiles.

### 3.9 Zero-copy and parsing safety

| Option | Strengths | Weaknesses |
|---|---|---|
| `zerocopy` 0.8.63 (2026-10-13; BSD-2/Apache/MIT; Google; MSRV 1.56) | `FromBytes`, `KnownLayout`, `Immutable` derives with compile-time soundness checks. `Ref::from_prefix` does bounds/alignment-checked views. **`byteorder::little_endian::{U16,U32,U64}` are unaligned and endian-explicit**, so they are correct on big-endian and need no alignment. `no_std`. Reader code can be `#![forbid(unsafe_code)]` | Derive-heavy compile times |
| `bytemuck` 1.25.2 (2026-07-19) | Simple `Pod`/`cast_slice`. `no_std` | Requires alignment for `cast_slice`. No endian types, so a big-endian reader would be wrong |
| `rkyv` 0.8.18 (2026-08-05; MIT) | Fast zero-copy archives. `bytecheck` validation | The format is defined by rkyv's internals and versions, not by a spec we own. Validation costs O(size) on open, or unsafe unchecked access. Hard to read from other languages |
| Hand-written validated reader (on zerocopy types) | We own the spec. Each section is validated once on open (offsets, lengths, monotone offset arrays, max bit width), then accessed with cheap checks. Cross-language readers are possible | More code to write and fuzz |

**Verdict:** a hand-written validated reader using **`zerocopy` little-endian types**.

Alignment:

- Sections are aligned to 64 bytes (cache line; enough for SIMD loads).
- The file never *needs* alignment, because the LE types are unaligned. Alignment is only for speed.
- WASM receives an `ArrayBuffer` copy, and alignment survives if the buffer base is 8-aligned (it is).

`memmap2` 0.9.12:

- Native only.
- A mapping is unsound if another process truncates or modifies the file. Document that packs are immutable once installed (write to a temp file, then rename). Optionally offer "read into memory" mode.
- iOS and Android support mmap. On WASM, read into memory.

Fuzzing:

- `cargo-fuzz` 0.13.2 with `libfuzzer-sys` 0.4.13 (nightly) and `arbitrary` 1.5.0 for structure-aware inputs; `bolero` 0.13.7 for one harness running as a property test on stable and a fuzzer on nightly.
- Targets:
  1. Raw bytes → open + every query.
  2. Arbitrary model → builder → reader round-trip equality.
  3. Each codec decoder alone.
- Also run Miri on the reader tests and keep `#![forbid(unsafe_code)]` in the reader crate if benchmarks allow.

### 3.10 Integrity and trust

- **BLAKE3** (O'Connor, Aumasson, Neves, Wilcox-O'Hearn, spec 2020). The `blake3` crate 1.8.7 (2026-08-20; CC0/Apache):
  - It is `no_std` with `default-features = false`, has `pure` and `wasm32_simd` features, and is a Merkle tree internally.
  - Hashing at several GB/s per core on native is its documented class (**unverified** on our devices).
- **Bao**: verified streaming over BLAKE3's tree. An outboard tree lets you verify any byte range without hashing the whole file.
  - `bao-tree` 0.17.0 (2026-10-22, n0/iroh) supports chunk groups (for example 16 KiB), which shrink the outboard to well under 1% (**unverified** exact figure). Its `no_std` status is **unverified**, and it pulls in more dependencies.
  - `abao` 0.2.0 (2023) is stale; `bao` 0.13.1 (2025-04-07) is the original.
  - Value for us: lazy, range-read packs (large DEM packs over HTTP, or partially paged-in files) can verify each block on first touch.
- **Ed25519**: `ed25519-dalek` 3.0.0 (2026-07-06; BSD-3; MSRV 1.85), `no_std`. Use `verify_strict` to reject malleable signatures.
- **minisign**: `minisign-verify` 0.3.0 (2026-09-25; MIT; zero dependencies) is a tiny verifier for detached minisign signatures (Ed25519 over a BLAKE2b prehash). It is good for signing catalog and index files that people may also verify with the `minisign` CLI.
- **TUF** (Samuel, Mathewson, Cappos, Dingledine, CCS 2010):
  - Roles root, targets, snapshot and timestamp, with thresholds and expiry. It protects against rollback, freeze, mix-and-match and key compromise.
  - `tough` 0.24.0 (2026-07-10; AWS) is a full client: std, heavy, HTTP transport.
  - **Verdict:** a **minimal TUF-style scheme** for the catalog in v1:
    - A root key set with a threshold, signed by the offline root.
    - A signed catalog carrying a monotonic version, an expiry, and per-pack BLAKE3 plus length.
    - A short-lived timestamp signature.
    - Rotation through a new root signed by the old root.
    - Keep the field names TUF-compatible so `tough` can be adopted server-side later.

**Verdict for packs:**

- The header and section table carry a BLAKE3-256 per section.
- An Ed25519 signature covers BLAKE3(header ‖ section table ‖ manifest).
- On open, the reader verifies the signature (~50 µs class, **unverified**) and either hashes all sections eagerly (small packs) or hashes each section on first use (large packs).
- Sections over 1 MiB carry a per-64 KiB-block hash list (our own, simple) so random access can verify lazily.
- Bao compatibility is an option to evaluate, not a v1 dependency.

### 3.11 Comparable formats: what to adopt

- **PMTiles v3:**
  - A 127-byte fixed header with magic and version, and the root directory guaranteed within the first 16 KiB, so one range request opens the file.
  - Directories are columnar (IDs, run lengths, lengths, offsets), delta + varint, then compressed.
  - Hilbert tile IDs; run-length deduplication; relocatable sections.
  - **Adopt:** header + section table + manifest + signature inside the first 16 KiB; columnar varint directories; Hilbert ordering; content deduplication by offset reuse (shared DEM ocean tiles, identical strings).
- **FlatGeobuf:**
  - The packed Hilbert R-tree comes before the features, which allows streaming with an HTTP-range bbox query.
  - Feature offsets come from the index.
  - **Adopt:** the index layout. **Avoid:** f64 coordinates and no compression.
- **GeoParquet:**
  - Columnar row groups with min/max statistics, a bbox "covering" column, CRS in metadata.
  - **Adopt:** per-block zone maps (min/max per bit-packed block, for pruning) and a schema description in the manifest.
  - **Avoid:** Parquet's page decoding cost for point lookups.
- **Lance (v2):**
  - Designed for random access: no row groups, per-column pages, "mini-block" vs "full-zip" structural encodings chosen by value size; uses FSST and bit-packing.
  - **Adopt:** small independently decodable blocks (a few KiB) sized for point access, and encodings chosen per column.
- **Vortex** (`vortex-array` 0.93.0, 2026-11-06):
  - Cascading compression (nested encodings chosen by a sampling compressor), FastLanes, FSST, ALP for floats, zero-copy/zero-parse metadata.
  - The file format is declared stable from 0.36.0 for backward reads.
  - **Adopt:** a per-column *encoding descriptor* (codec ID + parameters + child encodings) and **sampling-based codec selection in the builder** (try candidates on a sample, keep the smallest within a decode-time budget). This makes the A/B results a built-in feature.
- **SQLite:**
  - A rock-solid single-file format with a documented, long-stable spec, `application_id` and version fields, and extreme test discipline.
  - **Adopt:** a written spec as the contract, a strict compatibility policy (min reader version), and corpus-based compatibility tests over every released format version.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Own FOR + bit-pack (FastLanes layout) | no_std, in place, auto-vectorises incl. WASM | Our code to fuzz | ours | n/a | **Build** |
| `bitpacking` | Proven (tantivy), SIMD | std, x86-centred, own block API | MIT | 0.9.3 (2026-01) | Baseline |
| `fastlanes` | Reference FastLanes | no_std unverified, 0.x | Apache-2.0 | 0.7.2 (2026-09) | Baseline |
| `stream-vbyte` | Fast byte varints | Stale, non-standard licence | other | 0.4.1 (2023) | Reject |
| Own FSST decoder | Tiny, safe, random access | Encoder work in builder | ours | n/a | **Build** (decoder) |
| `fsst-rs` | Paper-faithful | Not production ready, unsafe, LE only | Apache-2.0 | 0.7.0 (2026-10) | Builder/oracle |
| `zstd` (C) | Best ratio, dictionaries | C dependency, WASM pain | BSD-3 | 0.14.3 (2026-10) | Builder only |
| `ruzstd` | Pure Rust, no_std decoder | Slower than C (**unverified** by how much) | MIT | 0.9.1 (2026-10) | **Use** (cold sections, if A/B wins) |
| `lz4_flex` | no_std, safe-decode, very fast | Lower ratio | MIT | 0.14.0 (2026-07) | Candidate (warm blocks) |
| Own rank/select + EF | In place, sized to us | Code to write | ours | n/a | **Build** |
| `sucds` / `vers-vecs` / `sux` | Fast, complete | Own memory or serialisation | MIT/Apache | 0.11.0 / 1.10.2 / 0.15.4 | Oracles |
| PtrHash-style own reader | ~3 bits/key, ns queries, deterministic | Code to write | ours | n/a | **Build** if it beats sorted search |
| `ptr_hash` / `ph` | State of the art | std, own serialisation | MIT / MIT-Apache | 2.1.3 / 0.11.0 | Reference |
| `boomphf` | Simple | Slower, stale | MIT | 0.6.0 (2023) | Reject |
| `fst` | Mature, in place, automata | std-only, last release 2021 | Unlicense/MIT | 0.4.7 | **Use** |
| `levenshtein_automata` | Fast parametric DFAs | Targets tantivy-fst (**unverified** fit) | MIT | 0.2.1 (2021) | **Use** or vendor |
| Top-k via ordinal range + RMQ | Proven (Hsu & Ottaviano) | Own code | ours | n/a | **Build** |
| Own flatbush-layout reader (i32) | Half the size of f64, in place, kNN | Own code | ours | n/a | **Build** |
| `geo-index` | Zero-copy flatbush ABI, safe | no_std unverified, f64 | MIT/Apache | 0.4.0 (2026-09) | Oracle / fallback |
| `static_aabb2d_index` | kNN visitor, safe by default | Owns memory | MIT/Apache | 2.1.0 (2026-08) | Oracle |
| `rstar`, `kiddo` | Mature | Dynamic, own memory | MIT/Apache | 0.14.0 / 6.3.0 | Builder/oracles |
| S2-style quadtree region index | Exact, O(1)-ish, compact | Builder complexity | ours | n/a | **Build** |
| `tzf-rs` | ~160 ns measured | Lite data inexact (≤112 m) | MIT (data ODbL) | 2.3.0 (2026-11) | Benchmark rival |
| `h3o` | Pure Rust, no_std | Not exact for regions | BSD-3 | 0.11.1 (2026-11) | Optional component |
| `robust`, `geo`, `i_overlay` | Correct predicates and clipping | f64, heavy for runtime | MIT/Apache | 1.2.0 / 0.33.1 / 9.1.0 | Builder |
| `polylabel` | Standard algorithm | | BlueOak-1.0.0 | 3.3.1 (2026-11) | Builder |
| Own near-lossless DEM codec | Guaranteed ±δ, best predictor | Own code | ours | n/a | **Build** |
| `lerc` | Proven bounded error | C++ binding | Apache-2.0 | 0.2.1 (2025-06) | Builder baseline |
| `zerocopy` | Sound, LE unaligned types, no_std | Compile time | BSD-2/Apache/MIT | 0.8.63 (2026-10) | **Use** |
| `bytemuck` | Simple | No endian types, alignment | Zlib/Apache/MIT | 1.25.2 | Reject for format |
| `rkyv` | Fast | Format not ours, O(n) validation | MIT | 0.8.18 | Reject for format |
| `memmap2` | Standard | Native only, soundness caveat | MIT/Apache | 0.9.12 | **Use** (native) |
| `blake3` | Fast, no_std, tree | | CC0/Apache | 1.8.7 | **Use** |
| `bao-tree` | Range verification | Heavier, no_std unverified | MIT/Apache | 0.17.0 | Evaluate later |
| `ed25519-dalek` | Standard, no_std | | BSD-3 | 3.0.0 | **Use** |
| `minisign-verify` | Zero dependencies | Detached files only | MIT | 0.3.0 | **Use** for catalog files (optional) |
| `tough` | Full TUF | Heavy, std | MIT/Apache | 0.24.0 | Server-side later |
| `cargo-fuzz` + `arbitrary`, `bolero` | Standard fuzzing | Nightly for libFuzzer | MIT/Apache | 0.13.2 / 1.5.0 / 0.13.7 | **Use** |

## 5. What we build ourselves, and why

Each item is a reader-side `no_std` module over `&[u8]`, fuzzed, with a crate oracle in tests. Each must prove the measurement next to it.

| # | What | Why own | Must prove |
|---|---|---|---|
| 1 | **Block codec**: FOR + bit-pack, 128 or 1024 values per block, optional delta and zigzag, per-block zone map | In-place access; no crate is no_std + in place | Decode ≥ 2 G ints/s native and ≥ 0.5 G/s in WASM SIMD (**target**); random access ≤ 5 ns; within 5% of `fastlanes` / `bitpacking` size |
| 2 | **FSST decoder + string pool**: symbol table + offsets (EF or bit-packed) + code bytes | Crates not production-ready or unsafe | Name fetch ≤ 50 ns median; size vs zstd-dict blocks and raw |
| 3 | **Rank/select + Elias–Fano** | In place; layout control | rank ≤ 5 ns, EF access ≤ 15 ns; overhead ≤ 6% for rank |
| 4 | **MPHF reader** (PtrHash-style, fixed seed) | Determinism and our layout | ≤ 3.2 bits/key; lookup ≤ 20 ns; byte-identical rebuilds; must beat sorted-ID Eytzinger search to be adopted |
| 5 | **Top-k completion**: FST ordinals + scores + block RMQ + short-prefix cache | Not available as a crate | Top-10 for any prefix ≤ 50 µs on the world names pack; size overhead ≤ 1.5 bits/term + scores |
| 6 | **Packed Hilbert R-tree reader** (flatbush layout, i32 boxes) + best-first kNN | Halves index size; in place | kNN k = 10 ≤ 5 µs on 10 M points; bbox query parity with `geo-index` |
| 7 | **Region index** (Morton quadtree, full/border leaves, clipped edges, exact integer crossings) | Exactness + speed; no crate does this in place | p50 ≤ 150 ns, p99 ≤ 1 µs; **0 mismatches** vs reference PIP on ≥ 10⁸ random points + all vertices and edge midpoints ± 1 grid unit; size ≤ tzf full (14 MB) for timezones |
| 8 | **Arc store** with per-vertex LOD thresholds | Topology-shared, one geometry for all LODs | Size vs per-LOD copies; LOD extraction ≤ 1 ms per country polygon; zero topology errors after simplification |
| 9 | **Near-lossless DEM codec** | Guaranteed ±1 m; WASM-clean | Max error ≤ 1 m (exhaustive); bits/pixel vs LERC and zstd + predictor; point query ≤ 20 µs cold tile, ≤ 100 ns warm |
| 10 | **Pack integrity**: section hashes, block hash lists, Ed25519 over the manifest | Simple, small, no_std | Open + verify cost per MB; rejects every mutated-byte fuzz case |
| 11 | **Minimal TUF-style catalog** | `tough` too heavy for mobile/WASM | Rollback and freeze tests; key rotation test |

### Recommended `.tgd` v1 section layout

All integers are little-endian. Every section starts at a 64-byte boundary. All offsets are u64 from file start. Sections are independently checksummed and are optional unless marked *required*. Rows are in Hilbert order of their representative point.

```
Offset 0 ──────────────────────────────────────────────────────────────
 HEADER (128 B, required)
   magic[8] = 0x89 'T' 'G' 'D' '\r' '\n' 0x1A '\n'   (PNG-style corruption detection)
   format_major u16, format_minor u16, min_reader_major u16, flags u16
   pack_uuid[16], build_seed u64, created_unix u64
   section_table_off u64, section_count u32, manifest_off u64, manifest_len u32
   signature_off u64
   coord_scale (1e-7 deg), quantisation origin
   header_blake3_trunc[16]
 SECTION TABLE (required): N × 80 B
   { kind u32, encoding_id u16, flags u16, offset u64, length u64,
     decoded_length u64, item_count u64, blake3[32], reserved[8] }
 MANIFEST (required): TLV or a small CBOR subset (own minimal reader)
   schema + components, coverage bbox and region IDs, dependencies (pack IDs + hashes),
   provenance and licences (ADR-0023), encoding descriptors per column (Vortex-style)
 SIGNATURE BLOCK (required for distributed packs): key_id, Ed25519 over
   BLAKE3(header ‖ section table ‖ manifest)
 ── everything above fits in the first 16 KiB (PMTiles rule) ──
 1  IDMAP        stable u64 IDs column (FOR bit-packed) + MPHF or sorted permutation
 2  PLACES       core columns: lat/lon i32 (delta-in-Hilbert-order, bit-packed blocks + zone maps),
                 kind/class dictionary codes, elevation i16, population (log-bucket), flags
 3  HIERARCHY    parent row (bit-packed) + children lists (Elias–Fano)
 4  NAMES        per-place name ranges (EF offsets), name records (lang, script, kind),
                 FSST symbol tables (per script group) + code bytes + EF offsets
 5  SEARCH       FST (normalised key → ordinal), postings (EF), scores (bit-packed),
                 block RMQ, short-prefix top-k cache
 6  PLACE_INDEX  packed Hilbert R-tree over place points (i32 boxes) [or KDBush]
 7  ARCS         arc offsets (EF), delta+zigzag bit-packed coordinates, per-vertex LOD level (2–3 bits)
 8  POLYGONS     ring/part tables (signed arc refs), per-polygon bbox, label point, area class
 9  POLY_INDEX   packed Hilbert R-tree over polygon bboxes
 10 REGION       region-index directory grid, quadtree node stream (Morton), leaf table,
                 clipped-edge pool (cell-local coordinates), region-ID → (kind, polygon/tz/admin ID)
 11 POSTAL       FST over postal codes → postal records (centroid, region, place ref)
 12 COMPONENTS   per component: presence bitvector + rank directory + dense value columns
                 (each column with its own encoding descriptor)
 13 DEM          tile directory (z/x/y → offset, length; dedup by offset reuse) + coded tiles
 14 BLOBS        cold data (attribution text, long strings) in zstd-dict blocks (ruzstd decode)
 15 BLOCK_HASHES per-64 KiB BLAKE3 lists for large sections (lazy verification)
 FOOTER (32 B): magic repeat + file length + header hash (truncation detection)
```

Feature mapping ([ADR-0015](../../03-decisions/adr/ADR-0015-tree-shaking.md)):

| Feature | Sections or modules |
|---|---|
| `core` | 1–3 |
| `names` | 4 |
| `search` | 5 (pulls in `fst`) |
| `spatial` | 6 |
| `geometry` | 7–9 |
| `region` | 10 |
| `postal` | 11 |
| `components` | 12 |
| `dem` | 13 |
| `zstd` | 14 (pulls in `ruzstd`) |
| `verify` | BLAKE3 / Ed25519 |

A reader built without a feature skips unknown or unwanted sections by the table, so the format stays forward-compatible.

## 6. Impact on the plan

- **ADR-0009 (pack format):** adopt the layout above as the v1 draft. Codecs stay "candidate" until the A/B gate passes. Add: 64-byte alignment, LE unaligned types via `zerocopy`, first-16-KiB rule, footer, per-column encoding descriptors, builder-side sampling codec selection.
- **ADR-0013 (region index):**
  - Refine to an S2ShapeIndex-style "center label + clipped edges crossing count", with exact integer predicates on cell-local coordinates and a defined on-boundary tie-break.
  - Record that "exact" means exact against the **quantised** source (1e-7°).
  - tzf-rs full data is the speed rival, and timezonefinder the correctness reference.
- **ADR-0007 (accuracy model):** add the quantisation grid (1e-7° ≈ 1.1 cm), the DEM bound (±1 m guaranteed), and per-LOD simplification tolerances.
- **ADR-0014 (names and search):** FST with ordinal outputs + RMQ top-k; FSST name storage with per-script tables; Levenshtein via Schulz–Mihov automata.
- **ADR-0015 (tree-shaking):** the feature map above. `fst` and `ruzstd` are the only non-trivial runtime dependencies.
- **ADR-0021 (Rust tooling):** add `cargo-fuzz` / `bolero` targets, Miri on the reader, `#![forbid(unsafe_code)]` in the reader crate, and a wasm size budget check.
- **ADR-0012 (offline loading) / ADR-0025 (hosted-ready):** minimal TUF-style catalog; per-block hashes for lazy range reads.
- **New architecture page:** `docs/04-architecture/pack-format.md`, the byte-level spec, written once the benchmarks below settle the codecs.
- **Roadmap gate:** "Format freeze v1" cannot pass until every benchmark in §7 has a recorded result in `docs/08-progress/`.

## 7. Risks and open items

**Risks**

- **`fst` is std-only.** If a strict `no_std` core is required for some embedded target, we must fork its read path. Low risk for native, mobile and WASM, which all have std.
- **mmap soundness:** external modification of a mapped pack is undefined behaviour. Mitigate with immutable installs (temp + rename) and an in-memory mode.
- **Region index size at full resolution** (coastlines with millions of vertices). Mitigate by sharing clipped edges with the arc store through references instead of copies (A/B), and by excluding coastline from timezone polygons where the source allows (**unverified**).
- **Determinism:** FSST training, MPHF construction and simplification must be single-threaded or deterministically partitioned, with recorded seeds. CI must rebuild a pack twice and compare the bytes.
- **Licences:** `polylabel` is BlueOak-1.0.0 (permissive; check it against [ADR-0002](../../03-decisions/adr/ADR-0002-licences.md)). `tzf-rel` data is ODbL and must never be embedded in our MIT/Apache packs without honouring share-alike. Use it for benchmarks only.

**A/B benchmarks to run before final choices**

Run each on native x86-64 and aarch64, on Android/iOS where it applies, and in WASM (Chrome, Safari). Measure size, decode/query latency (p50/p99) and WASM binary size delta.

1. **Integer blocks:** own FOR bit-pack at 128 vs 1024 values vs `bitpacking` vs `fastlanes` vs varint vs StreamVByte, on lat/lon deltas, parent rows, IDs and postings.
2. **Row order:** Hilbert vs Morton vs input order. Compressed size of coordinate columns and R-tree query page touches.
3. **Names:** FSST (one table vs per-script tables) vs zstd with trained dictionary in 4/16/64 KiB blocks vs lz4_flex blocks vs raw UTF-8 + front coding. Size and single-name fetch latency, on Latin and Devanagari sets separately.
4. **zstd decoder:** `ruzstd` vs C `zstd` decode throughput, and WASM size added by each.
5. **ID → row:** PtrHash-style MPHF + ID check vs sorted permutation with binary, Eytzinger and interpolation search, at n = 10⁵, 10⁶, 10⁷. Bytes per key and ns per lookup, hit and miss.
6. **Rank/select:** own rank9 vs poppy layout vs `vers-vecs` vs `sucds`, on presence bitmaps at 1%, 10% and 90% density.
7. **Top-k completion:** ordinal range + block RMQ vs succinct RMQ vs a precomputed top-k per FST state (size explosion check). Plus Levenshtein d = 1/2 latency with `levenshtein_automata` vs fst's built-in.
8. **Place index:** packed Hilbert R-tree (f64 flatbush ABI vs i32 vs per-node relative u16 boxes) vs KDBush. kNN k = 1/10/100 and bbox queries; size.
9. **Region index:**
   - Quadtree max edges per leaf (4/8/16/32) and top directory size vs latency and size.
   - Clipped-edge copies vs references into the arc store.
   - Against `tzf-rs` (lite and full) for timezones.
   - Correctness: 0 mismatches vs a reference PIP (`geo` + `robust`) on ≥ 10⁸ points incl. boundary-adjacent points.
10. **Polygons:** per-vertex LOD thresholds vs separate per-LOD arc sets. Bit-packed deltas vs varint + zstd. Shared-arc topology vs per-polygon rings (measures the TopoJSON gain on our admin data).
11. **DEM at ±1 m:** own MED + rANS/Huffman vs MED + zstd vs LERC (MaxZError 1 m) vs zstd + predictor 2 vs Terrarium PNG. Bytes/km² on SRTM 1″ and 3″ tiles over flat (Gangetic plain), mountain (Himalaya) and coastal areas, plus point-query latency.
12. **Integrity:** eager vs lazy (on-first-use) section hashing, and 64 KiB vs 16 KiB block hash granularity vs a `bao-tree` outboard. Open-time cost for 10 MB, 100 MB and 1 GB packs on a mid-range Android phone.
13. **Zero-copy reader:** validation-on-open cost (µs per MB) and hot-query overhead of checked access vs `unsafe` unchecked (decides whether `forbid(unsafe_code)` holds).
14. **Codec selection:** builder sampling selector vs fixed codecs. Total pack size and build time on the world pack.

## 8. Versions checked

All crate data from the crates.io API (`https://crates.io/api/v1/crates/<name>`). Dates are the release dates of the listed version.

| Thing | Version | Checked on | Where |
|---|---|---|---|
| bitpacking | 0.9.3 (2026-01-08) | 2026-11-06 | crates.io API |
| fastlanes | 0.7.2 (2026-09-02) | 2026-11-06 | crates.io API |
| stream-vbyte | 0.4.1 (2023-05-23) | 2026-11-06 | crates.io API |
| integer-encoding | 4.1.0 (2025-10-28) | 2026-11-06 | crates.io API |
| leb128 | 0.2.7 (2026-06-15) | 2026-11-06 | crates.io API |
| unsigned-varint | 0.8.0 (2023-11-01) | 2026-11-06 | crates.io API |
| fast_hilbert | 2.1.0 (2026-02-12) | 2026-11-06 | crates.io API |
| hilbert_2d | 1.1.0 (2023-01-06) | 2026-11-06 | crates.io API |
| morton-encoding | 2.0.1 (2021-03-26) | 2026-11-06 | crates.io API |
| fsst-rs | 0.7.0 (2026-10-27) | 2026-11-06 | crates.io API, GitHub README |
| fsst (Lance) | 14.0.2 (2026-10-29) | 2026-11-06 | crates.io API |
| zstd / zstd-sys | 0.14.3 / 2.1.2+zstd.1.5.7 (2026-10-16) | 2026-11-06 | crates.io API |
| ruzstd | 0.9.1 (2026-10-11) | 2026-11-06 | crates.io API, `lib.rs` (`#![no_std]`) |
| lz4_flex | 0.14.0 (2026-07-14) | 2026-11-06 | crates.io API, `lib.rs` |
| lz4 | 1.28.1 (2025-01-07) | 2026-11-06 | crates.io API |
| sucds | 0.11.0 (2026-10-25) | 2026-11-06 | crates.io API, `lib.rs` (no_std) |
| vers-vecs | 1.10.2 (2026-08-10) | 2026-11-06 | crates.io API |
| succinct | 0.5.2 (2019-08-29) | 2026-11-06 | crates.io API |
| sux / epserde | 0.15.4 (2026-11-03) / 0.13.2 (2026-10-12) | 2026-11-06 | crates.io API |
| bitm | 0.6.0 (2026-10-29) | 2026-11-06 | crates.io API |
| ptr_hash | 2.1.3 (2026-10-21) | 2026-11-06 | crates.io API, GitHub README |
| ph | 0.11.0 (2026-02-12) | 2026-11-06 | crates.io API |
| boomphf | 0.6.0 (2023-07-26) | 2026-11-06 | crates.io API |
| phf / quickphf | 0.14.0 (2026-06-21) / 0.1.0 (2023-10-16) | 2026-11-06 | crates.io API |
| fst | 0.4.7 (2021-06-06) | 2026-11-06 | crates.io API |
| levenshtein_automata | 0.2.1 (2021-05-18) | 2026-11-06 | crates.io API |
| tantivy-fst | 0.5.0 (2023-11-21) | 2026-11-06 | crates.io API |
| static_aabb2d_index | 2.1.0 (2026-08-08) | 2026-11-06 | crates.io API, docs.rs (`visit_neighbors`) |
| geo-index | 0.4.0 (2026-09-15) | 2026-11-06 | crates.io API, GitHub README |
| flatgeobuf | 6.0.1 (2025-12-28) | 2026-11-06 | crates.io API |
| rstar | 0.14.0 (2026-11-01) | 2026-11-06 | crates.io API, `lib.rs` (no_std, forbid unsafe) |
| kiddo | 6.3.0 (2026-09-06) | 2026-11-06 | crates.io API |
| h3o | 0.11.1 (2026-11-02) | 2026-11-06 | crates.io API, `lib.rs` (no_std) |
| s2 | 0.2.0 (2026-08-19) | 2026-11-06 | crates.io API |
| tzf-rs / tzf-rel | 2.3.0 (2026-11-02) / 0.0.2026-a (2026-03-08) | 2026-11-06 | crates.io API, GitHub README |
| geo / geo-types | 0.33.1 (2026-04-20) / 0.7.20 (2026-08-02) | 2026-11-06 | crates.io API |
| robust | 1.2.0 (2025-05-10) | 2026-11-06 | crates.io API |
| i_overlay | 9.1.0 (2026-10-14) | 2026-11-06 | crates.io API |
| polylabel | 3.3.1 (2026-11-06) | 2026-11-06 | crates.io API |
| topojson | 0.5.1 (2022-06-25) | 2026-11-06 | crates.io API |
| geozero | 0.15.1 (2025-12-11) | 2026-11-06 | crates.io API |
| lerc | 0.2.1 (2025-06-20) | 2026-11-06 | crates.io API (depends on lerc-sys) |
| zerocopy | 0.8.63 (2026-10-13) | 2026-11-06 | crates.io API |
| bytemuck | 1.25.2 (2026-07-19) | 2026-11-06 | crates.io API |
| rkyv | 0.8.18 (2026-08-05) | 2026-11-06 | crates.io API |
| memmap2 | 0.9.12 (2026-10-21) | 2026-11-06 | crates.io API |
| arbitrary | 1.5.0 (2026-10-06) | 2026-11-06 | crates.io API |
| cargo-fuzz / libfuzzer-sys | 0.13.2 (2026-06-09) / 0.4.13 (2026-06-04) | 2026-11-06 | crates.io API |
| bolero | 0.13.7 (2026-10-03) | 2026-11-06 | crates.io API |
| blake3 | 1.8.7 (2026-08-20) | 2026-11-06 | crates.io API |
| bao-tree / bao / abao | 0.17.0 (2026-10-22) / 0.13.1 (2025-04-07) / 0.2.0 (2023-02-27) | 2026-11-06 | crates.io API |
| ed25519-dalek | 3.0.0 (2026-07-06) | 2026-11-06 | crates.io API |
| minisign / minisign-verify | 0.10.0 / 0.3.0 (2026-09-25) | 2026-11-06 | crates.io API |
| tough | 0.24.0 (2026-07-10) | 2026-11-06 | crates.io API |
| pmtiles | 0.28.0 (2026-11-06) | 2026-11-06 | crates.io API |
| geoarrow | 0.10.0 (2026-11-06) | 2026-11-06 | crates.io API |
| lance | 14.0.2 (2026-10-29) | 2026-11-06 | crates.io API |
| vortex-array | 0.93.0 (2026-11-06) | 2026-11-06 | crates.io API |
| rusqlite | 0.41.1 (2026-10-28) | 2026-11-06 | crates.io API |
| pco | 1.1.0 (2026-11-03) | 2026-11-06 | crates.io API (float/int column codec; consider in A/B 1) |
| PMTiles spec | v3 | 2026-11-06 | GitHub protomaps/PMTiles |

## 9. Sources

- [Decoding billions of integers per second through vectorization](https://arxiv.org/abs/1209.2137), Lemire & Boytsov, Software: Practice and Experience, 2015.
- [Stream VByte: Faster byte-oriented integer compression](https://arxiv.org/abs/1709.08990), Lemire, Kurz, Rupp, Information Processing Letters, 2018.
- [The FastLanes compression layout](https://www.vldb.org/pvldb/vol16/p2132-afroozeh.pdf), Afroozeh & Boncz, PVLDB 16, 2023.
- Analysis of the clustering properties of the Hilbert space-filling curve, Moon et al., IEEE TKDE, 2001.
- [FSST: Fast Random Access String Compression](https://www.vldb.org/pvldb/vol13/p2649-boncz.pdf), Boncz, Neumann, Leis, PVLDB 13, 2020.
- [spiraldb/fsst README](https://github.com/spiraldb/fsst), accessed 2026-11-06.
- [Quasi-succinct indices](https://arxiv.org/abs/1206.4300), Vigna, WSDM, 2013.
- Partitioned Elias–Fano indexes, Ottaviano & Venturini, SIGIR, 2014.
- Broadword implementation of rank/select queries, Vigna, WEA, 2008.
- Space-efficient, high-performance rank and select structures on uncompressed bit sequences, Zhou, Andersen, Kaminsky, SEA, 2013.
- [PtrHash: Minimal Perfect Hashing at RAM Throughput](https://doi.org/10.4230/LIPIcs.SEA.2025.21), Groot Koerkamp, SEA, 2025. Numbers from the [ptrhash README](https://github.com/RagnarGrootKoerkamp/ptrhash), accessed 2026-11-06.
- [PHast: Perfect Hashing made fast](https://arxiv.org/abs/2504.17918), Beling & Sanders, arXiv 2025 (v6 2025-10-22; ALENEX 2026).
- Fingerprinting-based minimal perfect hashing revisited, Beling, ACM JEA, 2023.
- PTHash: Revisiting FCH minimal perfect hashing, Pibiri & Trani, SIGIR, 2021.
- Fast and scalable minimal perfect hashing for massive key sets (BBHash), Limasset et al., SEA, 2017.
- Hash, displace, and compress (CHD), Belazzougui, Botelho, Dietzfelbinger, ESA, 2009.
- [fst crate](https://github.com/BurntSushi/fst) and [Index 1,600,000,000 keys with automata and Rust](https://blog.burntsushi.net/transducers/), Gallant, 2015.
- Fast string correction with Levenshtein automata, Schulz & Mihov, IJDAR, 2002.
- Incremental construction of minimal acyclic finite-state automata, Daciuk et al., Computational Linguistics, 2000.
- Space-efficient data structures for top-k completion, Hsu & Ottaviano, WWW, 2013.
- Space-efficient preprocessing schemes for range minimum queries, Fischer & Heun, SIAM J. Computing, 2011.
- Hilbert R-tree: an improved R-tree using fractals, Kamel & Faloutsos, VLDB, 1994.
- Distance browsing in spatial databases, Hjaltason & Samet, ACM TODS, 1999.
- [flatbush](https://github.com/mourner/flatbush), Agafonkin; [static_aabb2d_index](https://github.com/jbuckmccready/static_aabb2d_index); [geo-index](https://github.com/kylebarron/geo-index), accessed 2026-11-06.
- [S2 geometry library: S2ShapeIndex](https://s2geometry.io/devguide/s2shapeindex), Google, accessed 2026-11-06.
- [tzf-rs README](https://github.com/ringsaturn/tzf-rs), accessed 2026-11-06 (timings on Apple M3 Max, lite dataset).
- [timezonefinder README](https://github.com/jannikmi/timezonefinder), accessed 2026-11-06.
- Adaptive precision floating-point arithmetic and fast robust geometric predicates, Shewchuk, Discrete & Computational Geometry, 1997.
- [TopoJSON specification](https://github.com/topojson/topojson-specification), Bostock.
- Line generalisation by repeated elimination of points, Visvalingam & Whyatt, Cartographic Journal, 1993.
- Topologically consistent line simplification with the Douglas–Peucker algorithm, Saalfeld, CaGIS, 1999.
- [mapshaper simplification](https://github.com/mbloch/mapshaper/wiki/Simplification-Tips), Bloch.
- Stable snap rounding, Hershberger, Computational Geometry, 2013; Practical segment intersection with finite precision output, Hobby, CGTA, 1999.
- [polylabel](https://github.com/mapbox/polylabel), Agafonkin, Mapbox, 2016.
- [TWKB specification](https://github.com/TWKB/Specification); [Geobuf](https://github.com/mapbox/geobuf); [FlatGeobuf](https://flatgeobuf.org/); [Mapbox Vector Tile spec](https://github.com/mapbox/vector-tile-spec).
- [LERC](https://github.com/Esri/lerc), Esri, accessed 2026-11-06.
- [TIFF compression research for elevation data](https://github.com/linz/elevation/blob/master/docs/research/tiff-compression.md), Toitū Te Whenua LINZ, accessed 2026-11-06.
- The LOCO-I lossless image compression algorithm (JPEG-LS), Weinberger, Seroussi, Sapiro, IEEE TIP, 2000.
- [quantized-mesh-1.0 terrain format](https://github.com/CesiumGS/quantized-mesh), Cesium; [Terrarium / terrain-RGB](https://docs.mapbox.com/data/tilesets/reference/mapbox-terrain-rgb-v1/), Mapbox.
- [Cloud Optimized GeoTIFF](https://cogeo.org/).
- [zerocopy](https://github.com/google/zerocopy), Google; [bytemuck](https://github.com/Lokathor/bytemuck); [rkyv](https://github.com/rkyv/rkyv); [memmap2](https://github.com/RazrFalcon/memmap2-rs).
- [Rust Fuzz Book (cargo-fuzz)](https://rust-fuzz.github.io/book/); [bolero](https://github.com/camshaft/bolero).
- [BLAKE3 specification](https://github.com/BLAKE3-team/BLAKE3-specs), O'Connor et al., 2020; [Bao](https://github.com/oconnor663/bao); [bao-tree](https://github.com/n0-computer/bao-tree).
- [ed25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek); [minisign](https://jedisct1.github.io/minisign/).
- Survivable key compromise in software update systems (TUF), Samuel, Mathewson, Cappos, Dingledine, CCS, 2010; [TUF specification](https://theupdateframework.github.io/specification/latest/); [tough](https://github.com/awslabs/tough).
- [PMTiles v3 specification](https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md), Protomaps, accessed 2026-11-06.
- [GeoParquet specification](https://geoparquet.org/); [Lance](https://github.com/lance-format/lance); [Vortex](https://github.com/vortex-data/vortex), accessed 2026-11-06.
- [SQLite database file format](https://www.sqlite.org/fileformat2.html).
