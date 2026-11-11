---
title: "Zones before 1970 (backzone) for South Asia"
status: research
updated: 2026-11-11
owner: research
revisit: 2027-05-01
---

# Zones before 1970 (backzone) for South Asia

## 1. Question

Consumer request TG-08 ([Tithipatra reply](../../01-product/consumers/tithipatra.md)): tzdb's
default build links zones that agree since 1970 to one another, so their older histories are
lost. The `backzone` file (built with `PACKRATDATA` and `PACKRATLIST`) keeps more. For South
Asia:

- What does tzdb 2026f say before 1970 for `Asia/Kolkata`, `Asia/Kathmandu`, `Asia/Dhaka`,
  `Asia/Karachi`, `Asia/Colombo` and `Asia/Thimphu`, and how reliable is it by tzdb's own
  comments?
- Which zones of the region exist only in `backzone`?
- Does any boundary source give outlines for pre-1970 zones (for example Bombay Time)?
- What would a pack built with `backzone` change, what can we ship honestly, and how is it gated?

It feeds phase 4 §4.9b (TG-08) and the TZif bundle of TG-03 under
[ADR-0033](../../03-decisions/adr/ADR-0033-timezone-data.md).

## 2. Summary and recommendation

- **`backzone` holds no South Asian zone.** Its 123 `Zone` and 13 `Link` lines (2026f) include
  none for India, Nepal, Bangladesh, Pakistan, Sri Lanka, Bhutan, the Maldives or Afghanistan.
  The only India entry is a commented-out placeholder, `#Zone Asia/Chennai [not enough info to
  complete]`. All six zones above are full `Zone` entries in the main `asia` file, so the
  default build already carries every pre-1970 transition tzdb knows for them.
- **So a backzone build changes no South Asian instant.** The six zones' zic input is the same
  with or without `backzone`. Their TZif output should therefore be byte-identical. This follows
  from the source and is **not yet measured** (gate G-tz-pack below measures it).
- **No Bombay or Calcutta zone exists in tzdb**, in either file. tzdb deliberately uses railway
  time for `Asia/Kolkata` across all of India: Madras time (+05:21:10) from 1870, IST (+05:30)
  from 1906-01-01. Bombay civil time (about +04:51, used until 1955 per secondary sources) and
  Calcutta civil time (+05:53:20 or +05:54) are described in comments only.
- **No outline source exists** for where Bombay Time, Calcutta Time or Madras Time applied.
  timezone-boundary-builder (TBB) draws only current `zone.tab`-level zones. Its issue #171 on
  pre-1970 zones is open with no plan. We found no other dataset.
- **Recommendation.** Build the TZif bundle with `PACKRATDATA=backzone PACKRATLIST=zone.tab`
  as the one bundle of every pack (as
  [timezone and elevation](timezone-and-elevation.md) already proposed). It is what gives TBB's
  own IDs (`Europe/Amsterdam`, `Asia/Kuala_Lumpur`) their own history. It leaves every instant
  since 1970 unchanged, and South Asia unchanged entirely. Record it in the manifest as two
  fields, `tzdb = "2026f"` and `tzdb_packrat = "backzone:zone.tab"` (display form
  `2026f+backzone(zone.tab)`). Do not ship two bundles.
- **Gate for historical sub-zones.** An instant reads a historical sub-zone offset (for example
  Bombay Time in Mumbai before 1955) only if both exist: a tzdb `Zone` for it (main or
  `backzone`) and a pinned outline source for its area in `data/sources.lock`. For South Asia
  neither exists today. A pre-1955 Mumbai instant therefore reads `Asia/Kolkata`'s +05:30 (IST,
  railway time), and the answer says it is before tzdb's 1970 scope.

## 3. Findings

### 3.1 tzdb's scope and the `backzone` mechanism

- tzdb aims to be correct from 1970 on. `backzone`'s header (Paul Eggert, 2014-10-31) says its
  zones "are less reliable, cover only a tiny sliver of the pre-1970 era", and that less effort
  goes into maintaining it. Unless stated otherwise its source through 1990 is Shanks and
  Pottenger, *The International Atlas* (2003), which, the header says, "contains many errors and
  cites no sources".
