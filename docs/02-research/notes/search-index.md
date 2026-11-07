---
title: "The SEARCH section: key dictionary, fuzzy automaton and postings (B7)"
status: research
updated: 2026-11-07
owner: maintainer's agents
revisit: 2027-05-07
---

# The SEARCH section: key dictionary, fuzzy automaton and postings (B7)

## 1. Question

Phase 2 step B7 ([phase 2](../../07-planning/phases/phase-2.md)) moves the search design of
spike S3 ([results](../benches/s3-search.md), D-052, [ADR-0032](../../03-decisions/adr/ADR-0032-text-search.md))
into a SEARCH pack section. The design stays: three key → postings maps (exact, loose,
phonetic), an anchored one-edit Damerau (OSA) automaton walked over the exact map with an
unanchored fallback, prefix enumeration, and one additive score. What changes is the storage
and the automaton, under the reader crate's rules: `no_std` (alloc only in the writer),
`#![forbid(unsafe_code)]`, zero-copy views over the pack bytes, a validating open and a deep
`check()` that never panics on hostile bytes, a deterministic writer, and an MIT/Apache-compatible
licence. It must work at world scale (10^7–10^8 keys, every script).

This note answers four questions and feeds B7's layout, its A/B and the pack-format page:

1. How to store the key dictionary: the `fst` crate, our own FST, front-coded blocks, or a
   succinct trie.
2. Which fuzzy automaton has the lowest cost per trie node for one edit with transpositions.
3. How to store the postings (record lists), and how to stop early on top-k.
4. How other geocoders store and fuzzy-search names, with their sizes.

