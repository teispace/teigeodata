---
title: "Phone-class bench: TG-17's gate without a phone"
status: accepted
updated: 2026-11-12
owner: teigeodata maintainers
revisit: 2027-05-12
---

# Phone-class bench: TG-17's gate without a phone

## 1. Question

Tithipatra asks that the embedded layer open fast, search fast and stay small in memory on a
phone (TG-17, [the consumer page](../../01-product/consumers/tithipatra.md)). No phone is
attached to this project's machines, and the shared development Mac runs at load 10 to 130.
How do we measure a low-end phone's experience, and gate it, without one? Feeds
[phase 4 §4.9c](../../07-planning/phases/phase-4.md) and decision D-235.

## 2. Summary and recommendation

- **Measure here, scale to a low-end phone by a stated factor, gate the estimate.** The
  factor is single-core speed: Geekbench 6 single-core, Apple M2 Pro about 2,689 against
  MediaTek Helio G85 (Cortex-A75 and A55; the Galaxy A05 class sold widely in South Asia) about
  424, so 6.3; the bench uses 6.5. **Unverified** on a real device: a check on one low-end
  phone calibrates it when the maintainer has one.
- **Every time is the minimum of several rounds**, so a loaded machine inflates a figure only
  when every round is slowed; an estimate over target is rerun before it is believed.
- **Heap by a counting allocator**, live and peak. The mapped pack is clean, file-backed
  memory the system can drop, so it is not the app's heap.
- **The gate:** `cargo run --release -p teigeodata-engine --all-features --example phone --
  <layer.tgd>` exits non-zero when an estimate is over its target (§3.3). Proven red with a
  3 ms sleep planted in the keystroke search (30.2 ms estimate, over 16 ms).
- **What the bench found, and what was fixed (D-235):**
  - Deriving SUBSTRING on the Tithipatra layer took 1.04 s here, 6.8 s on the phone estimate,
    with a 121 MiB heap peak. An open-addressing interner and one sort of flat (key, row)
    pairs made it 0.38 s (2.5 s) and 54 MiB, byte for byte the same section.
  - The first height on the Nepal pack took 63 ms (412 ms): it hashed all 68 MB of the DEM.
    The DEM's head is now checked on first use and each tile's blocks as a query reads it:
    0.42 ms (2.8 ms).

## 3. Findings

### 3.1 The factor

| Core | Geekbench 6 single-core | Source |
|---|--:|---|
| Apple M2 Pro (this Mac) | about 2,689 (submissions 2,516 to 2,650) | cpu-monkey.com, browser.geekbench.com (2026-11-12) |
| MediaTek Helio G85 | about 424 (a Galaxy A05 run about 400) | cpu-monkey.com, gsmarena.com reader report (2026-11-12) |

Single-core speed fits: every query and the derive run on one thread. Memory bandwidth and
flash reads differ more than the score says; the cold read of the pack from flash is not
measured (§7).

### 3.2 What a phone user feels

- One frame at 60 Hz is 16.7 ms: a keystroke's search must fit well inside it.
- The first screen of an app is a few hundred milliseconds at most; the layer's part of it
  (open plus the first query of each kind) should be a small share.
- A one-time background job after the first launch may take seconds, but not tens.

### 3.3 Targets, each on the low-end phone

| Measure | Target | Tithipatra layer, estimate (2026-11-12) |
|---|--:|--:|
| open (mapped, head checks) | 50 ms | 0.87 ms |
| first query of each kind, together | 150 ms | 35 ms |
| keystroke search p99 | 16 ms | 0.49 ms |
| keystroke search max | 50 ms | 0.90 ms |
| point query p99 (reverse 10, `admin_at`, `tz_at`, `elevation_at`) | 2 ms | 0.20 ms |
| `get` p99 | 0.5 ms | 1.9 µs |
| derive SUBSTRING, once | 3 s | 2.49 s |
| heap after open and the first queries | 4 MiB | 0.04 MiB |
| heap peak while querying | 16 MiB | 0.04 MiB |
| heap peak while deriving SUBSTRING | 64 MiB | 54.5 MiB |

Measured at load 16 on the M2 Pro; Nepal's full lite pack (with the 68 MB DEM) passes the
same targets (first queries 47 ms, derive 1.7 s).

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| a. Scaled wall-clock here (minimum of rounds) | runs on the real layers, today; covers I/O-free CPU and heap | the factor is an estimate; load inflates it | chosen |
| b. Instruction counts (gungraun) with an IPC × clock model | load-free | Linux only (Valgrind), CI has no real layers (the data build is dispatch-only) | later, when CI builds the layers |
| c. A phone emulator | real ARM code | emulation timing is not a phone's | rejected |
| d. A real low-end phone | the truth | none attached; a device lab costs | calibration, when available |

## 5. What we build ourselves, and why

The bench is an engine example (`crates/teigeodata-engine/examples/phone.rs`), reusing the
tests' pack model for probes, so it runs on any layer. A layer that ships SUBSTRING also proves
the derived section equal to it byte for byte.

## 6. Impact on the plan

- Phase 4 §4.9c TG-17: the bench and gate G-phone (D-235).
- [Benchmarks](../../06-engineering/benchmarks.md): suite P (phone class).
- Format: `encode_with` and `substring_from_names` share one entry encoder; the derive path
  interns keys. DEM: `Dem::height_checked` and `Dem::payload_range`; engine `ensure_except`.

## 7. Risks and open items

- The factor is unverified on a device; flash read speed (a cold first open after install)
  is not measured.
- Folding every name (120 ms of the 380 ms derive) allocates a string per name: a fold into a
  reused buffer is the next cut.
- Run G-phone on CI when the data build runs there.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| Geekbench | 6 | 2026-11-12 | cpu-monkey.com, browser.geekbench.com |

## 9. Sources

- [MediaTek Helio G85 vs Apple M2 Pro](https://www.cpu-monkey.com/en/compare_cpu-mediatek_helio_g85-vs-apple_m2_pro_12_cpu_19_gpu), CPU Monkey, accessed 2026-11-12.
- [Apple M2 Pro in Geekbench 6 single-core](https://www.cpu-monkey.com/en/benchmark-apple_m2_pro_12_cpu_19_gpu-geekbench_6_single_core), CPU Monkey, accessed 2026-11-12.
- [Samsung Galaxy A05 does Geekbench 6 with a Helio G85 chipset](https://m.gsmarena.com/newscomm-59475p3.php), GSMArena, accessed 2026-11-12.