- Release 2022b finished moving duplicate-since-1970 zones into `backzone` and added
  `PACKRATLIST`. Per NEWS, `make PACKRATDATA=backzone PACKRATLIST=zone.tab` produces TZif files
  identical to the global-tz project's. The 2026f Makefile default is `PACKRATDATA=` (empty), so
  a plain build drops `backzone`.
- 2026f `backzone` counts (2026-11-11): 123 zones, 95 of them named in `zone.tab`. The `Asia/`
  ones in `zone.tab` are Aden, Bahrain, Brunei, Kuala_Lumpur, Kuwait, Muscat, Phnom_Penh and
  Vientiane. None is in South Asia.
- TBB's comprehensive release (444 IDs with oceans) uses `zone.tab` IDs and includes, for
  example, `Europe/Amsterdam`, `Asia/Kuala_Lumpur` and `Asia/Muscat`. Without `backzone`, those
  IDs resolve through `backward` links and read another zone's pre-1970 history.

### 3.2 What tzdb 2026f says for South Asia

All entries are in the `asia` file. "Shanks" means tzdb's default source through 1990 (above).

| Zone | Before 1970, as compiled | Source and reliability (tzdb comments) |
|---|---|---|
| `Asia/Kolkata` | LMT +05:53:28 until 1854-06-28; HMT +05:53:20 until 1870; MMT (Madras railway time) +05:21:10 until 1906-01-01; IST +05:30; +06:30 (counted as DST) 1941 Oct to 1942-05-15; +05:30 to 1942 Sep; +06:30 to 1945-10-15; IST since | Railway time chosen on purpose: "better documented" and consistent across today's `Asia/Kolkata` area after 1906 (Indian Year Book 1936-37). Calcutta government time was +05:53:20 (Shanks) or +05:54 (Indian Year Book). The 1941-45 war time: "Shanks is our only (and dubious) source". Changed in 2017b to railway time |
| `Asia/Kathmandu` | LMT +05:41:16 until 1920; +05:30 until 1986; +05:45 since | No zone-specific comment, so Shanks by default |
| `Asia/Dhaka` | LMT +06:01:40 until 1890; HMT +05:53:20 until 1941 Oct; +06:30 to 1942-05-15; +05:30 to 1942 Sep; +06:30 to 1951-09-30; +06:00 since | Shanks by default; "Howrah Mean Time?" is marked as a guess |
| `Asia/Karachi` | LMT +04:28:12 until 1907; +05:30 to 1942 Sep; +06:30 to 1945-10-15; +05:30 to 1951-09-30; +05:00 since | Shanks by default. Unlike `Asia/Kolkata`, it has no +06:30 from 1941 Oct to 1942 May; tzdb does not comment on the difference |
| `Asia/Colombo` | LMT +05:19:24 until 1880; MMT (Moratuwa) +05:19:32 until 1906; +05:30; +06:00 from 1942-01-05; +07:00 from 1942 Sep to 1945-10-16 02:00; +05:30 since | Shanks. Milne's "Madras mean time from 1898" and "Colombo mean time 5h 4m 21.9s" are noted and set aside as inconsistent with Colombo's meridian |
| `Asia/Thimphu` | LMT +05:58:36 until 1947-08-15; +05:30 until 1987 Oct; +06:00 since | Shanks by default |

Comments with no data behind them:

- India had two civil zones from 1884, Bombay and Calcutta, and railways used Madras time;
  Bombay briefly switched to Madras time in 1881 (The Hindu, 2014, quoted in `backzone`).
- Madras civil time was 5:20:57.3 (US Navy report, 1876, quoted in `backzone`).
- The 1906 Bombay mill riots against IST; the "Battle of the Clocks" lasted "nearly half a
  century" (The Atlantic, 2015, quoted in `asia`).

### 3.3 Bombay and Calcutta time outside tzdb