It builds on [format, compression and indexing](format-compression-indexing.md) §3.5 (the first
look at `fst`, `levenshtein_automata` and top-k completion) and
[indexes](indexes.md#33-search) §3.3, and does not repeat them.

## 2. Summary and recommendation

1. **Key dictionary: our own minimal acyclic automaton (an FST in our format), one for all
   three maps.** The `fst` crate cannot be the reader: it is `std`-only, unmaintained (last
   release 2021-06-06), and not safe on hostile bytes. Its own source says that if the open
   check passes on a bad file "the program will probably panic", it slices without bounds
   checks, and its CRC is no defence against a crafted file. Our format keeps fst's idea
   (Daciuk minimisation, ordinal outputs) with rules a checker can prove: every transition points
   to a lower address (so a walk always ends), labels strictly increase, every read is
   bounds-checked, and `check()` enumerates every key once with a fixed stack and confirms each
   ordinal. The three maps become one automaton whose keys start with a map tag byte, so suffixes
   are shared and there is one postings column. `fst` 0.4.7 stays as a dev-dependency oracle.
2. **Fuzzy: a bit-parallel NFA (Wu–Manber bitap with Hyyrö's transposition term) per trie node,
   over code points, not bytes.** For one edit the state is two `u64` words plus the previous
   symbol's mask. A transition is about a dozen word operations, against S3's dynamic-programming
   row of O(query length) cells. Two edits add one word. No per-query construction is needed:
   `levenshtein_automata` spends about 36 µs building the DFA for an 11-letter word at one edit
   with transpositions (its README), more than S3's whole p50 of 26 µs. With per-node cost cut
   about 5–10× (**estimate**), the unanchored walk (1,342–1,735 nodes p50 on Nepal, measured
   here) fits the budget, so the first-letter loss of anchoring (probe recall@5 0.900) can go.
   The Schulz–Mihov parametric tables, driven per node without building a DFA, are the A/B.
3. **Postings: one B1 column of record IDs, frame-of-reference (FOR) coded, each list in
   importance order, plus a B1 column of list starts.** On Nepal 85% of keys have one record and
   lists are short with large gaps, so per-list Elias–Fano or delta coding buys little. FOR is
   indifferent to order, so lists can be sorted by importance at no cost, which is what the
   early stop needs. Estimate on Nepal: 290 KB of IDs plus 66 KB of starts, against 894 KB of
   S3's `u32` postings. Top-k prefix completion uses the key ordinal range plus a block maximum of
   importance per 128 keys.
4. **Other geocoders do not solve this offline and small.** Photon (OpenSearch, about 95 GB for
   the planet), Pelias (Elasticsearch), Nominatim (PostgreSQL, at least 1 TB for the planet, no
   typo tolerance documented) and Airmail (tantivy, 300 GB planet index **unverified**) are server
   designs. Lucene and Meilisearch confirm the method: Damerau (OSA) automata intersected with a
   term dictionary, and Meilisearch counts a first-letter typo as two, the same trade-off as our
   anchoring.

**The A/B for B7:** own automaton against front-coded blocks of 16 keys (size, and walk cost per
query on the golden set); bit-parallel against the spike's DP row and against parametric tables
(instructions per visited node, p50/p99, anchored and unanchored); one tagged automaton against
three; FOR postings against per-list Elias–Fano. Gates: the recall gates of D-052 held, p99 under
100 µs unanchored, and SEARCH no larger than S3's 3.6 MB (target about 1.5 MB on Nepal,
**estimate**).

## 3. Findings

### 3.1 Measurements made for this note (2026-11-07)

A throwaway Python script over GeoNames `NP.txt` (the P and A rows S3 loads, without COD-AB),
with an approximate fold (NFKD, Latin marks dropped, lower case, punctuation to one space). It
approximates S3's exact map only. B7 re-measures every number in Rust on the real fold.

| Quantity | Value |
|---|---:|
| Records | 129,799 |
| Exact keys | 91,447 |
| (key, record) pairs | 132,078 |
| Raw key bytes | 861,989 (9.4 B per key) |
| Keys with one record | 77,351 (84.6% of keys, 58.6% of pairs) |
| Longest list | 302 records |
| Trie nodes, bytes / code points | 266,373 / 262,149 |
| Minimal DAFSA over bytes | 50,367 states, 103,733 transitions |
| S3 postings layout (`4 + 4p` per list) | 894,100 B |

**Dictionary sizes** (keys only, no values):

| Layout | Bytes | B per key |
|---|---:|---:|
| Raw | 861,989 | 9.43 |
| Front coding, blocks of 8 | 481,210 | 5.26 |
| Front coding, blocks of 16 | 421,290 | 4.61 |
| Front coding, blocks of 32 | 391,414 | 4.28 |
| Front coding, blocks of 64 | 376,434 | 4.12 |
| FST (fst crate ratios, see §3.2) | about 250,000–350,000 | 2.7–3.8 (**estimate**) |

Front coding here is the tantivy sstable encoding: one byte (keep < 16, add < 16) or a long form,
then the suffix, a full key at each block head, and a `u32` block offset.

**Fuzzy walk cost:** trie nodes visited by a one-edit OSA walk, over the 512 golden queries:

| Walk | p50 nodes | p99 nodes | max |
|---|---:|---:|---:|
| bytes, anchored | 473 | 1,033 | 1,305 |
| bytes, unanchored | 1,342 | 2,246 | 2,515 |
| code points, anchored | 473 | 1,033 | 1,305 |
| code points, unanchored | 1,735 | 2,640 | 2,908 |
| code points, two edits, anchored | 1,992 | 4,652 | — |

S3's anchored p50 of 26 µs over about 473 nodes is about 55 ns per node, including the `fst`
node decode and collecting postings. That matches its unanchored p50 of 50 µs over about 1,342
nodes. The walk is node-bound, so per-node cost is the lever.

The code-point walk visits more nodes unanchored because a one-symbol edit on a Devanagari
letter (3 bytes in UTF-8) stays within distance 1, where over bytes it often costs 2 and is
pruned. That is a recall difference too: over bytes, S3 cannot correct most single Devanagari
substitutions, nor any transposition of two Devanagari letters.

**SymSpell size:** keys plus their one-deletion variants: 844,831 entries, 9.2× the 91,447 keys.
With SymSpell's prefix limit of 7: 385,348 entries, 4.2×.

**Postings sizes** (record IDs need 17 bits on Nepal):

| Layout | Bytes |
|---|---:|
| S3: `u32` length + `u32` IDs per list | 894,100 |
| B1 FOR over all IDs concatenated | 289,840 |
| B1 delta over list starts (n + 1 values) | 65,607 |
| Singletons inline (17 bits per key) + multi-record lists as Elias–Fano | 194,325 + 129,245 + their offsets |
| Multi-record lists as delta bit-packing per list | 148,696 |
| Multi-record lists as plain `u32` | 218,908 |

FOR over all IDs (290 KB) plus starts (66 KB) is 356 KB, 2.5× smaller than S3. The inline plus
Elias–Fano mix comes to about the same once offsets are counted, and it is more code.

### 3.2 The `fst` crate and its forks

- **`fst` 0.4.7** (BurntSushi, 2021-06-06, Unlicense/MIT, 33.2 M downloads). Last commit
  2024-09-25 (a FUNDING file). Read from a clone on 2026-11-07:
  - **`std` only:** no `no_std` attribute.
  - **No `unsafe` in `src/`** apart from an mmap in a doc example.
  - **Not safe on hostile bytes.** `Fst::new` checks the version, the type and that the root
    address is near the end. Its own comment: "If this check passes, it is still possible that the
    FST is invalid ... If this check reports a false positive, then the program will probably
    panic." Node decoding indexes the slice directly (`data[addr]`, `&node.data[i..]`). Transition
    targets are `node_addr - delta_addr` with no overflow check, so they panic in debug and wrap in
    release. `verify()` is a CRC-32C that anyone can recompute, so it catches damage, not attacks.
  - **Termination:** targets are backward deltas, so addresses fall along any path and a walk
    cannot loop. A hostile delta still panics or wraps (above).
  - **Size:** BurntSushi's own figures. 119,095 English words: 1.1 MB raw, FST 324 KB (29.4%,
    2.7 B per key). 15.8 M Wikipedia titles: 384 MB raw, FST 157 MB (40.9%). 1.65 B Common Crawl
    URLs: FST 20.1% of raw. Place names (9.4 B average) sit near the English-words case, so about
    30–40% of raw (§3.1 estimate). The ordinal outputs add some bytes on top.
  - **Levenshtein:** a DFA built per query, with a 10,000-state limit (`TooManyStates`, issue 83).
    S3 measured it at about 10× our automaton (293 µs p50).
