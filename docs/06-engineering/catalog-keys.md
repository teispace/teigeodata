---
title: "Catalog keys: the ceremony, releases and rotation"
status: draft
updated: 2026-11-10
---

# Catalog keys: the ceremony, releases and rotation

How the signed catalog's keys are made, used and replaced
([ADR-0037](../03-decisions/adr/ADR-0037-keys-and-hosting.md), D-163). The tooling is
`cargo xtask tuf`; the background is in
[catalog repository tooling](../02-research/notes/catalog-repository-tooling.md).

## Keys

| Role | Keys | Where it lives | Signs |
|---|---|---|---|
| root | 3, any 2 sign | three offline media in three places | every root (the keys of all roles) |
| targets | 1 | offline medium | each release's list of packs |
| snapshot | 1 | offline medium (with targets) | each release |
| timestamp | 1 | the `catalog-timestamp` environment secret `TUF_TIMESTAMP_KEY` | daily, 14 days |

A key is a file holding a 32-byte Ed25519 seed in hex, written with mode 0600 by
`cargo xtask tuf keygen`. Keys never enter the repository, a chat or an issue. Losing two root
keys loses the catalog: apps then need a new trusted root shipped in an app update. Keep the
three root keys apart.

## First ceremony (the maintainer)

Before you start (D-210): on the everyday machine, build the tool once,
`cargo build --release -p xtask`, and copy `target/release/xtask` and this page to a fresh USB
drive. Have four more drives ready: one per root key and one for targets and snapshot. On the
offline machine (network off, ideally a fresh account), run the copied binary as
`./xtask tuf …` wherever the steps say `cargo xtask tuf …`. Only public keys and the signed
`metadata/` leave that machine; the timestamp key leaves once, for step 5.

1. On an offline machine, for each key: `cargo xtask tuf keygen <name>.key`. Note each printed
   `public` key. Copy each root key to its own medium; keep targets and snapshot together.
2. Write root 1:
   `cargo xtask tuf root metadata --root <r1> --root <r2> --root <r3> --timestamp <ts> --snapshot <sn> --targets <tg>`.
3. Sign it with two root keys, one medium at a time:
   `cargo xtask tuf sign metadata/1.root.json --role root --key <medium>/r1.key`, then `r2`.
4. Create the `catalog` branch with `metadata/`'s contents at its root, and serve it with
   GitHub Pages. `1.root.json` is also the trusted root apps ship.
5. Create the GitHub environment `catalog-timestamp` with deployment branch `main`, add the
   timestamp key's hex as the secret `TUF_TIMESTAMP_KEY`, and set the repository variable
   `CATALOG_LIVE` to `true`. Delete the timestamp key file afterwards.

## A release

With the targets and snapshot keys at hand (the timestamp key may be `env:NAME`):

```
cargo xtask tuf release catalog --assets out/assets --pack np.tgd --pack in.tgd \
  --targets-key tg.key --snapshot-key sn.key --timestamp-key env:TUF_TIMESTAMP_KEY
```

It writes each pack's transport asset as `address.component.release.hash8.tgd.zst` into
`out/assets` (an existing asset is never overwritten with other bytes), adds or replaces the
packs' entries, writes the next targets and snapshot and a new timestamp, and verifies the whole
repository from root 1 with the client. A refused release leaves the repository as it was.
Upload the assets to the release, then commit the new metadata to `catalog`.

## The daily timestamp

The `catalog-timestamp` workflow runs `cargo xtask tuf timestamp catalog --key env:TUF_TIMESTAMP_KEY`
every day and pushes the result. It fails when anything expires within 7 days. If the schedule
has stopped (GitHub stops schedules after 60 quiet days), re-enable it with
`gh workflow enable catalog-timestamp`.

## Rotation

Any key is replaced by a new root that lists the new key:

1. `cargo xtask tuf keygen` the new key.
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
- `teigeodata-sync` `repo::tests::the_rotation_drill`: a client holding root 1 walks to root 2.

Before the first public release, the maintainer also runs it once by hand with real media.
