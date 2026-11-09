---
title: "Canonical names, romanisation and corrections (step 3.6)"
status: research
updated: 2026-11-09
owner: maintainer's agents
revisit: 2027-05-09
---

# Canonical names, romanisation and corrections (step 3.6)

## 1. Question

Step 3.6 of [phase 3](../../07-planning/phases/phase-3.md) builds three things:

1. **The canonical display name** of each place, by the policy of
   [names and search §5.1](names-and-search.md#51-canonical-display-spelling-policy).
2. **A readable romanisation** of Devanagari for places with no Latin name, by the profile of
   [§5.4](names-and-search.md#54-readable-romanisation-profile-devanagari--latin-for-step-6-and-indexing).
3. **Corrections as data** with guards, by [forge pipeline §3.7](forge-pipeline.md#37-corrections-as-data-and-qa-queues).

The design work is in those notes. This note answers what they left to measurement:

- How often the current build shows the wrong script.
- How accurate the rule-table romaniser is against names people actually write.
- Which rules earn their place.
- Where the rules live so that no code names a country (P16).

## 2. Summary and recommendation

- **The display name is the bigger problem, not romanisation.** The 3.1 build takes the first
  source record's name. OSM is first, so **6,269 of 98,020 settlements and localities (6.4%)**
  show a Devanagari canonical name. Yet only **243** clusters have no Latin name in any
  source. The fix is the §5.1 order:
  1. Local usage among the cluster's Latin names.
  2. GeoNames, then Wikidata.
  3. Romanisation only when no Latin name exists.

  Every other spelling stays an alternate name, so search loses nothing.
- **The romaniser beats ICU by 2.7× on exact match.** On 6,660 OSM Nepal places that carry
  both a Devanagari and a Latin name (2026-11-09), the rule tables give the writer's Latin name
  exactly on **58.4%** (character error rate 0.149). ICU 79.1 `Devanagari-Latin; Latin-ASCII`
  gives 22.0% (0.260). Split halves agree within 2.1 points.
- **Rules kept, by measured gain:** word-final schwa deletion (+15.4 points), the country
  lexicon's terms as morphemes in their canonical form, with a final ँ as n (+4.9), left-to-right medial
  deletion (Ohala's VC_CV rule; +1.3), aspirated geminates written once (+2.4), ङ before a
  velar as n (+1.2), no medial deletion before ह (+0.3).
  - **Rule rejected:** a positional व → b/v/w guess (−0.6). OSM writes w, and §5.4 already
    said guessing is worse than an alternate name.
- **Rules are data.** A script's letters and its schwa behaviour go in a profile per language
  and script, `data/romanise/<lang>-<Script>.yaml` (first: `ne-Deva`). The lexicon morphemes
  come from the country's lexicon. The engine in `teigeodata-names` names no language: one
  engine serves every abugida whose profile exists.
- **Corrections:** build the guarded override layer of forge pipeline §3.7 in the forge
  (`data/countries/<cc>/corrections/<id>.yaml`, ADR-0036). The guard is a BLAKE3 of the
  canonical source value. Each
  build applies, retires or flags each correction as stale, and the build summary lists
  them.
  G-corr fails the build on a stale correction older than one release.

## 3. Findings

### 3.1 The current build's display names

Measured with DuckDB over the `read` and `conflate` outputs of `forge make np` (2026-11-09).
Each cluster's first record by source priority (OSM, GeoNames, Wikidata) gives today's
canonical name (forge `build`, D-092):

| Clusters (settlement or locality) | Devanagari canonical | of them from OSM | No Latin name in any source | Only Devanagari names |
|---:|---:|---:|---:|---:|
| 98,020 | 6,269 | 6,238 | 3,346 | 243 |

Of the 3,346 clusters with no Latin name, 3,103 are not Devanagari either. They are empty
names, bracketed notes or Chinese labels from Wikidata; they keep today's rule.

### 3.2 Test set for romanisation

**Selection.** OSM Nepal records (Geofabrik extract, pinned `geofabrik-np`) whose `name:ne` is
pure Devanagari and whose `name:en` or `name` is pure Latin letters: **6,660 pairs**, 2026-11-09.
The query is in §5.3; the set is not committed (ODbL, derived, and rebuilt from the pinned
source).

**Noise.** The Latin side is what mappers wrote, not a standard:

- the same word spelled two ways (Dada and Danda for डाँडा);
- English translations (Pratima agriculture firm);
- spaces that differ from the Devanagari (Baguwabesi for बगुवा बेसी).

Some errors cannot be avoided. The scores are a lower bound and a comparison, not an
accuracy.

**Metrics.**

- *Exact:* case-folded, spaces and hyphens ignored.
- *CER:* Levenshtein distance divided by the reference length.
- *Token exact:* over pairs with the same number of words.

### 3.3 Results

The prototype is a Python model of the profile; the Rust engine must reproduce it (§5).

| Variant | Exact | CER | Token exact |
|---|---:|---:|---:|
| ICU 79.1 `Devanagari-Latin; Latin-ASCII` | 0.220 | 0.260 | 0.289 |
| Letters only (NSD-style consonants, no diacritics) | 0.312 | 0.215 | 0.392 |
| + word-final schwa deleted, except after a conjunct | 0.466 | 0.171 | 0.567 |
| + lexicon terms as morphemes, in canonical form; final ँ → n | 0.532 | 0.161 | 0.634 |
| + medial schwa deleted left to right in V C _ C V | 0.545 | 0.155 | 0.647 |
| + aspirated geminates once, plain ones twice (ख्ख → kh, ल्ल → ll) | 0.569 | 0.152 | 0.671 |
| + ङ before क/ख/ग/घ → n | 0.581 | 0.149 | 0.682 |
| + no medial deletion before ह | **0.584** | **0.149** | **0.685** |
| (rejected) व → b initially, w after u/o, else v | 0.563 | 0.153 | 0.666 |

Split halves (odd and even rows): 0.595 and 0.574 exact. ICU: 0.218 and 0.223.

What is left, by inspection of 100 misses:

- **The reference spells it another way (about half).** Shree/Shri, Pradeep/Pradip,
  Dada/Danda, -tol/-tole, English loans.
- **Medial schwa wrongly kept or deleted (about a quarter).** Jankalyan/Janakalyan, Khokna/Khokana,
  Battisputli/Battisputali. These are compounds and Newar names that need morpheme data.
- **व as b or v (about a tenth).** Tribeni, Binayak, Devi.

All three are fixed per place by an alternate name or a correction, not by more rules.

### 3.4 As built in the Nepal packs (2026-11-09)

**Engine against the prototype.** The Rust engine on the same 6,660 pairs, with only the
lexicon's accepted terms as morphemes: **58.7% exact**, CER 0.148, token exact 0.688. It
differs from the Python model on 67 names:

- the model's ज्ञ rule fired on any j + n after schwa deletion (वैजनाथ → "Waigyath"); the
  engine writes Waijnath;
- the engine repairs ा + े to ो and writes digits;
- the model also used the 8 proposed lexicon terms.

**Display names in `forge make np` (main build, 96,847 places):**

| How | Places |
|---|---:|
| The only Latin name among the records | 94,679 |
| The majority of two or more Latin spellings | 987 |
| Romanised from Devanagari | 175 |
| The first name that fits (no Latin name, no profile: Chinese labels, bracketed notes) | 140 |
| Respelled by the lexicon (of the above) | 7,494 |

The lite build: 89,948, 416, 34 and 8, with 6,523 respelled. The build writes every place
whose display name differs from its first source's spelling to
`data/build/pack/<cc>/naming-<build>.tsv` (19,108 rows for main), for review. Two builds give
identical bytes.

**What review of that file changed.**

- About 7,200 places showed GeoNames' diacritic spellings (Lāljhoḍā). Accents are now dropped
  (§5.1 display rules), and a plain spelling wins a tie.
- Names that list two spellings (`A / B`) no longer vote.
- Invisible format characters (a ZWJ in "Tamang ‍Chowk") are removed.
- Respelling Pura → Pur and Tola → Tole changed real names. The lexicon's own evidence calls
  -pura a different morpheme, and टोला is the Maithili and Bhojpuri word for a hamlet. The
  lexicon gains `not_respelled` for such forms, which a display name keeps as written.
- Suffix respelling is limited to `generic` terms, so Bhimwada is not turned into "Bhimward";
  Wada → Ward applies only to a whole word.

The respellings that remain, counted as places whose Latin source word ends in the variant:

| Respelling | Places |
|---|---:|
| Tol → Tole | 2,348 |
| Gau or Gaon → Gaun | 1,995 |
| Dada → Danda | 1,302 |
| Chok, Chouk or Chauk → Chowk | 609 |
| Bajar or Bazaar → Bazar | 366 |

All come from accepted lexicon terms with usage evidence.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Version (checked) | Verdict |
|---|---|---|---|---|---|
| Our rule tables (profile data + one engine) | 58.4% exact, readable, deterministic, `no_std`, no dependency | Our maintenance; one profile per language | ours | – | **build** |
| ICU4C `Devanagari-Latin` | Mature, every Indic script | ISO-style: keeps schwa, ṅ → n̄ noise; 22.0% exact; C++ | Unicode-3.0 | 79.1 (2026-11-09) | test oracle only |
| ICU4X `icu_experimental::transliterate` | Pure Rust, custom rules | Unstable API; the rule language has no context-free Ohala pass | Unicode-3.0 | 0.7.0 (2026-11-09) | watch |
| `vidyut-lipi`, `shlesha` | Rust, MIT | Sanskrit/ISO schemes, no schwa deletion (same class as ICU) | MIT, MIT/Apache-2.0 | 0.2.0, 0.5.7 (2026-11-09) | no |
| Learned model (Arora et al. 2020 style) | Best on Hindi schwa | Training data, non-determinism risk, model size | – | – | no for v1; per-place alternates instead |

## 5. What we build ourselves, and why

### 5.1 The romanisation engine (`teigeodata_names::romanise`)

**Input.** A compiled profile and a name.

**Steps.**

1. NFC, then split into words.
2. Each word becomes units: a consonant with its vowel (the inherent vowel, a vowel sign, or
   none after a virama), an independent vowel, a nasal sign, or other.
3. **Morphemes:** the longest lexicon native form that is the whole word, or a suffix leaving
   at least two characters. That morpheme is written in its canonical Latin form, and the stem
   is romanised alone.
4. **Schwa:** delete the word-final inherent vowel unless a conjunct precedes it. Then go left
   to right: delete an inherent vowel that has a vowel on its left and a consonant with a vowel
   on its right, unless that consonant is in the profile's `keep_before` list (Nepali: ह).
5. **Write:**
   - consonants and vowels from the tables;
   - anusvara as n, or m before a labial;
   - chandrabindu as n word-finally after a vowel, otherwise dropped;
   - a velar nasal before a velar as n;
   - a doubled aspirate once;
   - the profile's conjunct overrides (ज्ञ → gy, क्ष → ksh).
6. Title case per word.

**Profile file** (`data/romanise/ne-Deva.yaml`, schema `teigeodata/romanise/v1`):

- the consonant, vowel and vowel-sign tables;
- virama, nukta and the nasal signs;
- conjunct overrides, labials, velars, aspirates and `keep_before`;
- golden examples, which `check-data` runs.

`xtask` compiles the profile into the forge's input. The engine is generic over abugidas; a
Hindi or Bengali profile is a new file, not new code.

**Gates:**

- the Rust engine equals the Python model on all 6,660 pairs;
- exact ≥ 0.58 on the pairs, a bench gate that only rises;
- the golden examples in the profile;
- property tests: never panics, output ASCII only, deterministic.

### 5.2 The canonical name (forge `build`)

Per place:

1. A **correction**, if one applies (§5.4).
2. **Admin units:** the boundary source's name, as today: COD-AB from the Survey Department.
3. **Local usage:** the Latin names of the cluster's OSM records (`name`, `name:en`), the
   majority after lexicon rendering, ties by source order.
4. Else GeoNames, then Wikidata's English label.
5. Else the romanisation of the first Devanagari name.
6. Else today's rule: the first name that fits.

**Lexicon rendering** applies to the winner: a generic term in a variant spelling becomes its
canonical form, keeping the compounding (Lahachok → Lahachowk). The source's own spelling
stays an alternate name. Every Devanagari name stays as a name with `lang = ne`.

**Measure:**

- the count of Devanagari canonical names (expected 0 where a Latin name exists, so 6,269 → 0
  plus the 243 romanised);
- golden search recall, which must not fall;
- names changed against the 3.1 pack, as a reviewable diff in the build summary.

### 5.3 Reproducing the test set

```sql
-- in data/build/read, after `forge make np`
with n as (select * from 'geofabrik-np/names.parquet'),
ne as (select id, min(text) ne from n where lang = 'ne'
       and regexp_matches(text, '^[\x{0900}-\x{097F} ]+$') group by id),
en as (select id, min(text) en from n where (lang = 'en' or lang = '')
       and regexp_matches(text, '^[A-Za-z][A-Za-z .''-]*$') group by id)
select ne.id, ne.ne, en.en from ne join en using (id) order by ne.id;
```

### 5.4 Corrections

The forge pipeline §3.7 schema, with these choices for v1:

- **Operations:** `set` and `unset` (a name, the point, the population), `same-as` and
  `not-same-as` (fed to the clusterer as constraints), and `retire`.
- **Guard:** `source`, `record` and `field`, plus `value_hash`, the BLAKE3 of the canonical
  value's text form. A guard with no record targets a stable place ID and is checked against
  the ID registry.
- **Outcomes per build:**
  - *apply* when the guard holds;
  - *retire* when the source now equals the correction;
  - *stale* when the source changed otherwise;
  - a redirect is followed and a tombstone is stale.

  The summary lists every outcome. A correction joins the stage key, so editing one rebuilds.
- **Lite build:** a correction whose evidence licence class is not lite-clean is ignored there.
- **G-corr:** the build fails on any stale correction whose `created` is older than the
  previous release.

## 6. Impact on the plan

- **Phase 3, 3.6:**
  - romaniser and profile (built, D-106);
  - canonical names by §5.2 (built, D-106);
  - corrections and G-corr;
  - D-060's corroboration and collision rules inside `build`, which today carries every
    member's names.
- **Decision to record:** the romaniser design, its gate and the canonical name order (§5.2).
- **Schema:** `teigeodata/romanise/v1`, and the corrections file.
- **`check-data`:** profiles and corrections are validated; the golden examples run.

## 7. Risks and open items

- The Latin references are OSM mappers' spellings; 58% exact is against them, not against a
  standard. The labelled sample (Q-017) will measure display names against people.
- व is left as w. A curated list per lexicon (Biratnagar, Tribeni) can move single words to b.
- Newar (Nepal Bhasa) names written in Devanagari follow other schwa rules. A per-place
  alternate or a correction fixes them; a `new-Deva` profile is possible later.
- Hindi uses the same engine but with v for व and other conjunct habits: a separate `hi-Deva`
  profile, measured on India pairs when India starts.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| ICU4C (`uconv`) | 79.1 | 2026-11-09 | local Homebrew `icu4c@79` |
| icu_experimental | 0.7.0 (updated 2026-10-22) | 2026-11-09 | crates.io API |
| vidyut-lipi | 0.2.0 (2025-01-08) | 2026-11-09 | crates.io API |
| shlesha | 0.5.7 (2026-07-06) | 2026-11-09 | crates.io API |
| DuckDB CLI | local Homebrew | 2026-11-09 | `/opt/homebrew/bin/duckdb` |

## 9. Sources

- [Names, romanisation and search](names-and-search.md), this repository: the policy, the
  profile and the linguistic sources (Ohala; Choudhury and Basu 2002; Narasimhan, Sproat and
  Kiraz 2004; Arora, Gessler and Schneider 2020; the NSD system).
- [Forge pipeline §3.7](forge-pipeline.md#37-corrections-as-data-and-qa-queues), this
  repository: corrections and QA queues, and how WOF, Overture and OSM handle edits.
- OpenStreetMap contributors, the Geofabrik Nepal extract pinned as `geofabrik-np` in
  `data/sources.lock` (ODbL 1.0): the 6,660 test pairs.
- [ICU transforms](https://unicode-org.github.io/icu/userguide/transforms/general/), Unicode
  ICU user guide, accessed 2026-11-09.
- [crates.io](https://crates.io/) API for the crate versions, accessed 2026-11-09.
