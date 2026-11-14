---
title: "Load tests and size of the servers"
status: accepted
updated: 2026-11-15
owner: maintainer's agents
revisit: 2026-12-15
---

# Load tests and size of the servers

## 1. Question

Phase 6.4: what do `tgd serve` and its MCP endpoint sustain on real packs (requests per
second, p99 latency, memory footprint under load, no growth), with the numbers gated so a
regression fails; and what do the servers add to `tgd`?

## 2. Summary and recommendation

- **A harness, `cargo xtask load`,** runs the shipped binary: it builds `tgd` (release,
  default features), serves each pack on loopback, finds a place by a query (`--query`,
  `Kathmandu` by default, so the same command works on any pack that holds it), checks that
  every route in the mix answers 200 (the budget may name a pack's place: the world baseline
  holds countries, so it uses `Nepal`), then drives each with `oha` and samples the server's
  physical footprint. The mix covers what users call: native search (1, 16 and 64
  connections), reverse, get, near, hierarchy, a country's boundary, Nominatim search, and an
  MCP `tools/call` over `/mcp`.
- **Gated by a budget** (`tools/load/budget.toml`, per pack): a floor on requests per second
  and a ceiling on p99 per route, a ceiling on the footprint, and on its growth across a
  second identical round (a leak). Every request must succeed. The floors sit well under the
  measured numbers because the machine is shared (load average 8 to 18): they catch a
  regression of a kind, an accidental blocking call, a per-request allocation storm, a lock,
  a leak, not a few percent. Run before a release and after any change to the request path.
- **Size:** `cargo xtask sizes --tgd` builds `tgd` per feature set and reports it stripped
  (release, macOS arm64, 2026-11-15): both servers 9.38 MB (4.37 MB gzipped), neither
  4.59 MB (2.35), HTTP only 6.06 MB (3.00), MCP only 6.92 MB (3.31). The servers stay behind
  the CLI features `serve` and `mcp`, **on by default**: serving a pack is part of what the
  CLI is for, the extra is 2 MB gzipped, and an embedder that wants the small binary builds
  with `--no-default-features`. The libraries (bindings) never link the servers.

## 3. Findings

**`oha` 1.17.0** (crates.io, 2026-11-15, MIT): `-z` duration, `-c` connections,
`--output-format json` gives `summary.requestsPerSec`, `latencyPercentiles.p99`,
`statusCodeDistribution` and `errorDistribution` (requests cut by the deadline show as
`aborted due to deadline` and are not failures). It is the generator of the 6.0 A/B
([servers](servers.md) §5.1), so numbers compare.

**Footprint:** macOS `footprint -p <pid>` prints the physical footprint (`Footprint: N KB`),
what the system charges the process, compressed and swapped pages included; `ps` RSS
under-reports (AGENTS.md §7). Linux: `VmRSS` in `/proc/<pid>/status`.

**Shared machine:** two rounds interleaved in 6.0 differed by up to 35 % at c=1 when another
project built. Budgets therefore guard against regressions of a kind, and the measured
numbers are recorded with their date and load average.

**Results (2026-11-15, load average 8 to 21, round two of four runs per pack).** Requests
per second (p99 ms):

| Route | c | np-places-lite | world-baseline-lite |
|---|---|---|---|
| search | 1 | 12.6k to 14k (0.12 to 0.22) | 5.8k to 20k (0.08 to 2.0) |
| search | 16 | 78k to 88k (0.5 to 0.7) | 83k to 99k (0.35 to 0.8) |
| search | 64 | 65k to 97k (1.4 to 3.9) | 84k to 123k (1.1 to 2.8) |
| reverse | 16 | 56k to 94k (0.4 to 1.6) | 75k to 89k (0.45 to 0.9) |
| get | 16 | 89k to 102k (0.3 to 0.8) | 102k to 103k (0.3) |
| near | 16 | 65k to 93k (0.4 to 1.1) | 69k to 89k (0.5 to 1.1) |
| hierarchy | 16 | 75k to 92k (0.5 to 0.8) | 93k to 105k (0.3 to 0.9) |
| boundary (the country) | 16 | 5.2k to 10k (5 to 25) | 43k to 60k (0.8 to 1.7) |
| Nominatim search | 16 | 49k to 57k (0.8 to 1.2) | 75k to 93k (0.4 to 0.9) |
| MCP `tools/call` search | 16 | 44k to 45k (0.9 to 1.1) | 40k to 58k (0.7 to 1.8) |

Footprint: 5 to 6 MB idle, 10 to 15 MB peak, growth across a round within ±4 MB (allocator
noise, no trend). Every request succeeded.

**What the first runs found.** Boundary ran at 1.2k to 2.6k requests per second on the Nepal
pack: it decoded the outline at every level of detail to pick one by its vertex count. It now
counts vertices from the arcs' level column (`Engine::boundary_vertices`, no decode), decodes
once, and writes coordinates as exact 7-decimal degrees from the stored integers; 7.4k to 8k
requests per second, p50 5.7 to 1.7 ms, every answer byte-identical (16 boundaries at 4 LOD
settings). The country's outline is large (Nepal's full ring), so this route stays the
slowest; the tail is the outline's size, not the server.

**The gate's proof.** A 1 ms blocking sleep in `/v1`'s handler failed 8 points (every `/v1`
route near 6.5k requests per second, under its floor); a 1 KiB leak per request failed both
footprint checks (4.7 GB peak, 2.3 GB growth across a round). The unmodified server passes.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| **`oha` driven by an xtask** | Mature, JSON output, already used in 6.0 | An external tool to install | **Yes** |
| Our own Rust load generator | No external tool | A coordinated-omission-prone generator to maintain | No |
| k6 or wrk | Scriptable | Not Rust, no JSON summary (wrk) | No |
| Load in CI | Repeatable hardware | No real packs in CI (data releases are local); 4 shared vCPUs | Later, with a data release |

## 5. What we build ourselves, and why

`xtask/src/load.rs` and `tools/load/budget.toml`, proved on planted faults (§3), and
`cargo xtask sizes --tgd`. No size gate on `tgd`: the libraries' sizes are what users ship
(Z2); the binary's size is reported per release.

## 6. Impact on the plan

Phase 6.4's two items are built as above. Decision D-320. Run the gate before a release and
after any change to the request path; it is not in CI (no real packs there).

## 7. Risks and open items

- Numbers come from a shared laptop; a dedicated runner would allow tighter budgets.
- Load on the hosted tier (many packs, many tenants) is a later concern; nothing here blocks it.

## 8. Versions checked

| Item | Version | Date | Where |
|---|---|---|---|
| oha | 1.17.0 | 2026-11-15 | `cargo search oha` |

## 9. Sources

- oha's `--help` and JSON output, run 2026-11-15.
- [servers](servers.md) §5.1, the 6.0 A/B and its method.
