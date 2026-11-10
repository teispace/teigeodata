---
title: "Catalog repository tooling: keys, signing, publishing, the timestamp job"
status: research
updated: 2026-11-10
owner: maintainer's agent
revisit: 2027-02-10
---

# Catalog repository tooling: keys, signing, publishing, the timestamp job

## 1. Question

Step 4.3's last trust item: the tooling that makes and keeps the signed catalog that
[`sync::trust`](offline-loading-and-trust.md#37-the-client-as-built-2026-11-10) verifies. Who holds
which key, how a release signs, where the metadata goes, how the timestamp stays fresh, and how
keys rotate ([ADR-0037](../../03-decisions/adr/ADR-0037-keys-and-hosting.md)).

## 2. Summary and recommendation

- **Our own repository writer, in Rust**, in `xtask tuf`, built on the client's own types:
  canonical JSON, `catalog::Entry`, the role formats. It is checked by our client (every
  published state must verify end to end) and by the python-tuf oracle (D-161). tuf-on-ci is the
  best ready-made system, but its online signing needs a cloud KMS account (only the maintainer
  can open one) and it would write the catalog entries a second time, in Python.
- **Keys:**
  - Root: 2 of 3 Ed25519 keys, offline.
  - Targets and snapshot: offline, used at release.
  - Timestamp: one online key, stored as a GitHub Actions secret in a protected environment.
  - A key is a 32-byte seed in a file kept on offline media, never in the repository.
  - Hardware keys are possible later through a `Signer` seam, without changing the format.
- **Publishing:**
  - Metadata goes to GitHub Pages, which sends `cache-control: max-age=600`, ETags and
    `access-control-allow-origin: *` (measured 2026-11-10).
  - Consistent snapshot names (`N.root.json`, `N.snapshot.json`, `N.targets.json`) never change
    once written; only `timestamp.json` is replaced.
- **The timestamp job:** daily, signing a 14-day timestamp, so ten missed days still leave a
  valid catalog.
  - GitHub disables scheduled workflows in a public repository after 60 days without activity.
    The job therefore also checks for expiry at least 7 days out and fails loudly. Its own
    commits keep the repository active.
- **Ceremony and drill:** written steps, plus the drill as an automated test: rotate a root key,
  the timestamp key and the targets key, and a client holding root 1 walks to the new root.

## 3. Findings

**tuf-on-ci** (theupdateframework/tuf-on-ci):
- Runs a TUF repository and its signing events in GitHub Actions: offline signers sign in pull
  requests, online roles are signed in CI, and the result is published to GitHub Pages.
- Sigstore's root-signing staging signs snapshot and timestamp online with Google Cloud KMS,
  and changing that key needs a Cloud KMS permission.
- Online-signed targets were added as experimental (v0.14.0) and then as a feature (v0.15.2).
  The latest release found is v0.16.0; its year could not be confirmed (**unverified**).
- Fit for us: good for a project with KMS accounts and several signers. We have neither yet,
  and our `custom` entries come from the packs (wire sizes per component), which the forge and
  `xtask` already measure in Rust.

**GitHub Pages** (measured with `curl -I` on 2026-11-10, theupdateframework.github.io and
sigstore.github.io/root-signing/timestamp.json): `cache-control: max-age=600`, `etag`,
`access-control-allow-origin: *`, `content-type: application/json`. A new timestamp reaches
every client within 10 minutes. A client sees each `N.*.json` name exactly once, so caching
never serves a mix.

**Scheduled workflows:** GitHub's documentation (the Enterprise Server copies of "Disabling and
enabling a workflow") says scheduled workflows in a public repository are disabled after 60 days
without repository activity; `gh workflow enable` re-enables them. Community reports agree
that the disabling is easy to miss.

**Secrets:**
- A secret scoped to a GitHub environment is only given to jobs that name that environment, and
  the environment's deployment-branch rule can limit it to `main`.
- The timestamp key can only sign timestamps; root rotates it.
- With the 14-day expiry, a stolen timestamp key buys a freeze of at most the time until it is
  rotated. It cannot install anything, because targets and snapshot are signed offline (TUF's
  role separation).

## 4. Options compared

| Option | Strengths | Weaknesses | Licence | Maturity / version | Verdict |
|---|---|---|---|---|---|
| Own writer in `xtask` (Rust) | one code path with the client; entries from the measured packs; no account | ours to test | n/a | new | **Use** |
| tuf-on-ci | ceremonies in PRs, hardware keys, Pages publishing | online roles need a cloud KMS; Python; would compute entries separately | MIT OR Apache-2.0 | v0.16.0 (**unverified** date) | Revisit with a KMS account or a second signer |
| `tuftool` (tough) | Rust CLI, creates and signs repos | its own layout and key files; our entries still separate | MIT OR Apache-2.0 | tough 0.24.0 | Reject for now |
| python-tuf `repository` API | reference implementation | Python release path | MIT OR Apache-2.0 | 7.0.1 | Oracle only (D-161) |

**Online timestamp key:**

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Ed25519 seed in an environment secret | no account; a few lines | a CI compromise exposes it (limited to freeze) | **Use** |
| Cloud KMS | key never leaves the KMS | account, cost, a non-Ed25519 or separate scheme | Later, through `Signer` |
| Sigstore keyless | no key at all | the client would need Sigstore verification (rejected in ADR-0037) | Reject |

## 5. What we build ourselves, and why

`teigeodata-sync::repo` (feature `repo`, std) writes and signs the four roles with the client's
canonical JSON. `xtask tuf` drives it:

- `keygen`: a new key file and its public key;
- `init`: root 1 from public keys and thresholds;
- `sign`: adds a signature to a metadata file;
- `release`: targets from transport assets, then snapshot and timestamp;
- `timestamp`: refreshes the timestamp only;
- `verify`: replays the published chain with the client from root 1 and fails on anything
  within 7 days of expiry.

Proof:
- every state the tool writes verifies with our client, and the python-tuf oracle accepts it;
- the rotation drill as a test;
- planted faults (a wrong role key, a version not raised, expiry in the past) fail.

## 6. Impact on the plan

- Phase 4.3 repository item, split: writer and `xtask tuf` with the drill; the timestamp
  workflow, ready but switched off until the maintainer creates the keys and the Pages site;
  the ceremony document.
- D-160 refined: the catalog also gives the head frame, the seek table and `BLOCK_HASHES`, so a
  plan's download bytes equal G-wire's.

### As built (2026-11-10, D-162, D-163)

- python-tuf reads the four roles our writer signs, but refuses a target listing `blake3`:
  targets carry SHA-256 alone (D-162).
- A refused write must leave nothing behind. The first version left the new targets and
  snapshot files, and the next release skipped a version. `commit` now writes the timestamp last
  and restores the previous state when the check fails; a test proves it.
- The catalog's download sizes must include what every install fetches: the head frame, the
  seek table and `BLOCK_HASHES`. `Wire::catalog_sizes` gives both G-wire and the catalog from one
  measurement, and they agree for all 4,096 component sets.

## 7. Risks and open items

- Key creation, the Pages site and the environment secret need the maintainer (accounts and
  physical media). The tooling is ready before then; the first real ceremony is theirs.
- Losing two of the three root keys loses the catalog: apps then need a new trusted root
  shipped in an update. The ceremony document covers storage in separate places.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| python-tuf | 7.0.1 | 2026-11-10 | PyPI |
| tuf-on-ci | v0.16.0 (**unverified** date) | 2026-11-10 | GitHub releases mirror |
| tough | 0.24.0 | 2026-11-06 | crates.io |

## 9. Sources

- [tuf-on-ci](https://github.com/theupdateframework/tuf-on-ci), The Update Framework, accessed 2026-11-10.
- [Sigstore root-signing-staging infrastructure](https://www.github.com/sigstore/root-signing-staging/blob/main/docs/infrastructure.md), accessed 2026-11-10.
- [Disable and enable workflows](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/disable-and-enable-workflows), GitHub Docs, accessed 2026-11-10.
- [Disabling and enabling a workflow (Enterprise Server 3.9)](https://docs.github.com/en/enterprise-server@3.9/actions/managing-workflow-runs/disabling-and-enabling-a-workflow), GitHub Docs, accessed 2026-11-10.
- GitHub Pages headers: `curl -I` on theupdateframework.github.io and sigstore.github.io, 2026-11-10.