- Wikipedia's "Bombay Time" page gives +04:51, official from 1884 and "maintained until 1955",
  with no day. It cites a 2001 local web page, an archived GMT site and a 2010 Economic Times
  article. It gives no area: city or Presidency is not stated. Secondary sources only; we
  found no gazette or legal instrument. **Unverified.**
- Wikipedia's "Time in India" says Calcutta Time was kept until 1948. Also secondary,
  **unverified**.
- We found no source for the *area* where Bombay, Calcutta or Madras time applied, nor any
  date by which each town changed.

### 3.4 Outline sources

| Source | What it has before 1970 | South Asia |
|---|---|---|
| TBB 2026f comprehensive (`timezones-with-oceans`) | One polygon per current `zone.tab`-level ID; history comes only from the tzdb rules for that ID | Six zones, current borders |
| TBB 2026f `-1970` (301 groups) and `-now` (62 groups) | Merged groups; wrong before 1970 and before "now" respectively ([timezone and elevation](timezone-and-elevation.md)) | `-1970` keeps all six separate; `-now` merges `Asia/Colombo` into `Asia/Kolkata` and `Asia/Thimphu` into `Asia/Dhaka` |
| TBB issue #171 "pre-1970 time zones" | Proposal of up to 2,500 extra zones; maintainer interested but needs help; a commenter says such zones are too contested for OSM and points to OpenHistoricalMap or Wikidata | Open since 2024-02-28, last comment 2026-01-22, no milestone |
| Any other dataset | None found | None |

TBB's comprehensive variant is not "backzone-aware" in any special way. It already keeps the
`zone.tab` IDs that `backzone` restores, so `PACKRATLIST=zone.tab` matches it ID for ID.

### 3.5 What a backzone build changes

- **Changes:** the pre-1970 history of the 95 `zone.tab` zones that are links in the default
  build (for example `Europe/Amsterdam`, `Asia/Kuala_Lumpur`, `Asia/Muscat`), when looked up
  by their TBB ID.
- **Does not change:** any instant from 1970-01-01 on, for any ID (tzdb's rule for what goes
  into `backzone`); any instant at all for the six South Asian zones.
- **Full `backzone` without `PACKRATLIST`** also gives the 28 zones not in `zone.tab` (for
  example `Asia/Chongqing`, `Asia/Harbin`, `Asia/Kashgar`) their own data. No outline exists
  for them, so a point lookup never returns them; only a lookup by name would. We leave them out.

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| A. Default build (no `backzone`) | tzdb's supported scope only | TBB IDs such as `Europe/Amsterdam` read another zone's pre-1970 history | Public domain | tzdb 2026f | Reject |
| B. `PACKRATDATA=backzone PACKRATLIST=zone.tab` | Each TBB ID reads its own history; equals global-tz; no change since 1970 | `backzone` is less reliable and less maintained | Public domain | tzdb 2026f, option since 2022b | **Adopt as the one bundle** |
| C. Full `backzone` | Also covers 28 non-`zone.tab` zones | Those zones have no outline; more bytes for names nobody reaches by point | Public domain | tzdb 2026f | Reject |
| D. Our own sub-zones (Bombay, Calcutta) | Would answer TG-08's example | No rules source, no outline source: we would be inventing data | — | — | Reject (forbidden by §4 of AGENTS.md) |

## 5. What we build ourselves, and why

Nothing new in data. Three small things:

1. **Bundle flavour.** The TG-03 forge step runs zic over `asia … backzone` with
   `PACKRATLIST=zone.tab` semantics and writes `tzdb_packrat` to the manifest.
2. **Scope flag.** An offset answer for an instant before 1970-01-01 UTC carries a
   `before_tzdb_scope` flag (name to be fixed in TG-03), so a consumer can tell "tzdb's best
   guess" from "supported".
3. **Sub-zone gate.** A historical sub-zone is added only as data: a tzdb `Zone` (main or
   `backzone`) plus an outline pinned in `data/sources.lock`. The forge refuses a sub-zone with
   only one of the two. Today no South Asian sub-zone passes.

