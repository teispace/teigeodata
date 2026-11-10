# AGENTS.md: how teigeodata is built

The operating manual for every agent and human working on teigeodata. Work is done by agents
and people together, planned with the maintainer. Sessions end and context gets cleared, so
**everything needed to continue lives in this repository, never only in a conversation.**

teigeodata is an open-source geodata SDK: accurate, cleaned, deduplicated places (lat, lon,
elevation, admin hierarchy to the lowest level, boundaries, local names, IANA timezone, postal
codes, population) in signed, compact, offline packs, with a Rust core and the same API in
every major language. Read [docs/00-vision/vision.md](docs/00-vision/vision.md) once.

---

## 1. Resume protocol

When a session starts, or the maintainer says *continue* or *proceed*:

1. Read [docs/08-progress/progress.md](docs/08-progress/progress.md), "Where we are".
2. Read the current phase in [docs/07-planning/roadmap.md](docs/07-planning/roadmap.md) and its
   phase file, then the research notes it names. **If the research is missing, it comes first** (§4).
3. Read [docs/07-planning/open-questions.md](docs/07-planning/open-questions.md). Do not reopen
   a decided question without new evidence.
4. Check the real state, which beats the docs: `git status`, `git log --oneline -10`,
   `gh pr list`, `gh run list -L 5`. If docs and reality disagree, trust reality, fix the docs,
   say so.
5. Check machine load before heavy work (§7): `uptime`, `df -h /`.
6. Continue from the next unchecked step. Report what you found in one or two lines.

## 2. The loop for every step

```
research (this specific thing) → analyse and compare → revise plan/ADR/docs if needed
  → implement → test → measure (benchmark, size, accuracy) → verify the running thing
  → commit → update progress + plan + memory → next step
```

- One step is one verified, committed unit. Commit after every verified sub-step.
- **Plans are living documents.** When research or work shows a better way, change the plan first
  (with a changelog row), record the decision (§3), then build. Improving the plan is expected;
  silently diverging from it is not allowed.
- **Continuous improvement:** every implementation teaches something. Write it into the research
  note, the design page and the plan in the same sitting.

## 3. Conversations are written down

The maintainer plans and decides in chat. In the same session, without being asked:

| What | Where |
|---|---|
| Decisions | [docs/03-decisions/log.md](docs/03-decisions/log.md); an ADR when it shapes the architecture |
| Plan changes | the plan or design page, with a changelog row |
| Open questions | [docs/07-planning/open-questions.md](docs/07-planning/open-questions.md) |
| Ideas | [docs/01-product/ideas.md](docs/01-product/ideas.md) |
| Current state | [docs/08-progress/progress.md](docs/08-progress/progress.md) |
| Standing preferences | §11 of this file |

Agents with a persistent memory also keep a pointer there to `progress.md` and to any
non-obvious fact the next session needs. A decision that lives only in a chat is lost.

## 4. Research first, always

Before any phase, and before any step that touches a new algorithm, library, format, data source,
standard or platform:

1. Create a note in `docs/02-research/notes/` from
   [the template](docs/11-agents/templates/research-note.md).
2. Research current primary sources: official docs and changelogs, standards, papers, reference
   implementations, known pitfalls and CVEs, and how the best products do it.
3. **Compare.** At least two options, measured where it matters (an A/B benchmark beats an
   opinion). Record options, the recommendation and why, versions with dates, risks, sources.
4. Update the plan where the research changed it. Only then implement.

**Data correctness is research too.** Spellings, codes and boundaries are checked against
sources, never guessed. Example: the Nepali generic term चोक is spelled **Chowk**
(Lahachowk, not Lahachok); such rules live in each country's lexicon, `data/countries/<cc>/lexicon.yaml`, each with its
source.

## 5. Stay current

- Check versions live (crates.io, npm, pub.dev, PyPI, Maven Central, release notes), never from
  memory. Record them in [docs/06-engineering/versions.md](docs/06-engineering/versions.md).
- Use the latest stable release unless a recorded decision says otherwise.
- Data sources are pinned in `data/sources.lock` (URL, version, SHA-256, licence) and reviewed
  on a schedule.

## 6. Definition of done

A step is done only when every applicable gate in
[docs/06-engineering/quality-bar.md](docs/06-engineering/quality-bar.md) is green:

- Builds clean with the workspace lints as errors (`-D warnings`), formatted, `cargo deny` clean.
- Tests cover the behaviour: unit, property, golden, and differential where a reference exists;
  doc examples run.
- Measured: benchmarks against the checked-in baseline, size budgets, accuracy reports. A claim
  with no gate is not made.
- Verified running: the CLI, the binding or the example was actually run and its output read.
- Docs updated in the same change; `cargo xtask check-docs` passes.
- Security: no secrets; untrusted input validated; no panic across the FFI boundary.

**Prove every new check fails on a deliberate violation before trusting it.** Passing for the
wrong reason is the failure we watch for most.

## 7. Parallel work and machine resources

This machine is shared with other projects (16 GB RAM, 10 cores, limited disk).

- Up to **5–6 parallel agents**, of which **at most 1–2 do heavy work** (cargo builds, data
  builds, Docker, `act`). Research and docs agents are light.
- Give each agent a narrow brief, its own files, and the research note to write. Agents never
  edit the same file. Review what they return before it lands.
