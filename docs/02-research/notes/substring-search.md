---
title: "Substring (infix) search of 3+ characters, Latin and Devanagari (TG-06)"
status: accepted
updated: 2026-11-11
owner: maintainer's agents
revisit: 2027-05-11
---

# Substring (infix) search of 3+ characters, Latin and Devanagari (TG-06)

## 1. Question

Consumer request TG-06 ([tithipatra](../../01-product/consumers/tithipatra.md), phase 4 item
4.9b in [phase 4](../../07-planning/phases/phase-4.md)): search must find a place from any
substring of 3 or more characters of one of its names, in Latin or Devanagari. Examples:
काठमाडौं from "काठ", Kathmandu from "kathm" and from the middle of the word, "mandu". Results are
ranked exact, then prefix, then substring, then population or importance. The names are Nepal's
and India's, in ne, hi, en and romanised forms.

The search we have ([ADR-0032](../../03-decisions/adr/ADR-0032-text-search.md),
[the SEARCH note](search-index.md), [B7](../benches/phase-2.md#b7-search)) does exact, loose,
prefix and fuzzy matching (a bit-parallel NFA, up to 2 edits) plus a phonetic key, all over one
FST of whole folded names. This note answers:

1. What the current search returns for a substring from the middle of a word.
2. Which index to add: (a) word-suffix keys in an FST, (b) a trigram posting index, (c) an
   FM-index or suffix array, or (d) a linear scan over the folded text. Size on Nepal and India,
   p99 latency (target under 100 µs warm), and how each one works with per-pack layering.
3. Whether "3 characters" means code points or graphemes in Devanagari.
4. How the new match fits the ranking, which golden cases to add, and what the size budget is.

## 2. Summary and recommendation

1. **Today a mid-word substring is not found.** Keys are whole folded names (`search_keys` in
   `teigeodata-names`), prefix ranges start at the first letter of the name, and fuzzy matching
   compares the whole key within one edit. Probed on the CLI's 911-place Nepal test pack
   (2026-11-11): "holashi" finds Kholashi (one deletion), but "olashi" finds nothing and "lashi"
   returns a different place (Lashideu, by prefix). The second word of a name ("lekhnath" in
   "Pokhara Lekhnath") is not a prefix either.
2. **Recommendation: option (a), word-suffix keys in our own FST, stored as a separate
   `SUBSTRING` section in its own component.** Every suffix of every folded token that has 3 or
   more code points and does not start on a combining mark becomes a key. The suffix at offset 0
   of the first token is left out because the EXACT prefix range already covers it. A substring
   query is a prefix range on these keys, read best-first with the existing `best_in` and block
   maxima. This reuses the reader, the checker, the writer and the prefix-tier code unchanged.
   Measured on a GeoNames proxy for Nepal (129,799 rows): **2.49 MB raw (19.2 B per row), p99
   35 µs** for the top 16 under a load average of 50–90. On a 5× copy (an India proxy with
   649 k rows) p99 is 36 µs. Keeping the keys in their own section costs only 9 KB more than
   sharing the SEARCH automaton, and it lets a user who does not need substring search skip the
   download.
3. **Rejected:** (b) trigrams are the same size (669 k postings on Nepal, about 1.42 MB before
   offsets) and slower (481 µs for "khara" on the India proxy before ranking), and their postings
   cannot be read best-first by importance. (c) An FM-index is smaller but has to locate every
   hit before ranking ("ara" has 4,870 on Nepal), and a plain suffix array is no smaller than
   (a). (d) A linear scan takes 0.7 ms on Nepal and 4–5 ms on the India proxy, 7–50× over the
   budget.
4. **"3 characters" means 3 code points**, as in ADR-0032 (prefix matching is on code points)
   and the fuzzy NFA. काठ is 3 code points (क ा ठ) but only 2 extended grapheme clusters (का, ठ),
   so a grapheme rule would refuse the request's own example. Suffixes never start on a
   General_Category M code point (a vowel sign, virama or anusvara). This rule comes from Unicode
   properties, not from one script (P16). 37% of the Devanagari code points in the data are marks.
5. **Ranking:** add a substring bonus of **20**: below prefix (30) and one edit (25), above two
   edits (15) and phonetic (5). It is not a strong match, so it does not suppress the unanchored
   fuzzy pass. With one additive score (D-052), an important place found by substring can still
   outrank an unimportant exact match. That conflicts with TG-06's strict order, and the
   maintainer needs to decide it (§7).
6. **Size budget:** SUBSTRING at most **24 B raw per place** (measured 19.2), reported raw and on
   the wire in `SIZES.md`. That is about 2.5 MB for Nepal and about 13 MB raw for India
   (**estimate**; India split builds, D-191, divide it). Latency gate: p99 under 100 µs for the
   whole query, with the substring tier included.

## 3. Findings

### 3.1 What the current search does with a substring (2026-11-11)

`target/debug/tgd search --pack target/tmp/cli-queries/np.tgd` (a 911-place test pack, debug
build). Times were not measured. The point is which tier fires.

| Query | Name in the pack | Result | Why |
|---|---|---|---|
| `Kholashi` | Kholashi | found, score 177 | exact (60) |
| `kholas` | Kholashi | found, 147 | prefix (30) |
| `holashi` | Kholashi | found, 142 | one edit (a leading deletion), unanchored fuzzy |
| `olashi` | Kholashi | nothing | two deletions; the main query runs with one edit |
| `lashi` | Kholashi | Lashideu only | prefix of another name |
| `haratar` | Dharatar | found, 33 | one edit |
| `ratar` | Dharatar | Rata (wrong) | one edit to another name |

So unanchored fuzzy finds a substring that is missing exactly one leading character, and nothing
shorter. Raising the edit budget would not help: "mandu" is 4 edits from "kathmandu".

### 3.2 Measurements made for this note (2026-11-11)

**Input:** GeoNames `NP.txt` (`data/cache/geonames-np/`): the P and A rows, the name, ASCII name
and alternate names, with an approximate fold (NFKD, Latin marks dropped, Devanagari nukta
dropped and chandrabindu turned into anusvara, lower case, punctuation turned into one space).
This gives 129,799 rows, 91,445 distinct folded names and 132,078 (key, row) pairs. Importance
is approximated as class base plus 12·log10(population). The proxy has only 95 Devanagari keys;
the real pack has more from OSM (§3.4).

**Code:** a throwaway bench (not checked in) builds real SEARCH bodies with
`teigeodata_format::search::encode` (`write` feature), opens them from a pack, and queries them
through `Search::range` and `Search::best_in`, as the engine's prefix tier does. It also
contains a `str::find` scan and an in-memory trigram index. Release build, Apple M2 Pro, load
average **50–94** during the runs (other projects), so the times are an upper bound. Each
query's time is the best of 11 runs, and p50 and p99 are taken over 2,000 queries: 11 fixed
ones plus random mid-word substrings of 3–8 code points cut from random names (seeded).

**Option (a), word-suffix keys:**

| Variant (Nepal proxy) | Suffix keys | (key, row) pairs | Section body | Growth over exact-only (823,376 B) |
|---|---:|---:|---:|---:|
| every token suffix of 3+ code points | 292,039 | 862,447 | +3,083,948 B | ×4.75 |
| **and without the first token's offset 0** | **216,220** | **730,775** | **+2,485,576 B** | **×4.02** |
| as above, 4+ code points | 212,798 | 589,646 | +2,180,852 B | ×3.65 |
| the chosen keys in their own section | 216,220 | 730,775 | 2,494,168 B | +9 KB against shared |

The automaton is not where the bytes go. In the chosen variant it is 716,324 B on its own, and
the rest is the lists (730,775 row IDs of 17 bits ≈ 1.55 MB, plus their starts). Suffixes share
tails well in a minimal automaton; postings do not shrink that way.

The **4-code-point minimum is wrong**, not just smaller. A 3-code-point query ranges over
suffixes that start with it, so a name that ends in those 3 code points needs its own 3-code-point
suffix. With the 4 minimum, "pur" lost Kanchanpur, Makwanpur and Lalitpur, and 27 of the 2,000
random queries found nothing.

**Latency** of the best 16 rows by importance for a substring, in one layer:

| Floor rule in `best_in` | p50 | p99 | max | Rows read p50 / p99 |
|---|---:|---:|---:|---:|
| ties with the worst kept row are still read (the prefix tier's rule) | 2.0 µs | 243 µs | 519 µs | 23 / 2,610 |
| **strict: a key whose best row only ties the worst kept row is skipped** | **1.5 µs** | **31–35 µs** | 39–57 µs | 17 / 38 |
| strict, 5× India proxy (649 k rows) | 1.3 µs | 36 µs | 86 µs | 17 / 57 |

The prefix tier reads ties so that it can break them by stable ID. Over a substring range, a
3-letter query such as "ara" or "pur" covers thousands of keys with the same low importance, so
reading ties costs p99 7×. A substring tier breaks ties by key order and row order instead. That
order is deterministic, but it is not by ID (§7).

Top results by importance (proxy importance) for sample queries, strict:

| Query | Top rows |
|---|---|
| mandu | Kathmandu (PPLC), Central Region, Kathmandu district, … |
| athm | Kathmandu, … |
| khara | Samkhara, Bhimpokhara, Thulapokhara, … (Pokhara itself is in the EXACT prefix range, not in SUBSTRING; the engine merges both tiers) |
| pur | Kathmandu (alternate "Kantipur"), Kanchanpur, Makwanpur, Lalitpur |

**Option (b), trigrams** (code-point trigrams of each distinct folded name → sorted name IDs,
intersect by binary search, verify with `str::contains`):

| | Nepal | 5× proxy |
|---|---:|---:|
| distinct trigrams | 8,694 | 9,864 |
| postings | 669,116 | 4,071,031 |
| postings bytes at 17 / 19 bits, before list offsets | ≈ 1.42 MB | ≈ 9.67 MB |
| "mandu" / "athm" | 3.1 / 0.9 µs | 17.9 / 6.0 µs |
| "khara" / "nagar" | 60.0 / 34.9 µs | 481 / 207 µs |

Times are before ranking. Every verified name must then have its rows read and scored, because
the postings are in name order, not importance order. Verification can read each name back
from the FST (`Fst::key(ordinal)`), so no extra text is stored, but the index is no smaller than
(a) and is slower on common trigrams.

**Option (c), suffix structures:**

- A suffix array over the joined folded names: 952,529 B of text, 20-bit entries, about
  2.4 MB plus the text. A suffix array over word starts only (an (ordinal, offset) pair per
  suffix) has 544,506 entries (the (suffix, name) pairs) at about 25 bits each, about 1.7 MB.
  Neither is smaller than (a), and binary search must rebuild a key from the FST at each probe.
- An FM-index over the same text would be about 40–60% of the text with sampled locate data
  (**estimate**, no crate measured). Counting is fast, but ranking needs every occurrence
  located first. That is about one LF step per sample distance for each hit, and "ara" has 4,870
  hits on Nepal and 34,718 on the proxy. It also has no importance order, and it is a large,
  unaudited structure for a `no_std` reader that must not panic on hostile bytes.

**Option (d), linear scan** (`str::find` over the joined folded names, each hit mapped to its
name by binary search):

| Query | Nepal (952 KB) | 5× proxy (5.5 MB) |
|---|---:|---:|
| mandu | 738 µs | 3,945 µs |
| ara | 891 µs | 5,223 µs |
| zzq (no hit) | 159 µs | 891 µs |
| काठ | 82 µs | 527 µs |

7–50× over the budget before ranking, and linear in the pack. It is useful only as the test
oracle.

### 3.3 Code points, graphemes and where a suffix may start

- काठमाडौं is 8 code points: क ा ठ म ा ड ौ ं. काठ is 3 code points and 2 extended grapheme
  clusters. Since Unicode 15.1 the grapheme rule GB9c also joins virama conjuncts (क्ष) into one
  cluster, so counting graphemes would make Devanagari queries "shorter" than Latin ones of the
  same keystrokes. The search fold, the prefix ranges and the fuzzy NFA already count code
  points (ADR-0032, D-085), so the minimum of 3 does too.
- A user does not type a query that starts with a vowel sign or a virama. Starting suffixes only
  on code points whose General_Category is not M removes 37% of the Devanagari start positions
  (373 of 1,010 code points in the proxy's Devanagari keys) and changes nothing for Latin (its
  marks are folded away). A start after a virama (ज in ञ्ज) is kept: the rule is about marks, not
  clusters, and it stays one Unicode property for every script.
- The Devanagari fold already makes काठमांडू and काठमाडौं share काठ, and the anusvara fold applies
  to the suffixes as well, because they are cut from the folded key.

### 3.4 What the real pack changes

The Nepal 2026.11.0 main pack has 100,358 places and a 2,487,196 B SEARCH section
([components](components.md)). It carries more names per place (OSM `name:*`, romanised forms)
and more Devanagari than the GeoNames proxy. The alternate names add little in the proxy (the
primary and ASCII names alone give 722,067 of the 730,775 pairs), so the size follows
**name forms per place × code points per form**. A full ne or hi name on every place adds about
(1 − 0.37) × its code points of suffix starts, about +60% of the Latin cost per form
(**estimate**). The forge measures this on the real pack before the budget is fixed (§6).

### 3.5 How others do it

| System | Method | Notes |
|---|---|---|
| SQLite FTS5 `trigram` tokenizer (since 3.34.0, 2020-12-01) | trigram postings; indexed `LIKE '%x%'` and `GLOB` | substrings under 3 characters match nothing; `remove_diacritics` turns off LIKE/GLOB indexing; `detail=none` shrinks the index when only LIKE/GLOB is used |
| Tantivy `NgramTokenizer` (0.27.0) | `new(min_gram, max_gram, prefix_only)`; `all_ngrams` for infix | n-gram sizes count characters, offsets are bytes; a server or desktop index, too large for our mobile/WASM budget (ADR-0032) |
| Typesense (v30.x docs) | per-field `"infix": true` plus a query `infix=off/always/fallback` | its docs recommend infix for small fields (codes, emails); "requires an additional data structure"; uses only the first word of the query; `fallback` runs only when normal search found nothing |
| Meilisearch | prefix on the last query word only; no infix | substring search is an open community request |

Typesense's `fallback` mode and per-field opt-in, and FTS5's 3-character minimum, are the same
trade-offs we make: opt-in by component, a 3-code-point minimum, and a tier that ranks below
prefix.

## 4. Options compared

Nepal is the GeoNames proxy (129,799 rows); "India" is the 5× proxy (649 k rows, **estimate** of
India's size). Latency is under load 50–94.

| Option | Size Nepal / India | p99 | Best-first by importance | Per-pack layering | Reader rules (`no_std`, checked, never panics) | Verdict |
|---|---|---|---|---|---|---|
| **(a) word-suffix keys, own FST, own section** | 2.49 MB / ≈ 13 MB | 35 / 36 µs | yes (`best_in`, block maxima) | as SEARCH: one section per layer, candidates merged by importance, `claimed_above` hides overridden rows | reuses `Search` (open, check, fuzz) unchanged | **chosen** |
| (b) trigram postings | ≈ 1.42 MB + offsets / ≈ 9.7 MB + offsets | 60 µs Nepal, 481 µs India before ranking | no | per layer, but each layer must read every match | new section and checker | no |
| (c) suffix array / FM-index | 1.7–2.4 MB / ≈ 9–13 MB; FM smaller | SA: log n FST key rebuilds; FM: locate per hit | no | per layer | large new structure | no |
| (d) linear scan | 0 extra / 0 | 0.9 ms / 5.2 ms | no | per layer | trivial | test oracle only |

## 5. What we build ourselves, and why

**Keys (forge, `teigeodata-names`).** `search_keys` gains a `SearchMap::Substring` output. For
each folded name form and each token *t* at index *i*, it outputs every suffix of *t* that starts
on a code point outside General_Category M and has at least 3 code points, except the suffix at
offset 0 when *i* = 0. Keys over 254 bytes are skipped, as today. Every name form the forge keys
(en, ne, hi, romanised, alternates) gets suffixes. One code path serves every script and country
(P16).

**Section.** A new kind `SUBSTRING`, the SEARCH body layout with one tag, plus its own
IMPORTANCE column (1 B per row, 130 KB on Nepal) so that it opens with `Search::open`
unchanged. It belongs to a new component `Substring` that depends on `Search` and `Names`
([components](components.md)). The automaton, lists, block maxima and checks are the existing
ones, and the fuzz harness and brute-force tests cover the new kind by construction.

**Engine.** For a query of at least 3 code points with no space, a substring tier runs
alongside the prefix tier. It is `prefix_best` over `SUBSTRING` instead of `EXACT`, with the
strict floor. It keeps the best 16 by importance across layers and adds them in `layer_hits`
with `SUBSTRING_BONUS = 20`. Rows found by several tiers keep their highest bonus (the existing
sort and dedupe). A multi-word query does not use the substring tier (§7).

**What must be measured (the gates):**

1. Size per place of SUBSTRING on the real Nepal pack, raw and wire, at most 24 B raw per place.
2. Golden recall: the existing 512 Nepal queries must not lose recall@1 or MRR@10 (the substring
   tier only adds lower-bonus candidates; any loss is a ranking bug). The new `substring` class
   must reach recall@5 ≥ 0.95.
3. p99 of the whole query, the unanchored pass included, under 100 µs, gated on instruction
   counts ([benchmark method](../../06-engineering/benchmarks.md)).
4. A/B: the substring tier always on, against only when fewer than *k* strong hits exist
   (Typesense's `fallback`). Keep "always" unless it fails gate 3.
5. Differential test: SUBSTRING range results equal a brute-force scan (option d) on random
   keys in ASCII, 2-byte and 3-byte UTF-8, including mark starts.

**Golden cases to add** to `data/countries/np/golden-curated.yaml` (schema
`teigeodata/golden/v0`, a new class `substring`; the golden generator and the evaluator must
accept the class). IDs and names were checked against `NP.txt` on 2026-11-11:

```yaml
  - id: np-c013
    class: substring
    query: "mandu"
    expect: { "geonames:1283240": 3 }
    max_rank: 1
    source: "GeoNames 1283240 Kathmandu; a mid-word substring (TG-06)"
  - id: np-c014
    class: substring
    query: "athmand"
    expect: { "geonames:1283240": 3 }
    max_rank: 1
    source: "GeoNames 1283240 Kathmandu; a mid-word substring (TG-06)"
  - id: np-c015
    class: substring
    query: "काठ"
    expect: { "geonames:1283240": 3 }
    max_rank: 1
    source: "GeoNames 1283240 alternate काठमाडौं; 3 code points, 2 graphemes (TG-06)"
  - id: np-c016
    class: substring
    query: "माडौं"
    expect: { "geonames:1283240": 3 }
    max_rank: 1
    source: "GeoNames 1283240 alternate काठमाडौं; a mid-word Devanagari substring (TG-06)"
  - id: np-c017
    class: substring
    query: "khara"
    expect: { "geonames:1282898": 3 }
    max_rank: 3
    source: "GeoNames 1282898 Pokhara; many smaller names also contain 'khara' (TG-06)"
  - id: np-c018
    class: substring
    query: "खरा"
    expect: { "geonames:1282898": 3 }
    max_rank: 3
    source: "GeoNames 1282898 alternate पोखरा (TG-06)"
  - id: np-c019
    class: substring
    query: "ratnagar"
    expect: { "geonames:1283582": 3 }
    max_rank: 1
    source: "GeoNames 1283582 Biratnagar (TG-06)"
  - id: np-c020
    class: substring
    query: "राटनगर"
    expect: { "geonames:1283582": 3 }
    max_rank: 1
    source: "GeoNames 1283582 alternate बिराटनगर (TG-06)"
  - id: np-c021
    class: substring
    query: "Pokhara"
    expect: { "geonames:1282898": 3 }
    max_rank: 1
    source: "Guard: an exact match stays first over names containing it (Bhimpokhara, Thulapokhara)"
```

India cases are added with India's golden set: for example "galuru" (Bengaluru), "दराबाद"
(हैदराबाद) and "ananthapuram" (Thiruvananthapuram), each with its GeoNames ID checked.

## 6. Impact on the plan

- **Decision log:** a D-row records option (a) as a separate SUBSTRING section and `Substring`
  component, the 3-code-point minimum, the no-mark-start rule, the bonus of 20, the strict floor
  and the 24 B per place budget.
- **ADR-0032** gets a refinement (the ADR stays; this is a new matching tier, not a new
  architecture). [The pack format](../../04-architecture/pack-format.md#search) gets the new
  kind, and the components page gets the new component.
- **Phase 4, TG-06:** research done; the implementation steps are keys, section, engine tier,
  golden class, then the gates of §5 on the real Nepal pack.
- **Golden schema:** the class list gains `substring`.
- `SIZES.md` gains the SUBSTRING row per pack.

## 7. Risks and open items

- **Strict order against one additive score.** TG-06 asks for exact, then prefix, then
  substring, then importance. D-052 measured strict tiers and rejected them (they failed prefix
  queries). With bonus 20 and importance 0–255, an important place found by substring outranks
  an unimportant exact match: "mandu" puts Kathmandu above a village named Mandu, if one exists.
  We recommend keeping the additive score and checking the order with the guard cases above.
  The consumer should confirm, and the maintainer decide. **Open question.**
- **Ties by key order, not ID** in the substring tier (strict floor). This is deterministic for
  one pack, but the order of equal-importance rows can change between releases. The prefix tier
  keeps its tie-by-ID rule.
- **Size on the real pack is not yet measured.** The proxy has few Devanagari names. If full ne
  and hi names push Nepal over 24 B per place, the A/B levers are: substring keys only for the
  name forms the country profile lists (a data rule), and per-list delta coding if rows are
  numbered by importance.
- **India:** about 13 MB raw (**estimate**, linear from Nepal). This is why the index is
  opt-in. India's state split builds (D-191) put it in per-state parts.
- **Not covered:** typos inside a substring (no fuzzy substring), and substrings across a space
  ("du met"). A multi-word query uses the existing tiers only. Revisit if a consumer asks.
- The bench was run under load average 50–94. Re-measured in the implementation step under
  load 14–25 (§10); instruction counts still need a Linux run.
- While reading, the Typesense docs page contained text addressed to AI agents asking for
  another page to be fetched. It was ignored; only the documented facts above were used.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| SQLite FTS5 trigram tokenizer | since 3.34.0 (2020-12-01) | 2026-11-11 | sqlite.org/fts5.html; release date from secondary sources |
| tantivy `NgramTokenizer` | 0.27.0 | 2026-11-11 | docs.rs |
| Typesense search API (infix) | docs v30.x (inferred from links) | 2026-11-11 | typesense.org |
| Meilisearch prefix search | current docs | 2026-11-11 | meilisearch.com |
| GeoNames `NP.txt` | the cached copy in `data/cache/geonames-np/` | 2026-11-11 | local |
| Rust toolchain | 1.99.1 | 2026-11-11 | `rust-toolchain.toml` |

## 9. Sources

- [SQLite FTS5, the trigram tokenizer](https://www.sqlite.org/fts5.html), SQLite, accessed 2026-11-11.
- [sqlite-fts5-trigram](https://github.com/simonw/sqlite-fts5-trigram), Simon Willison, accessed 2026-11-11 (it shipped in 3.34.0).
- [tantivy `NgramTokenizer`](https://docs.rs/tantivy/latest/tantivy/tokenizer/struct.NgramTokenizer.html), docs.rs, 0.27.0, accessed 2026-11-11.
- [Typesense search parameters, `infix`](https://typesense.org/docs/latest/api/search.html), Typesense, accessed 2026-11-11.
- [Meilisearch prefix search](https://www.meilisearch.com/docs/learn/engine/prefix), Meilisearch, accessed 2026-11-11.
- [Meilisearch issue 4636](https://github.com/meilisearch/meilisearch/issues/4636), prefix only on the last word, accessed 2026-11-11.
- [UAX #29 Unicode Text Segmentation](https://www.unicode.org/reports/tr29/), rule GB9c (Unicode 15.1), Unicode Consortium.
- Daciuk, Mihov, Watson and Watson, "Incremental construction of minimal acyclic finite-state automata", *Computational Linguistics* 26(1), 2000 (the FST, via [the SEARCH note](search-index.md)).
- Ferragina and Manzini, "Opportunistic data structures with applications", FOCS 2000 (the FM-index).

## 10. As built (2026-11-11)

Built as §5 says (D-199), with these differences and numbers.

**Differences from §5:**

- The keys come from a function of their own, `teigeodata_names::substring_keys(folded, out)`,
  not from a fourth output of `search_keys`: the world pack and the baseline call `search_keys`
  and need no suffixes, and the new function returns slices of the fold, so it allocates
  nothing. The forge calls it on every name it keys for SEARCH.
- "General_Category M" is `unicode_normalization::char::is_combining_mark`, which
  `teigeodata-names` already uses for its fold.
- `Search::check` proves on a SUBSTRING section that every key is tag 4 and at least 3 code
  points of UTF-8. The no-mark rule is the writer's; the reader has no Unicode tables and a key
  that breaks it changes no answer.
- The prefix tier and the substring tier share one function (`best_by_importance`); only the
  map and the floor rule differ.
- A layer whose pack has SUBSTRING but which was installed without `substring` adds nothing to
  the tier, and `search` still answers; installing `substring` without `search` is refused
  with `MissingComponent(search)`.

**Measured** with `cargo run --release -p teigeodata-engine --example substring --
data/cache/geonames-np/NP.txt` (checked in): the proxy of §3.2, but keyed by the real fold,
`search_keys` and `substring_keys`, importance a class base plus 12·log10(population). A real
Nepal build was not run: its inputs (OSM, Wikidata, buildings, DEM) are not all in the local
store and were not fetched. Apple M2 Pro, release build, load average 14–25 from other
projects; each query the best of 11 runs.

| | substring off | substring on |
|---|---:|---:|
| SEARCH (exact and loose maps, no phonetic, no rules) | 1,028,032 B | 1,028,032 B |
| SUBSTRING | – | 2,586,228 B, 19.9 B per place |
| whole `search`, 2,000 queries: p50 / p99 / max | 4.9 / 80.5 / 243 µs | 10.8 / 115.3 / 284 µs |
| the 1,287 queries of 4 or more code points: p50 / p99 | 6.1 / 37.5 µs | 10.4 / 41.0 µs |
| random mid-word queries: the source place first / in the top 10 | 31 / 53 | 159 / 541 |
| the nine golden `substring` cases (np-c013 to np-c021) at rank 1 | 2 (काठ, Pokhara) | 9 |

The slowest queries are 3 code points ("kha", "bha", "cha"): 243 µs already without the tier,
because the prefix tier reads ties to break them by ID over thousands of keys; the tier adds
about 40–60 µs to them. Random mid-word queries are often common fragments, so the source
place is one of many equal matches; "in the top 10" is a floor, not a recall figure.

**Gates of §5:** (1) 19.9 B per place on the proxy, under 24; the real pack is still to be
measured. (2) Golden recall: the `substring` class is 9 of 9 at rank 1 on the proxy; the
existing 512 queries must be scored on a real pack. (3) p99 under 100 µs holds for queries of
4 or more code points (41 µs), not for 3-code-point queries; it was timed under load, not
counted in instructions (Callgrind runs on Linux only). (4) Always on was kept: a fallback
that runs only without strong matches would hide Pokhara behind names that start with
"khara". (5) The differential test is in `teigeodata-names`: exact prefix plus substring keys
find what a scan of the folds finds, over ASCII, 2-byte and 3-byte UTF-8 and mark starts.

### 10.1 On the real Nepal main pack (2026-11-11)

Built with `forge make np` once the inputs were fetched: SUBSTRING 2,561,452 B (25.0 B per
place over 102,367 places, above the proxy's 19.9 B: more names per place), 2.08 MB on the
wire. Golden (`pack-eval`, product engine): the nine `substring` cases all within rank 3,
four at rank 1; the others lose to a same-name district or municipality ("mandu":
Kathmandu district first) or, by design, to a one-edit match ("ratnagar": Ramnagar), so
their `max_rank` is 3 like the other same-name cases. All queries p50 7.4 µs, p99 110 µs.
The first real build lost "Nepa" → Nepalganj: Wikidata labels such as "Bara District,
Nepal" made the word "Nepal" a key of every district; D-202 drops such names.

