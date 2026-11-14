---
title: "The MCP conformance suite in CI"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2026-12-15
---

# The MCP conformance suite in CI

## 1. Question

Phase 6.3's last item: run the official MCP conformance suite in CI against the server, for
2026-07-28 and 2025-11-25 clients. Which release of the suite, which scenarios apply to a
product server (not an SDK's reference fixture), and how is a regression kept from passing
silently?

## 2. Summary and recommendation

- **The referee:** `@modelcontextprotocol/conformance` **0.2.0-alpha.12** (npm `alpha` tag,
  2026-10-01; `latest` is 0.1.16 of 2026-03-27, which predates 2026-07-28). It is what rmcp's
  own conformance workflow pins. MIT. Run with `npx -y` at the pinned version in CI only; it
  is never a dependency of the repository.
- **The target:** `tgd serve`'s `/mcp` (stateless Streamable HTTP, D-316) over a fixture pack.
  The suite's server mode speaks HTTP only; stdio is the same `Mcp` and keeps its own wire
  tests (`tests/mcp.rs`, `tests/store.rs`, the CLI's `installs_from_mcp_*`).
- **Both revisions, every scenario:** `server --suite all --spec-version 2025-11-25` and again
  with `2026-07-28` (a scenario under both must run under each wire, as the suite's
  requirement sets say).
- **A baseline of expected failures** per revision, in `tools/mcp-conformance/`, lists the
  scenarios that need the SDK fixture's tools, prompts and resources (`test_simple_text`,
  `test://static-text`, prompts, completions, logging): a product server has none of them, by
  design. The suite exits non-zero on a failure not listed **and** on a listed scenario that
  passes, so the baseline cannot go stale and a regression cannot hide. Every entry carries
  its reason.
- The scenarios that do apply (lifecycle, `server-stateless`, `tools-list`, `resources-list`,
  `caching`, `dns-rebinding-protection`, the MRTR state checks that a server without
  fixture tools can meet, and the wire-schema check on every message) must pass.

## 3. Findings

**The suite (github.com/modelcontextprotocol/conformance, README and `requirements/`,
accessed 2026-11-15).** `server --url` connects as a client, runs scenarios and writes
`checks.json` per scenario; `--spec-version` picks the wire (2025-11-25 stateful with
`initialize`, 2026-07-28 stateless with per-request `_meta`); `--expected-failures <yaml>`
takes `server:` and `client:` lists; `-o` sets the results directory. Every scenario also
validates each message the server sent against the revision's JSON schema
(`wire-schema-valid`). `requirements/2026-07-28.yaml` (frozen, anchored at 0.2.0-alpha.10)
lists 37 server scenarios, most of them against the fixture's named tools, resources and
prompts.

**rmcp** (`.github/workflows/conformance.yml`, accessed 2026-11-15) pins 0.2.0-alpha.12, runs
both revisions with `--suite all` against its own fixture server, and runs the tasks
extension scenarios as informational.

**Applicability.** `tools.ts`, `resources.ts`, `prompts.ts`, `stateless.ts` call fixture names
(`test_simple_text`, `test://static-text`, `test_missing_capability`, ...); where the name is
absent a check fails or reports that it could not be exercised. Which checks fail for us is
read from the first run, not guessed (§7).

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Add the fixture's tools to `tgd` behind a flag | Every scenario runs | Test code in the shipped binary; tools no user wants | No |
| A separate fixture binary over `teigeodata_server::mcp` | Scores the SDK-level behaviour | Tests rmcp more than us; a second server to keep | No |
| **`tgd serve` as shipped, baseline of fixture-only scenarios** | Tests what users run; stale entries fail | Fixture scenarios are not scored | **Yes** |
| `--requirements` sets | The frozen contract | Same fixture dependence; hides scenarios added since | Later, for the report |

## 5. What we build ourselves, and why

The workflow `mcp-conformance.yml` (on pushes touching the server, the CLI or the lockfile):
build `tgd`, serve `bindings/fixtures/synth.tgd` on 127.0.0.1, run both revisions with their
baselines, gate every failing or warning check against `<revision>.checks`, upload the
results.

## 6. Impact on the plan

Phase 6.3's conformance item is built as above; the scenario lists go into the baseline files
with reasons. Decision D-319.

## 7. Risks and open items

- The suite is an alpha; a new pin may add scenarios. The baseline fails on a newly passing
  listed scenario, and a new failing scenario fails the job: either way a bump is a reviewed
  change.
- First run (2026-11-15): 37 of 61 checks passed for 2025-11-25 and 90 of 126 for
  2026-07-28. Beyond the fixture's scenarios it found three defects, fixed in the same
  change: the server's own origin refused on loopback (`dns-rebinding-protection`),
  `prompts/list` and `completion/complete` answered without being declared
  (`server-stateless`), and no `data.uri` on a missing resource (`sep-2164`). The 2025-11-25
  session scenarios warn by design (stateless, D-316); `caching` calls `prompts/list`
  whatever the server declares, so that one check is listed.
- The gate's own proof: the first run failed on the unlisted failures, the second on three
  checks missing from the list (the ones the fixes moved), each as the diff showed.

## 8. Versions checked

| Item | Version | Date | Where |
|---|---|---|---|
| @modelcontextprotocol/conformance | 0.2.0-alpha.12 (alpha tag); latest 0.1.16 | 2026-11-15 | `npm view` |
| rmcp | 3.10.0 | 2026-11-15 | [mcp-downloads](mcp-downloads.md) |

## 9. Sources

- github.com/modelcontextprotocol/conformance: README, `requirements/2026-07-28.yaml`,
  `src/scenarios/server/*.ts`, accessed 2026-11-15.
- github.com/modelcontextprotocol/rust-sdk: `.github/workflows/conformance.yml`, accessed
  2026-11-15.