- **`fst-no-std` 0.4.8** (2024-01-18, 3,811 downloads): a fork with
  `#![cfg_attr(not(feature = "std"), no_std)]`, but also `feature(error_in_core)` when not `std`,
  so the `no_std` build needed nightly at its last commit. Same decoding code, same hostile-byte
  behaviour. One release; not a maintained base.
- **`tantivy-fst` 0.5.0** (2023-11-21): tantivy's fork, `std`, the same decoder.

**Verdict:** none of these meets the reader rules. Forking means rewriting the decoder with
checked reads and a deep checker, which is most of the reader, plus a format we do not control.

### 3.3 Our own FST

The structure is a minimal acyclic DFA built incrementally from sorted keys (Daciuk, Mihov,
Watson and Watson, *Computational Linguistics* 26(1), 2000), with outputs as in `fst`: each
transition carries the number of keys in the smaller sibling subtrees, and their sum along a
path is the key's ordinal.

Rules that make it checkable:

- Nodes are written children first, so every target address is lower than its node's address.
  A walk therefore ends after at most `len` steps. `check()` verifies it per transition.
- Each node is a header, labels strictly increasing, then fixed-width target deltas and outputs
  (widths in the header). All reads go through checked `get`. A bad width or address is an
  `Error`, never a panic.
- `check()` enumerates every key once, depth-first, with a fixed stack of at most `MAX_KEY`
  levels (no allocation). It confirms that each transition's output equals the running ordinal,
  that the count equals the header's key count, and that no key is longer than `MAX_KEY`. Cost:
  one step per trie node (266 k on Nepal). For 10^8 keys that is about 5 × 10^8 steps, about a
  second (**estimate**). Fine for the deep check, which is optional.
- The writer is deterministic: sorted input and a registry keyed by node content. The `fst`
  crate is a dev-dependency oracle: the same keys must give the same ordinals and the same
  results for every automaton.

Fuzzy search on a trie walks child transitions and drops a child as soon as the automaton state
is dead, so a dead subtree costs nothing. This is the decisive advantage over front coding (§3.4).

### 3.4 Front-coded blocks (Lucene/tantivy term dictionaries)

- Tantivy's sstable dictionary (`tantivy-sstable` 0.27.0, 2026-10-16) stores blocks of keys as
  (keep, add, suffix) deltas, with an FST over block heads as the index. Its README says it was
  built for remote, block-wise reads (one fetch per lookup), not for the smallest size or
  fastest `get` ("the performance of a get is very bad"). Lucene's BlockTree is the same idea,
  with a prefix-trie index.
