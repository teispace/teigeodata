---
title: "The 15 sync gates"
status: draft
updated: 2026-11-11
---

# The 15 sync gates

The gates of [ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md), listed in
[offline loading and trust](../02-research/notes/offline-loading-and-trust.md) §6, and where
each one lives (plan step 4.8). Each was first shown to fail on a planted violation; the
decision named holds the evidence. `fast-check` runs `cargo test --workspace` and the two
oracle jobs on every push.

| # | Gate | Test | Planted faults | State |
|---|---|---|---|---|
| 1 | Rollback | `trust::tests::rollbacks_are_refused`; packs: `store_tests::an_older_release_is_refused` | D-159; D-175 (no check, text order) | green |
| 2 | Fast-forward recovery | `trust::tests::a_fast_forward_is_recovered_by_rotating_the_timestamp_key`, `repo::tests::the_rotation_drill` | D-175 (rotation unseen) | green |
| 3 | Freeze | `trust::tests::freeze_is_detected_but_what_was_verified_is_kept` | D-159 | green |
| 4 | Mix-and-match | `trust::tests::mix_and_match_is_refused_by_hashes_and_versions` | D-159 | green |
| 5 | Endless data | metadata: `trust::tests::endless_data_is_refused`; packs: `install_tests::the_default_transport_installs_and_refuses_bad_hosts` (`RunsOver`, `IgnoresRange`) | D-159, D-168 | green |
| 6 | Root rotation | `trust::tests::a_root_rotates_only_with_both_thresholds_and_the_next_version` | D-159 | green |
| 7 | Differential | `tests/tuf_differential.rs` against python-tuf (`tuf-oracle` job) and `tools/tough-oracle` (`tough-oracle` job), both over country roles | D-161, D-165, D-177 | green: python-tuf 43 cases, tough 23 repositories (2026-11-11) |
| 8 | Crash in an install | `store_tests::the_store_survives_a_crash_at_every_step`, `a_removal_survives_a_crash_at_every_step`, `a_repair_survives_a_crash_at_every_step` | D-171, D-172, D-173 | green; SIGKILL of a real process not yet run |
| 9 | Resume | `install_tests::resume_keeps_what_checks_and_fetches_the_rest`; `ChangesEtag` host | D-169 | green |
| 10 | Bad mirror | `install_tests::mirrors_fail_over_and_demote` | D-170 | green |
| 11 | Section delta | `store_tests::an_update_fetches_only_what_changed` (mechanism) | D-174 | **open**: two real Nepal releases to measure |
| 12 | Catalog size | `tuf::world` tests; `cargo xtask tuf world-catalog` | D-175, D-176 | **green** (2026-11-11): delegated by country, the top-level targets 3.8 KB, the snapshot 13.0 KB and the largest role (779 targets) 220 KB gzip, each against 512 KiB |
| 13 | Binary size | `tools/size-probe` | D-168 | measured: `ureq` 1.28 MB, `reqwest` 2.34 MB |
| 14 | Platform | iOS, Android and web prototypes: `store_tests::a_platform_installs_and_updates_in_passes`, `tests/sync.rs::a_platform_installs_through_downloads` in CI; `tools/platform-probe` (`apple/run.sh`, `apple/run-ios.sh`, `android/run-android.sh`, `web/run-web.sh`) by hand | D-184, D-185, D-186 | **green** (2026-11-11, by hand): every platform installs in passes, survives its app, process or browser being killed mid-download, and commits a pack the deep check passes |
| 15 | Eviction | `store_tests::plan_room_never_removes_what_must_stay`, `the_storage_manager_keeps_what_is_pinned_needed_or_kept`; the baseline is embedded, never in the store | D-172 | green |