Measurements that must prove it (gate G-tz-pack, to be added with TG-03):

- For every one of TBB's 444 IDs, the backzone bundle and a default bundle give the same offset
  and abbreviation at every transition from 1970-01-01 on. A planted post-1970 edit to one
  `backzone` zone must fail it.
- The six South Asian zones' TZif files are byte-identical between the two builds.
- The bundle's size, both ways, is recorded in `SIZES.md` (about 200 KB per ADR-0033; the
  backzone delta is **not measured**).

## 6. Impact on the plan

- TG-03 (TZif in the pack): adopt option B and the manifest field `tzdb_packrat`. Needs a
  decision-log row; ADR-0033 already says "our own TZif bundle", so no new ADR.
- TG-08: answered by this note. No data work for South Asia. Reply row: "backzone has no South
  Asian zone; the bundle is built with it anyway; pre-1970 answers are flagged; Bombay Time
  needs a rules source and an outline source, neither of which exists".
- Data-source review: watch TBB issue #171 and `backzone`'s India entry (`Asia/Chennai`) at each
  tzdb release.

## 7. Risks and open items

- **Unknown, and documented as unknown:** the area and end date of Bombay Time, Calcutta Time
  and Madras civil time; whether princely states followed IST after 1906; the exact 1941-45
  war-time dates in India (tzdb calls its only source dubious); why `Asia/Karachi` lacks the
  1941-42 +06:30 that `Asia/Kolkata` has; Nepal's switch dates of 1920 and 1986 (Shanks only).
- Before 1906, tzdb's `Asia/Kolkata` gives Madras railway time everywhere in India, which is not
  the civil time of Bombay or Calcutta. Consumers computing historical local solar or civil time
  (Tithipatra) need to know this; the scope flag and this note say it.
- A future tzdb release could add a South Asian zone to `backzone`; the per-release review
  catches it.
- Byte-identity of the six zones and the bundle's size delta are inferred, not measured.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| tzdb | 2026f (released 2026-10-27) | 2026-11-11 | data.iana.org `tzdb/version`; `asia`, `backzone`, `backward`, `zone.tab`, `zone1970.tab`, `NEWS`, `Makefile` at tag 2026f |
| timezone-boundary-builder | 2026f (published 2026-11-01) | 2026-11-11 | GitHub releases API; `timezone-names-with-oceans*.json`; `timezones.json`; README |
| TBB issue #171 | open | 2026-11-11 | GitHub |

## 9. Sources

- [tzdb 2026f `backzone`](https://github.com/eggert/tz/blob/2026f/backzone), IANA tz project, accessed 2026-11-11.
- [tzdb 2026f `asia`](https://github.com/eggert/tz/blob/2026f/asia), IANA tz project, accessed 2026-11-11.
- [tzdb 2026f `NEWS`](https://github.com/eggert/tz/blob/2026f/NEWS) (2017b railway time; 2022b `PACKRATLIST`), accessed 2026-11-11.
- [tzdb 2026f `Makefile`](https://github.com/eggert/tz/blob/2026f/Makefile) (`PACKRATDATA`, `PACKRATLIST` defaults), accessed 2026-11-11.
- [tzdb release directory](https://data.iana.org/time-zones/releases/), IANA, accessed 2026-11-11.
- [timezone-boundary-builder README](https://github.com/evansiroky/timezone-boundary-builder/blob/master/README.md) and [2026f release](https://github.com/evansiroky/timezone-boundary-builder/releases/tag/2026f), Evan Siroky, accessed 2026-11-11.
- [timezone-boundary-builder issue #171, "pre-1970 time zones"](https://github.com/evansiroky/timezone-boundary-builder/issues/171), opened 2024-02-28, accessed 2026-11-11.
- [Bombay Time](https://en.wikipedia.org/wiki/Bombay_Time) and [Time in India](https://en.wikipedia.org/wiki/Time_in_India), Wikipedia, accessed 2026-11-11 (secondary).