- **Size:** 4.6 B per key at block 16 on Nepal (§3.1), larger than the FST estimate. Re-Pair over
  front coding roughly halves front coding again (Martínez-Prieto et al., *Information Systems*
  56, 2016, and its follow-ups), but a grammar decoder per key costs walk time.
- **Fuzzy walk:** keep the automaton state per depth on a stack. For each next key, keep the
  first `lcp` states and step only the new suffix, so the work is the same trie nodes. A dead
  state at depth L must skip every following key with that prefix. Within a block that means
  decoding each skipped key's header; across blocks it is a binary search of block heads for the
  successor of the dead prefix. With about 1,300–1,700 nodes per unanchored query, many of them
  dead ends, the skips are the open cost. They cannot be estimated reliably without the A/B.
- **Strengths:** trivial to validate (keys strictly increasing, `lcp` ≤ previous length, every
  suffix inside its block), ordinals implicit in position, about 200 lines of code.

### 3.5 Succinct tries and double arrays

- **LOUDS / MARISA.** A LOUDS trie costs about 2 bits per node plus a label byte, so on Nepal's
  262 k code-point nodes about 330 KB (3.6 B per key) before terminal bits and values. MARISA
  (Yata) recursively compresses path labels and is smaller still. Every child step costs a
  rank/select, which is slower per node than an FST transition, and we would need our own
  rank/select (the format note already plans one). `rsmarisa` 0.4.2 (2026-06-01) is a pure-Rust
  port. Its `unsafe` use and `no_std` status were not checked (**unverified**).
- **`crawdad` 0.4.1** (2026-08-12, MIT/Apache): character-wise double-array tries. `#![no_std]`
  and no `unsafe` in `src/` (checked in a clone). But `deserialize_from_slice` copies into a
  `Vec<Node>` (not zero-copy), slices and `unwrap`s without checks (it panics on short input), has
  no ordered enumeration or automaton walk, and costs 8 B per node (base and check `u32`): about
  2 MB on Nepal. Rejected. Its code mapper (code points renumbered by frequency) is an idea worth
  keeping for the automaton's alphabet.
- **`yada` 0.7.0** (2026-06-06): a byte-wise double array with `unsafe` (`new_unchecked`).
  Rejected.

### 3.6 Fuzzy automata

- **Our DP row (S3):** the state is two rows and the last byte. A transition is O(m) cells
  (m ≤ 31 bytes), each with three minimums and a transposition test. About 55 ns per node in S3,
  including the FST decode.
- **Schulz–Mihov parametric automata** (*IJDAR* 5, 2002; with the dictionary walk in Mihov and
  Schulz, *Computational Linguistics* 30(4), 2004). The state is a parametric state plus an
  offset. A transition is a table lookup on the characteristic vector of the next 2d + 2 query
  symbols. The tables depend only on d and transpositions, not on the query.
  - **`levenshtein_automata` 0.2.1** (2021-05-18, last commit 2021-10-19, MIT, 25.9 M downloads,
    tantivy's): no `unsafe`, but `std` (no `no_std` attribute; it uses `Vec` and builds at run
    time). `LevenshteinAutomatonBuilder::new(d, transposition_cost_one)` supports Damerau as
    restricted (OSA) transpositions. `build_dfa(query)` materialises a per-query UTF-8 DFA as
    `Vec<[u32; 256]>`, up to C·(n + 1) states (C = 68 for d = 2 with transpositions). The README's
    benchmark for "Levenshtein": 36.5 µs at d = 1 with transpositions, 101 µs at d = 2. That
    build alone exceeds our budget per query.
  - The way to use the method is to embed the d = 1 and d = 2 tables (generated once, checked
    in) and step them per node. This is the A/B variant.
