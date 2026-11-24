---
title: "Catalog keys: the ceremony, releases and rotation"
status: draft
updated: 2026-11-23
---

# Catalog keys: the ceremony, releases and rotation

How the signed catalog's keys are made, used and replaced
([ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md), D-163, D-383). The tooling is
`cargo xtask tuf`; the background is in
[catalog repository tooling](../02-research/notes/catalog-repository-tooling.md) and
[catalog key custody](../02-research/notes/catalog-key-custody.md).

## Keys

| Role | Keys | Where it lives | Signs |
|---|---|---|---|
| root | 3, any 2 sign | the vault | every root (the keys of all roles) |
| targets | 1 | the vault | each release's list of packs |
| snapshot | 1 | the vault | each release |
| timestamp | 1 | the vault, and the `catalog-timestamp` environment secret `TUF_TIMESTAMP_KEY` | daily, 14 days |

**The vault** (D-383) is one directory:

- `vault.age`: an [age](https://age-encryption.org) identity sealed with a passphrase;
- `vault.pub`: its recipient;
- `keys/<role>.key.age`: every private key, encrypted to the vault;
- `metadata/`: the signed root;
- `public-keys.md`: the public keys.

No plain key is ever written. A copy is useless without the passphrase, so keep the directory
in **at least two places** (a cloud drive and a USB stick). Keep the passphrase in a password
manager and on paper.

Losing the passphrase loses the catalog: apps then need a new trusted root shipped in an app
update. Every `--key` and `--*-key` option takes a `*.key.age` file and asks for the
passphrase once per command. Keys never enter the repository, a chat or an issue.

## First ceremony (the maintainer)

About ten minutes, on the everyday Mac. It needs `age` (`brew install age`) and `gh`, signed in.

1. Pull `main` and build the tool: `cargo build --release -p xtask`. Then turn the network off.
2. Run the ceremony in a new directory:
   ```sh
   target/release/xtask tuf ceremony ~/teigeodata-keys
   ```
   - **The passphrase:** age asks for it twice. Press Enter both times to have age generate a
     strong one, and save what it prints in your password manager and on paper. Then type it
     once more: the ceremony opens a key back through the vault to prove it.
   - **What it writes:** the vault, six encrypted keys, `metadata/1.root.json` signed by two
     root keys, and `public-keys.md`.
3. Turn the network on. Copy `~/teigeodata-keys` to two places: the cloud drive of your
   choice and a USB stick.
4. Create the `catalog` branch with `metadata/`'s root in `main/` and `lite/` (D-384); it is
   served from GitHub's raw host. `1.root.json` is also the trusted root that apps ship.
5. Create the GitHub environment `catalog-timestamp`, limited to the deployment branch `main`.
   Put the timestamp key into it and switch the workflow on:
   ```sh
   target/release/xtask tuf secret ~/teigeodata-keys/keys/timestamp.key.age \
     --env catalog-timestamp --name TUF_TIMESTAMP_KEY
   gh variable set CATALOG_LIVE --body true
   ```

Steps 4 and 5 publish only public files and a secret, so an agent may do them with you. Steps
1 to 3 and the passphrase are yours alone.

**With a hardware key** (later, or from the start): give the ceremony an age identity you
hold, such as a YubiKey's from `age-plugin-yubikey`, instead of a passphrase vault:
`xtask tuf ceremony <dir> --identity <stub> --to <recipient>`. Opening a key then asks for the
key's PIN and a touch. Use a YubiKey with firmware 5.7 or later (YSA-2024-03).

## A release

**The layout** (D-384):

- **Two catalogs under one root:** `main/` (ODbL packs, and the CC BY ones a main app also
  uses) and `lite/` (CC BY 4.0 only), because an app chooses its licence once. Both start
  from the ceremony's `1.root.json`. They are served from the `catalog` branch at
  `https://raw.githubusercontent.com/teispace/teigeodata/catalog/main/` and `…/lite/` (D-384): that host sends
  `Access-Control-Allow-Origin: *`, ETags, gzip, a 5-minute cache and a `404` for a root
  that does not exist yet. GitHub Pages cannot serve them, because the organisation's site
  redirects every project page to `teispace.com`, which another host serves.
- **The assets:** in a GitHub release of their own, `catalog-<date>`, created as a draft and
  published once every asset is up. The catalog names each asset with that folder
  (`--folder`), so the one asset base `https://github.com/teispace/teigeodata/releases/download/`
  serves every release, although immutable releases cannot take new files after publishing.

`--assets` must hold the asset of **every** entry the catalog will list, not only the new
ones: the release verifies the whole repository against it. Download the assets of the
entries it keeps from their catalog release first (`gh release download catalog-<date> -p
'<asset>'`, the names from `tgd catalog` or the `custom.file` of each target). A missing one
is reported before any key is opened (D-403).

For each catalog, with the vault at hand (one passphrase per command):

```
cargo xtask tuf release catalog/main --assets out/assets --folder catalog-2026.11.23 \
  --pack np-places-main.tgd --pack app-np-main.tgd … \
  --targets-key ~/teigeodata-keys/keys/targets.key.age \
  --snapshot-key ~/teigeodata-keys/keys/snapshot.key.age \
  --timestamp-key ~/teigeodata-keys/keys/timestamp.key.age
```

**Deltas** (D-395). For each pack, make a delta from each of the last 3 releases a client may
hold, from the two whole packs (the older ones are the catalog's earlier assets):

```
tgd delta app-np-2026.11.22.tgd app-np-2026.11.23.tgd -o app-np-from-2026.11.22.tgd
```

`tgd delta` refuses a pair a delta cannot carry; such a pack gets no delta from that release,
and its clients update by section. Pass each delta to the release with `--delta
app-np-from-2026.11.22.tgd`: it is listed in its pack's entry and its asset written beside the
pack's.

It does the following:

- writes each pack's transport asset as `address.component.release.hash8.tgd.zst` into
  `out/assets`, never overwriting an existing asset with other bytes;
- adds or replaces the packs' entries;
- writes the next targets and snapshot and a new timestamp;
- verifies the whole repository from root 1 with the client.

A refused release leaves the repository as it was. Upload the new assets to the draft release
(`gh release create catalog-<date> --draft --prerelease --target <full SHA>`; a short SHA is
refused), check their digests against the local files, and publish it; then commit the new
metadata to `catalog`. The repository's hooks work in a worktree of that branch: with no code
there, the commit message is checked from the main checkout (D-403). Readers before 1.2 cannot read an
entry with a folder.

## The daily timestamp

The `catalog-timestamp` workflow runs `cargo xtask tuf timestamp catalog/<main|lite> --key env:TUF_TIMESTAMP_KEY`
for both catalogs every day and pushes the result. It fails when anything expires within 7 days. If the schedule
has stopped (GitHub stops schedules after 60 quiet days), re-enable it with
`gh workflow enable catalog-timestamp`.

## Rotation

Any key is replaced by a new root that lists the new key:

1. `cargo xtask tuf keygen <vault>/keys/<name>.key.age`: the new key, encrypted to the vault.
2. `cargo xtask tuf root metadata …` with the new set: it writes the next root (`N+1`).
3. Sign it with a threshold of the **previous** root's keys and a threshold of its **own**.
   Keys in both count for both.
4. If the timestamp key changed, replace the secret; if targets or snapshot changed, the next
   release signs with the new key. `cargo xtask tuf verify catalog` must pass before the commit.

Roots expire after a year; renew with the same keys the same way.

## The drill

The rotation drill runs in every test run:
- `xtask` `tuf::tests::a_release_publishes_a_verified_catalog_and_the_drill_rotates_keys`:
  root 2 drops a root key and rotates the timestamp key; a timestamp of the old key is refused;
  partial signatures and short expiry are refused;
- `teigeodata-sync` `repo::tests::the_rotation_drill`: a client holding root 1 walks to root 2;
- `xtask` `tuf::vault_tests`: a ceremony writes only encrypted keys and a root signed twice, and
  those keys sign a release that verifies; a key without its vault, with another vault, or
  changed by a byte is refused.

Before the first public release, the maintainer also runs it once by hand with the vault.