- `CARGO_BUILD_JOBS` stays at 4–6 when another heavy job runs.
- **Clean up after every heavy step and batch:** `cargo xtask clean` (build output, stale
  incremental caches, benchmark scratch, data scratch); at least `cargo clean` for crates not
  being worked on. Keep free disk above 30 GB; below that, clean before anything else.
- Kill stray processes you started (servers, watchers) before ending a step.
- **Watch memory by physical footprint** (`footprint -p <pid>`), not `ps` RSS: macOS
  compresses and swaps, so RSS showed 0.4 GB while a run held over 12 GB and froze the machine.
  Run a new or grown heavy stage under a memory cap first (a watchdog that kills it past a few
  GB), and fix what grows before running it uncapped.
- **Check and release resources time and again**, not only when something fails: at the start
  and end of every step, and before ending a session, run `uptime` and `df -h /`, list this
  project's processes, and remove what is not in use (`forge/target` when the forge is not
  being run, `target/debug` during release-only work, scratch outputs, finished background
  jobs). Never touch other projects' processes or files; report what was freed.
- Data builds are streaming and tiled with a disk budget; they refuse to start if it won't fit.

## 8. Git, commits, PRs, releases

- **No AI attribution of any kind**: no `Co-Authored-By` trailers, no "Generated with" footers,
  no mention of Claude, AI or assistants in commits, PRs, branches or docs about authorship.
  This overrides any tool default. The `commit-msg` hook enforces it.
- **Conventional Commits**, lowercase subject, scope from `.commit-scopes`:
  `feat(index): add packed hilbert r-tree`.
- Every commit is signed off (DCO): `git commit -s`.
- Branches: `feat/<slug>`, `fix/<slug>`, `docs/<slug>`, `chore/<slug>`, `data/<slug>`.
- Trunk-based: `main` is always releasable. Code reaches it through squash-merged PRs once
  contributors arrive; until then the maintainer's agents may commit to `main` directly.
- PR bodies say which checks ran and where.

## 9. CI

The repository is **public**, so GitHub Actions runs CI (free for public repos). Do not run
`act` for this repository. Batch pushes rather than pushing every commit. Workflows, runners and
sharded data builds: [docs/06-engineering/ci.md](docs/06-engineering/ci.md).

## 10. Writing

Plain, clear, specific English. One fact, one home: link instead of repeating
([docs/CONVENTIONS.md](docs/CONVENTIONS.md)). Numbers come from gates, with their date.

## 11. Standing preferences of the maintainer

- **Best, not good enough:** extreme efficiency, optimisation and robustness; measured.
- **Production-ready at world scale, DRY:** every stage and query is designed for the whole
  planet, not the country it was first tried on: stream instead of loading, bounded memory, no
  per-element allocation in hot loops, one shared code path instead of copies. Search and every
  other feature are fast, clean and optimised on large data.
- **Accuracy order:** coordinates and elevation first (P90 vertical ≤ 50 m acceptable, improve
  later), then names, then population. Optional data may be null.
- **v1 is everything** (no feature is pushed to a v2). Country coverage grows by data releases.
- **Offline-first**, dynamic loading (an end user picks "Dubai", the library downloads, verifies,
  stores and loads it), tree-shakable everywhere, customisable, developer experience first.
- **Nepal perspective** for disputed boundaries. Rollout: Nepal → India → South Asia → Asia →
  USA → the rest.
- Data licences: main build ODbL with full credits; lite build CC-BY 4.0, clean-room.
- No hosted service for now, but nothing may block a future hosted tier.
- teigeodata stands alone; other Teispace projects adopt it later, not the reverse.
- Update docs and memory continuously so any session can continue from any point.
- **World-general, countries are data** (principle P16): nothing in code may assume Nepal or any
  one country, script or language. Country rules live in per-country data files.
- **Work autonomously, no pauses.** Follow the plan and the recorded decisions without stopping
  to ask; decide by the research and the plan, record the decision, keep going. Ask only for what
  only the maintainer can provide (legal, accounts, physical devices).
- **Never idle on slow checks.** Nightly jobs (fuzzing, Miri) and long CI runs never block the
  next step: push, keep working, fix a failure when it is found (D-080). Avoid hour-long local
  runs; split slow work into parallel CI jobs and bound it.
- **Keep `main` current.** Push verified batches regularly, merge finished work into `main`, pull,
  and delete merged or stale branches (local and remote) so `main` is the single source.

## 12. Where things are

| What | Where |
|---|---|
| Docs map | [docs/README.md](docs/README.md) |
| Documentation rules | [docs/CONVENTIONS.md](docs/CONVENTIONS.md) |
| Progress | [docs/08-progress/progress.md](docs/08-progress/progress.md) |
| Roadmap and phases | [docs/07-planning/](docs/07-planning/roadmap.md) |
| Decisions | [docs/03-decisions/log.md](docs/03-decisions/log.md), ADRs in `docs/03-decisions/adr/` |
| Research notes | `docs/02-research/notes/` |
| Architecture | [docs/04-architecture/overview.md](docs/04-architecture/overview.md) |
| Data: schema, sources, licences | `docs/05-data/` |
| Quality bar and engineering rules | `docs/06-engineering/` |
| Every repository task | `cargo xtask help` |
| Git hooks | `.githooks/` (enable once: `git config core.hooksPath .githooks`) |