- **Bit-parallel simulation.** Myers (*JACM* 46(3), 1999) and Hyyrö, *A bit-vector algorithm for
  computing Levenshtein and Damerau edit distances* (*Nordic Journal of Computing* 10, 2003),
  which adds transpositions and notes that the method suits a test against a preset error
  threshold. Hyyrö (*J. Discrete Algorithms*, 2005) extends the Wu–Manber bitap NFA
  (*CACM* 35(10), 1992) with transpositions.
  - For a whole-key match, the anchored NFA with k = 1 has state R0, R1 (one `u64` each for
    queries of at most 63 symbols) plus the previous symbol's mask for the transposition term.
    A step is a shift, an AND and a few ORs per row, about 12 operations, and the state is dead
    when R1 = 0. k = 2 adds one row.
  - The per-query setup is a small table from query symbol to bit mask (at most 64 distinct
    symbols): a 128-entry array for ASCII and a sorted array for the rest. It costs nanoseconds
    and no allocation.
  - It works on code points: the FST yields UTF-8 bytes, and the walk steps the NFA only at a
    code-point boundary, keeping the partial bytes in the stack frame.
- **SymSpell** (symmetric deletion): exact probes of the query and its deletions, with no walk.
  Lookup is the fastest, but on Nepal the index is 9.2× the keys at d = 1 (4.2× with the prefix-7
  limit, §3.1). At 10^8 keys that is about 10^9 entries. ADR-0032 asks for it to be benchmarked;
  the size alone rules it out for packs (**proposal:** record that and drop the full build). The
  `symspell` crate is 0.5.2 (2026-03-22).

**Lowest per-node cost for one edit with transpositions:** the parametric table step and the
bitap step are both a few nanoseconds and need no per-query construction. Bitap is simpler,
needs no generated tables, gives k = 2 for one more word, and its liveness test is one compare.
Both are far below the DP row. The A/B decides by counted instructions per visited node.

### 3.7 Postings and top-k

- **Elias–Fano** (Vigna, *Quasi-succinct indices*, WSDM 2013; partitioned, Ottaviano and
  Venturini, SIGIR 2014) is near-optimal for long, dense, sorted lists. It needs sorted IDs, and
  per-list headers dominate on short lists.
- **PForDelta** (Zukowski et al., ICDE 2006; SIMD variants by Lemire and Boytsov, 2015) is
  bit-packing with exceptions, for long lists with a few outliers. Our B1 codec (D-073) already
  covers the core: blocks of 128, one width per block, FOR or delta. Exceptions help only when a
  block has a few large values. On short lists of large gaps there are none to help with.
- **On place names the lists are short and spread out.** 85% of keys have one record; IDs are in
  Hilbert order (D-071), so places sharing a name are far apart and gaps are near-uniform. FOR at
  ⌈log2 records⌉ bits (17 on Nepal, 27 at 10^8) is close to the entropy, and it does not care
  about order.
- **Importance order for the early stop.** Each list is sorted by importance descending, then
  record ID (deterministic). A candidate key's best record is its first posting. When the score
  is match bonus + importance, a list is read until bonus + importance falls below the current
  k-th score. This is the per-list case of block-max methods (Ding and Suel, SIGIR 2011).
- **Top-k prefix completion** (Hsu and Ottaviano, WWW 2013): a prefix is a contiguous ordinal
  range [lo, hi), found from two walks. Over that range, a block maximum of key importance (one
  `u8` per 128 keys, 714 B on Nepal) lets the scan visit blocks in descending maximum and stop
  when the k-th score beats the next block's maximum. A key's own importance is its first
  posting's importance, read from PLACES if that carries an importance column, else a `u8`
  column per key (about 90 KB on Nepal). Whether ADR-0032's precomputed table for 1–2-character
  prefixes is still needed is an A/B item.

### 3.8 How other geocoders do it

| System | Store | Fuzzy | Size (planet unless noted) | Source |
|---|---|---|---|---|
| Photon | OpenSearch 3.x (Lucene) | "typo tolerance" (Lucene fuzzy) | about 95 GB disk, +10%/year | README, 2026-11 |
| Pelias | Elasticsearch | edge n-grams for autocomplete; limited fuzzy (**unverified**) | server scale | README |
| Nominatim | PostgreSQL word table and tokens | none documented; little typo tolerance in user reports | at least 1 TB disk, 128 GB RAM recommended | install docs; OSM-dev 2016-11 |
| Airmail | tantivy | typo tolerance only for tokens of 8+ characters (roadmap) | 300 GB index (author, Reddit 2024, **unverified**); 10–100 GB free disk advised | README |
| Lucene `FuzzyQuery` | term dictionary + `LevenshteinAutomata` | Damerau (OSA) by default, at most 2 edits | — | Lucene 9.12 javadoc |
| Meilisearch | `fst` words + prefix Levenshtein | 1 typo from 5 characters, 2 from 9; a first-letter typo counts as 2 | — | typo tolerance docs |
| Mapbox carmen / `fuzzy-phrase` | FST with a deletion index (**unverified**, from memory) | — | — | repositories return 404 (2026-11-07) |

