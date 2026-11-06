---
title: "Commits and branches"
status: accepted
updated: 2026-11-06
---

# Commits and branches

- **Conventional Commits**, lowercase subject, imperative mood, at most 72 characters:
  `feat(index): add packed hilbert r-tree`.
- Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`, `data`, `revert`.
- Scopes come from [`.commit-scopes`](../../.commit-scopes); add a scope there in the same commit
  that first uses it.
- The body says why, and for `perf`, the measurement before and after.
- Every commit is signed off (`git commit -s`), certifying the [DCO](../../DCO).
- No AI or tool attribution of any kind ([AGENTS.md](../../AGENTS.md) §8).
- Branches: `feat/<slug>`, `fix/<slug>`, `perf/<slug>`, `docs/<slug>`, `chore/<slug>`, `data/<slug>`.
- The hooks in `.githooks/` check all of the above; enable them once with
  `git config core.hooksPath .githooks`.
