---
title: "Documentation conventions"
status: accepted
updated: 2026-11-06
---

# Documentation conventions

The rules every document here follows, so that any person or agent can find a fact, trust it,
and update it without breaking anything. `cargo xtask check-docs` enforces the checkable rules
(§8); review enforces the rest.

## 1. One fact, one home

Every fact lives in exactly one document. Everywhere else links to it. If you find a fact in two
places, keep it in its home folder (§2), replace the copy with a link, and add a changelog row.
Conversations are not a home ([AGENTS.md](../AGENTS.md) §3).

## 2. Where things go

| Folder | Holds | Typical status |
|---|---|---|
| `00-vision/` | Why and for whom: vision, principles, scope | accepted |
| `01-product/` | What we build: the feature catalogue, the API surface, ideas | living |
| `02-research/` | What we learned: research notes, benchmarks, comparisons | research |
| `03-decisions/` | What we decided: the decision log and ADRs | living, accepted |
| `04-architecture/` | How it is built: crates, format, engine, indexes, bindings, security | draft, accepted |
| `05-data/` | The data: schema, sources, licences, perspectives, names policy, per-country pages | draft, living |
| `06-engineering/` | How we work: quality bar, testing, CI, release, versions, resources | accepted, living |
| `07-planning/` | When: roadmap, milestones, phases, open questions | living |
| `08-progress/` | Where we are: progress and the session log | living |
| `09-guidelines/` | How to contribute a specific thing: a country, a source, a binding, a correction | accepted |
| `11-agents/` | Templates and briefs for agents | accepted |

The site (`site/`) renders user-facing docs. Its pages link here for design and decisions
instead of copying them.

## 3. Frontmatter

Every page starts with:

```yaml
---
title: "Plain title"
status: research | draft | proposed | accepted | living | superseded
updated: YYYY-MM-DD
---
```

Research notes also carry `owner` and `revisit`. A superseded page says what replaces it.

## 4. Statuses

- `research`: findings, not yet a decision.
- `draft`: being written; may change freely.
- `proposed`: complete, waiting for the maintainer.
- `accepted`: decided; changes need a decision-log row.
- `living`: kept current by design (logs, progress, registers).
- `superseded`: kept for history; links to its replacement.

## 5. Decisions

Small decisions are a row in [the log](03-decisions/log.md) (`D-NNN`). A decision that shapes
the architecture, the format, the API, the data licence or the process also gets an ADR
(`adr/ADR-NNNN-slug.md`: context, decision, consequences, alternatives, evidence). ADRs are
never edited after acceptance except their status line; a new ADR supersedes an old one.

## 6. Numbers

A number in prose (a size, a speed, an accuracy) names its gate or measurement and its date.
Generated pages (`ACCURACY.md`, `SIZES.md`, `BENCHMARKS.md`, `CONFORMANCE.md`) are written by
`cargo xtask` and are never hand-edited.

## 7. Style

Plain, specific English; short sentences; tables for comparisons; no marketing words; say what
is unverified. Code identifiers in backticks. Dates as `YYYY-MM-DD`.

## 8. Checked by `cargo xtask check-docs`

- Every Markdown file under `docs/` has valid frontmatter with a known status and a date.
- Every relative link resolves to an existing file (and anchor, where checked).
- Research notes have the template's sections.
- ADR file names follow `ADR-NNNN-slug.md` and are listed in the decision log.

## 9. Changelog rows

Living plans end with a `## Changelog` table: `| date | change | why |`.