None publishes bytes per key or latency for an offline name index. The method itself (OSA
automata over a term dictionary, at most 2 edits, a stricter rule on the first letter) is
standard practice. Our differences are the size target (MB, not GB) and running in place.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| `fst` crate as the reader | Mature, in place, S3 used it | `std`; panics on hostile bytes (own comment); unmaintained | Unlicense/MIT | 0.4.7 (2021-06-06) | Oracle only |
| `fst-no-std` / `tantivy-fst` | `no_std` (nightly) / tantivy-tested | Same decoder; one release / 2023 | MIT | 0.4.8 / 0.5.0 | Reject |
| **Own FST, one tagged automaton** | Dead subtrees free; checkable; about 3 B per key | Builder and checker are ours | ours | n/a | **Build** |
| Front-coded blocks of 16 | Simplest; trivial check | 4.6 B per key; skip cost on dead prefixes | ours | n/a | A/B |
| LOUDS / MARISA trie | Smallest | rank/select per child step | BSD/LGPL (marisa), ? (`rsmarisa`) | rsmarisa 0.4.2 | Reject (speed, code) |
| `crawdad` | `no_std`, safe | Copies, panics, no walk, 8 B per node | MIT/Apache | 0.4.1 | Reject |
| `yada` | Fast | `unsafe` | MIT/Apache | 0.7.0 | Reject |
| DP row automaton (S3) | Built, recall met | O(m) per node | ours | n/a | Baseline |
| **Bitap NFA + Hyyrö transposition, code points** | About 12 ops per node, no setup, k = 2 for one word | Queries ≤ 63 symbols | ours | n/a | **Build** |
| Parametric tables stepped per node | One lookup per node | Generated tables; more code | ours (method), MIT reference | `levenshtein_automata` 0.2.1 | A/B |
| `levenshtein_automata` DFA per query | Proven in tantivy | 36 µs build per query; `std` | MIT | 0.2.1 | Reject |
| SymSpell | Fastest lookup | 9.2× keys at d = 1 | MIT (`symspell`) | 0.5.2 | Reject (size) |
| **B1 FOR postings, importance order** | Smallest measured; order free | Not optimal for very long lists | ours | D-073 | **Build** |
| Elias–Fano per list | Optimal on long sorted lists | Needs ID order; headers on short lists | ours | n/a | A/B on long lists |

## 5. What we build ourselves, and why

**Layout of SEARCH** (a draft for the pack-format page, settled by the B7 A/B):

```text
header    version, key count, map tag count, MAX_KEY, automaton length, column offsets
automaton our FST: keys = tag byte (0 exact, 1 loose, 2 phonetic) + folded key, output = ordinal
columns   (the shared column table, codec 0 = B1)
  starts  n + 1 list starts into ids          (B1 delta)
  ids     record IDs, each list by importance descending then ID   (B1 FOR)
  kmax    importance maximum per 128 ordinals (u8, bytes codec)
```

**Walk:** a fixed-depth stack of (node, transition index, NFA state, partial UTF-8). No allocation
per query beyond a caller-provided candidate buffer. Prefix: two walks give [lo, hi) on the
tagged key. Fuzzy: anchored or unanchored by whether the first-level children are filtered.

**Measurements that must prove it (B7):**

1. Size: SEARCH on Nepal (S3 inputs) against 3.6 MB, split into automaton, starts, ids and kmax.
   Own FST against front coding 16 against `fst` on the same keys.
2. Speed: instructions per query (the counts gate) and p50/p99 on the 512 golden queries.
   Three automata (DP row, bitap, parametric), anchored and unanchored, one and two edits.
3. Recall: the D-052 gates, plus the first-letter probe (target 1.000 unanchored) and a
   Devanagari one-letter typo probe (code points against bytes).
4. Robustness: a fuzz target on the section bytes (open plus `check` plus queries never panic),
   and a property test against the `fst` oracle and brute-force OSA over all keys.

## 6. Impact on the plan

- [Phase 2](../../07-planning/phases/phase-2.md) B7: the A/B list above replaces "our anchored
  Damerau automaton against a parametric one". The parametric automaton stays as one arm.
