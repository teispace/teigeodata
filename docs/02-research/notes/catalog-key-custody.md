---
title: "Catalog key custody for one maintainer"
status: accepted
updated: 2026-11-23
owner: maintainer's agents
revisit: 2027-05-23
---

# Catalog key custody for one maintainer

## 1. Question

How should the signed catalog's keys be made and kept by a project with one maintainer?
[ADR-0037](../../03-decisions/adr/ADR-0037-keys-and-hosting.md) and the first
[ceremony document](../../06-engineering/catalog-keys.md) (D-163, D-210) call for:

- a separate offline machine;
- five USB drives;
- three root keys kept in three places.

The maintainer asked whether the keys can be generated and kept on their own drives instead,
and what the best and cleanest way is. This feeds the first ceremony (phase 4.3), which blocks
the live catalog and the M8 exit review.

## 2. Summary and recommendation

**One vault, every key encrypted to it, copies anywhere** (D-383).

- `cargo xtask tuf ceremony <dir>` runs on the everyday Mac with the network off. It does the
  whole ceremony in one command:
  1. creates an [age](https://age-encryption.org) identity, the vault, sealed with a passphrase;
  2. generates the six keys and writes each one only as `keys/<role>.key.age`, encrypted to the
     vault;
  3. writes root 1 and signs it in memory with two root keys;
  4. opens one key back through the vault, which proves the passphrase.

  No plain key ever reaches a disk.
- The directory holds nothing usable without the passphrase. Copy it to **at least two places**,
  for example the maintainer's cloud drive and a USB stick. Keep the passphrase in a password
  manager and on paper.
- Every tool reads a `*.key.age` key directly and asks for the passphrase once per command.
- The timestamp key goes into its GitHub environment secret through `cargo xtask tuf secret`,
  which pipes it to `gh` and never prints it.
- Root stays at 2 of 3. The client and the ADR are unchanged. With one keyholder, all three
  root keys sit in the vault; the threshold is ready for a second maintainer, or for moving the
  root keys onto hardware later.
- **The upgrade path is hardware.** A YubiKey 5 (firmware 5.7 or later) holding the vault's
  identity through `age-plugin-yubikey` makes the vault unextractable, with no code change: the
  ceremony takes `--identity <stub> --to <recipient>`. Moving keys by root rotation keeps
  every installed app working. **Unverified here:** `age-plugin-yubikey` was not tried on a
  device.

What this gives up: a Mac that is already compromised at the moment of the ceremony could copy
the keys as they are made. A separate offline machine guards against that. The same Mac builds,
signs and pushes the code users run, so the separate machine adds little for a one-person
project, at a large cost in effort. Hardware keys close the gap later.

## 3. Findings

- **Practice in TUF projects.**
  - Sigstore's root signing uses YubiKeys held by five keyholders, threshold 3, for root and
    targets; snapshot and timestamp are online keys in Google Cloud KMS
    ([sigstore/root-signing](https://github.com/sigstore/root-signing)).
  - tuf-on-ci documents YubiKey (PIV) signers for its offline roles
    ([YUBIKEY-PIV-SETUP](https://github.com/theupdateframework/tuf-on-ci/blob/main/docs/YUBIKEY-PIV-SETUP.md)).
  - The Repository Service for TUF says root keys must be kept offline, and ties their number
    and threshold to the number of people administering the metadata
    ([RSTUF keys](https://repository-service-tuf.readthedocs.io/en/v0.3.2/guide/general/keys.html)).
  - A threshold above one protects only when the keys are held apart, by people or places.
  - FoundriesFactory treats one offline root key and one offline targets key as a practical
    baseline, with rotation signed by the previous root
    ([offline keys](https://docs.foundries.io/latest/reference-manual/security/offline-keys.html)).
- **"Offline" means not on a server.** None of these sources requires a dedicated air-gapped
  machine for a small project; what they require is that root and targets keys never live
  where releases are automated.
- **age.**
  - A passphrase-encrypted age file can be used as an identity file, so the vault opens with
    `-i vault.age` and `-` reads an identity from standard input (`age --help`, 1.3.2).
  - Passphrase files are protected by scrypt, so the passphrase's strength is the whole
    protection; an empty answer makes age generate a strong one.
  - Passphrases belong at the terminal, not in variables, which leak to shell history and other
    processes ([age-plugin-batchpass(1)](https://man.archlinux.org/man/age-plugin-batchpass.1.en)).
- **YubiKey.**
  - Firmware 5.7 added Ed25519 to PIV ([5.7 firmware specifics](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/5.7-firmware-specifics.html)).
  - The EUCLEAK side channel (YSA-2024-03, CVE-2024-45678) recovers ECDSA keys from YubiKey 5
    devices before firmware 5.7. Those devices cannot be updated, so buy 5.7 or later
    ([YSA-2024-03](https://www.yubico.com/support/security-advisory-ysa-2024-03/)).

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| A. As documented: separate offline machine, five drives, keys in three places | Strongest against a compromised everyday machine | A second machine, six drives, three locations; a lost drive or a mixed-up label loses a key; heavy for one person | Replaced |
| **B. One age vault on the everyday Mac, encrypted keys copied to cloud and USB** | One command; copies cost nothing, so losing a drive loses nothing; a leaked copy is useless without the passphrase; vendor-free format | A Mac compromised at ceremony time sees the keys; the passphrase is a single secret | **Chosen** |
| C. B with the vault's identity on YubiKeys (`age-plugin-yubikey`) | The vault key cannot be copied; a PIN and touch to sign | About USD 55 a key, two for a spare; plugin untested here | The upgrade, by rotation or a new vault |
| D. Ed25519 keys directly on YubiKey PIV, signing through PKCS#11 | The keys themselves never leave hardware | A PKCS#11 signer in xtask; no backup copy by design, so two or three devices are needed | Later, only if C is not enough |
| E. Plain key files in a cloud drive | Simplest | One leaked file or account is a catalog compromise | Rejected |
| F. Hex keys in a password manager | Easy, synced, backed up | Tied to one vendor and one account; keys pass through the clipboard | Acceptable alternative; B keeps the passphrase there instead |
| G. Cloud KMS for every role | No local secret | An account and online signing for root; ADR-0037 keeps root offline | Rejected for root and targets; possible for snapshot and timestamp later |
| H. Sigstore keyless | No key at all | The client would need online Fulcio and Rekor checks; ADR-0037 rejects this for the trust path | Rejected |

## 5. What we build ourselves, and why

`xtask tuf` gains the following, in `xtask/src/tuf/vault.rs`:

- **`ceremony`:** the whole first ceremony.
- **`*.key.age` keys:** read by every command, and written by `keygen` to the vault's recipient.
- **`secret`:** puts a key into a GitHub environment secret through `gh`'s standard input.

The `age` tool does the cryptography and the prompt, so no new Rust dependency enters the
workspace.

Tests:

- The tests run the real `age` with a plain identity standing in for the passphrase vault.
  CI installs `age`, and a missing `age` fails there rather than skipping.
- Two planted faults each fail the tests: a key read without decryption, and a root signed once.
- The passphrase path was checked by hand on 2026-11-23: a scripted terminal ran a full
  ceremony, and a wrong passphrase was refused.

## 6. Impact on the plan

- **D-383** in the decision log.
- [Catalog keys](../../06-engineering/catalog-keys.md) is rewritten around the vault.
- ADR-0037 is unchanged in substance: root 2 of 3, offline means held by the maintainer and
  never on a server. It links D-383.
- Phase 4.3's ceremony item points to the new steps.

## 7. Risks and open items

- **Losing the passphrase loses the catalog.** Apps would need a new trusted root in an app
  update. Mitigation: a password manager plus a paper copy, and a yearly check, since the
  root's renewal opens the vault anyway.
- **A compromised Mac during the ceremony** could copy the keys. Mitigation: network off, a
  fresh build, and the hardware upgrade (option C).
- **Snapshot and targets keys stay in the vault**, so a catalog release asks the maintainer
  for the passphrase. Moving them to a protected CI environment with a required reviewer
  would make data releases fully automatic; that is a separate decision for when the catalog
  release workflow is built.
- **`age-plugin-yubikey` is untried here.** Verify it on a device before relying on option C.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| age (CLI) | 1.3.2 | 2026-11-23 | Homebrew, `age --version` |
| age (Ubuntu runner) | the `age` apt package | 2026-11-23 | `apt-get install age` in `fast-check` |
| YubiKey firmware with Ed25519 PIV | 5.7 or later | 2026-11-23 | Yubico 5.7 firmware specifics |

## 9. Sources

- [sigstore/root-signing](https://github.com/sigstore/root-signing), Sigstore, accessed 2026-11-23.
- [tuf-on-ci: YubiKey PIV setup](https://github.com/theupdateframework/tuf-on-ci/blob/main/docs/YUBIKEY-PIV-SETUP.md), The Update Framework, accessed 2026-11-23.
- [Signing keys](https://repository-service-tuf.readthedocs.io/en/v0.3.2/guide/general/keys.html), Repository Service for TUF, accessed 2026-11-23.
- [Offline TUF keys](https://docs.foundries.io/latest/reference-manual/security/offline-keys.html), FoundriesFactory, accessed 2026-11-23.
- [5.7 firmware specifics](https://docs.yubico.com/hardware/yubikey/yk-tech-manual/5.7-firmware-specifics.html), Yubico, accessed 2026-11-23.
- [Security advisory YSA-2024-03](https://www.yubico.com/support/security-advisory-ysa-2024-03/), Yubico, 2024-09-03.
- [age-plugin-batchpass(1)](https://man.archlinux.org/man/age-plugin-batchpass.1.en), Arch manual pages, accessed 2026-11-23.
- [Offline PKI using 3 YubiKeys](https://vincent.bernat.ch/en/blog/2025-offline-pki-yubikeys), Vincent Bernat, 2025.