- [Pack format](../../04-architecture/pack-format.md) SEARCH row: "`fst` + Elias–Fano postings +
  block RMQ" becomes "own FST (tagged keys, ordinal outputs) + B1 FOR postings in importance
  order + block maximum", once B7 confirms it.
- [Format, compression and indexing](format-compression-indexing.md) §3.5 verdict ("Use `fst`",
  "Use `levenshtein_automata`", "Elias–Fano postings") is superseded by this note when B7 lands.
  That needs a decision-log row.
- ADR-0032 names "one `fst` map per normalised form" and a SymSpell benchmark. The tagged single
  automaton and dropping SymSpell on size need a decision row, and a superseding ADR if the
  maintainer sees the index structure as architecture. **Proposal**, not yet decided.
- The fuzzy unit becomes the code point, not the byte. This changes S3's behaviour on
  non-Latin scripts, so the golden numbers are re-baselined.

## 7. Risks and open items

- **Own FST is new code on the hot path.** Mitigation: the `fst` oracle, brute-force tests, a
  fuzz target, and the strict address-decreasing rule.
- **Front coding may win on simplicity** if the skip cost is small in practice. The A/B decides;
  either layout fits the section header.
- **Queries longer than 63 code points** skip fuzzy (S3 already caps at 31 bytes). Two-word
  bitap is the fallback if a script needs it.
- **Importance may not live in PLACES** yet; then `kmax` needs a per-key `u8` column (about
  90 KB on Nepal).
- **Very long lists** at world scale (a common name in a large country, phonetic buckets up to
  672 on Nepal) may favour Elias–Fano or a skip structure. Measure on the India wave.
- **The numbers in §3.1 come from an approximate fold** and GeoNames only. They show the
  direction, not the gate.
- **Unverified:** Pelias's fuzzy behaviour; Airmail's planet size; Mapbox `fuzzy-phrase`'s
  design (repositories gone); `rsmarisa`'s safety.

## 8. Outcome (B7, 2026-11-07)

Measured in [B7](../benches/phase-2.md#b7-search) and decided in D-085 and D-086. What changed
from §5's draft:

- Tags are 1, 2 and 3 (exact, loose, phonetic), so 0 stays free; the section is a plain column
  table with a LENGTH column instead of a header, and `kmax` is BLOCK_MAX.
- Importance lives in the section, one byte per row (IMPORTANCE, 16.6 KB on Nepal), as §7
  foresaw, so lists can be ordered and checked without PLACES.
- The automaton has a fourth form, `one` (one transition to any target), and the first output
  of a node is implied: 488 → 457 KB.
- A loose key stores only the rows its exact twin lacks: 2.13 → 1.50 MB, near the 1.5 MB
  estimate.
- The NFA is Wu–Manber with a transposition term in Baeza-Yates and Navarro's layout; the
  parametric arm was not built because the walk fell to 3.7 µs. Front coding lost on speed
  (12.0 µs with the DP, 6.4 µs with the NFA, against 3.7 µs on the FST).
- Unanchored fuzzy is the default: p99 41.2 µs, first-letter probe 1.000.

## 9. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| `fst` | 0.4.7 (2021-06-06); last commit 2024-09-25 | 2026-11-07 | crates.io API, GitHub clone |
| `fst-no-std` | 0.4.8 (2024-01-18) | 2026-11-07 | crates.io API, GitHub clone |
| `tantivy-fst` | 0.5.0 (2023-11-21) | 2026-11-07 | crates.io API, GitHub clone |
| `levenshtein_automata` | 0.2.1 (2021-05-18); last commit 2021-10-19 | 2026-11-07 | crates.io API, GitHub clone |
| `crawdad` | 0.4.1 (2026-08-12) | 2026-11-07 | crates.io API, GitHub clone |
| `yada` | 0.7.0 (2026-06-06) | 2026-11-07 | crates.io API, GitHub clone |
| `rsmarisa` | 0.4.2 (2026-06-01) | 2026-11-07 | crates.io API |
| `sucds` | 0.11.0 (2026-10-25) | 2026-11-07 | crates.io API |
| `vers-vecs` | 1.10.2 (2026-08-10) | 2026-11-07 | crates.io API |
| `tantivy`, `tantivy-sstable` | 0.27.0 (2026-10-17, 2026-10-16) | 2026-11-07 | crates.io API |
| `symspell` | 0.5.2 (2026-03-22) | 2026-11-07 | crates.io API |
| `bitpacking` | 0.9.3 (2026-01-08) | 2026-11-07 | crates.io API |
| Photon | main, pushed 2026-11-02 | 2026-11-07 | GitHub |
| Nominatim | main, pushed 2026-11-03 | 2026-11-07 | GitHub, docs |
| Airmail | main, pushed 2025-08-08 | 2026-11-07 | GitHub |

## 10. Sources

- [fst](https://github.com/BurntSushi/fst), BurntSushi, source read 2026-11-07 (`src/raw/mod.rs`, `src/raw/node.rs`).
- [Index 1,600,000,000 Keys with Automata and Rust](https://burntsushi.net/transducers/), Andrew Gallant, 2015, accessed 2026-11-07.
- [fst-no-std](https://github.com/CrabNejonas/fst), accessed 2026-11-07.
- [levenshtein-automata](https://github.com/tantivy-search/levenshtein-automata), Paul Masurel, README benchmark, accessed 2026-11-07.
- [tantivy sstable format](https://github.com/quickwit-oss/tantivy/blob/main/sstable/README.md), Quickwit, accessed 2026-11-07.
- [crawdad](https://github.com/daac-tools/crawdad), [yada](https://github.com/takuyaa/yada), accessed 2026-11-07.
- [Lucene 9.12 FuzzyQuery](https://lucene.apache.org/core/9_12_0/core/org/apache/lucene/search/FuzzyQuery.html), Apache, accessed 2026-11-07.
- [Meilisearch typo tolerance settings](https://www.meilisearch.com/docs/learn/relevancy/typo_tolerance_settings), accessed 2026-11-07.
- [Photon](https://github.com/komoot/photon), komoot, accessed 2026-11-07.
- [Nominatim installation](https://nominatim.org/release-docs/latest/admin/Installation/), accessed 2026-11-07.
- [Nominatim and/or a fault-tolerant geocoder](https://lists.openstreetmap.org/pipermail/dev/2016-November/029596.html), OSM-dev, 2016-11.
- [Airmail](https://github.com/ellenhp/airmail), Ellen Poe, README accessed 2026-11-07; planet size from the author's [r/rust comment](https://redlib.hbubli.cc/r/rust/comments/1ascktr/geocode_the_planet_10x_cheaper_with_rust/kqq3vha), 2024-02.
- [Hyyrö, A bit-vector algorithm for computing Levenshtein and Damerau edit distances](https://www.Gwern.net/doc/cs/algorithm/2003-hyyro.pdf), *Nordic Journal of Computing* 10, 2003.
- Hyyrö, *Bit-parallel approximate string matching algorithms with transposition*, *J. Discrete Algorithms* 3, 2005.
- Myers, *A fast bit-vector algorithm for approximate string matching based on dynamic programming*, *JACM* 46(3), 1999.
- Wu and Manber, *Fast text searching allowing errors*, *CACM* 35(10), 1992.
- Schulz and Mihov, *Fast string correction with Levenshtein automata*, *IJDAR* 5, 2002.
- Mihov and Schulz, *Fast approximate search in large dictionaries*, *Computational Linguistics* 30(4), 2004.
- Daciuk, Mihov, Watson and Watson, *Incremental construction of minimal acyclic finite-state automata*, *Computational Linguistics* 26(1), 2000.
- [Martínez-Prieto et al., Practical compressed string dictionaries](https://users.dcc.uchile.cl/~gnavarro/abstracts/is15.2.html), *Information Systems* 56, 2016.
- Vigna, *Quasi-succinct indices*, WSDM 2013; Ottaviano and Venturini, *Partitioned Elias–Fano indexes*, SIGIR 2014.
- Zukowski, Héman, Nes and Boncz, *Super-scalar RAM-CPU cache compression*, ICDE 2006; Lemire and Boytsov, *Decoding billions of integers per second through vectorization*, SPE 45(1), 2015.
- Ding and Suel, *Faster top-k document retrieval using block-max indexes*, SIGIR 2011.
- Hsu and Ottaviano, *Space-efficient data structures for top-k completion*, WWW 2013.
- Spike S3 and D-052, D-071, D-073, linked above.
