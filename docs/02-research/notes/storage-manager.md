---
title: "Storage manager: quota, eviction, pinning, dependencies and the snapshot swap"
status: research
updated: 2026-11-11
owner: sync, engine
revisit: 2027-05-01
---

# Storage manager: quota, eviction, pinning, dependencies and the snapshot swap

## 1. Question

Step 4.6 builds the storage manager on the store of D-171 (`sync::store`), and 4.5 still owes
the snapshot swap. What must they do, and how do comparable systems do it?

1. Which packs may be removed to make room, in what order, and which never.
2. How "free space" is known on each platform, and who measures it.
3. How a removal and an eviction stay crash-safe with the journal as the commit point.
4. How the engine follows the journal while queries run on the old packs.

Earlier work fixed the frame: LRU by last use, pinning, dependency protection, quota off
unless the developer sets one ([offline loading and trust](offline-loading-and-trust.md) §2),
and the disk-peak formula ([download and delta](download-and-delta.md) §3.4).

## 2. Summary and recommendation

- **Two marks per pack, as apt and Flatpak do.** `asked` (the user or app installed it) or
  needed (installed as another pack's dependency), and `pinned`. A needed pack goes when no
  installed pack depends on it any more (apt's `auto`); a pinned pack is never evicted
  (Flatpak's pin). Removal of a pack another one needs is refused.
- **Eviction is a pure plan over the journal's records, then one commit.** The plan takes the
  bytes needed, the bytes available (the caller's number), the quota and the packs to keep,
  and picks least-recently-used candidates until it fits. A candidate is unpinned, not kept,
  and not a dependency of any pack that stays; removing one can free its own dependencies,
  which are then candidates too. If everything removable is not enough, nothing is removed and
  the error says how much is needed and how much could be had.
- **Free space comes from the caller.** The platform knows best: Android's
  `StorageManager.getAllocatableBytes` counts cache the system can clear, iOS's
  `volumeAvailableCapacityForImportantUsage` is meant for data the user asked for, the web has
  `navigator.storage.estimate()`. `std` has no portable call. The store takes the number.
- **Last use is kept in memory and saved with the next commit or on request.** Writing the
  journal on every query would cost two syncs per use. A crash loses only recent order, which
  only shifts which pack is evicted first; it never removes a pinned or needed pack.
- **Removal is commit, then delete.** The journal without the pack is committed first; then the
  files are removed best effort. A crash between leaves a file the journal does not list, which
  recovery (D-171) already removes.
- **The snapshot swap follows the journal.** `Shared::follow` builds the next engine from the
  current one: it opens each listed pack not yet open, drops each open store layer the journal
  no longer lists, and publishes once. Queries keep their snapshot, so a dropped file stays
  mapped until they end (on Unix the inode lives; on Windows the delete fails and recovery
  removes it later). A pack that fails to open is reported, not fatal, so one damaged file never
  hides the rest.

## 3. Findings

**apt.** `apt-mark` keeps an `auto` or `manual` mark per package in
`/var/lib/apt/extended_states`; `auto` packages are removed by `autoremove` "when no more
manually installed packages depend on this package" (apt-mark(8)). That is exactly our
dependency case: a country's admin pack installed for a city pack.

**Flatpak.** `flatpak pin` keeps patterns of runtimes that "will never be automatically
uninstalled", and `flatpak uninstall --unused` skips them (flatpak-pin(1)). Pins are separate
from the manual/auto mark: a pinned pack can be auto. We do the same: `pinned` and `asked` are
independent.

**Android.** `StorageManager.getAllocatableBytes(UUID)` is "the maximum number of new bytes that
your app can allocate for itself", including cached data the system may clear; Google advises it
over `getUsableSpace()`, on a worker thread. `allocateBytes(FileDescriptor, long)` reserves space
for an open file (API 26).

**Apple.** `URLResourceKey.volumeAvailableCapacityForImportantUsageKey` (iOS 11+) is the capacity
"for storing important resources", the key for downloads a user asked for; reading it needs a
reason in `PrivacyInfo.xcprivacy`.

**Web.** `navigator.storage.estimate()` gives usage and quota per origin
([offline loading and trust](offline-loading-and-trust.md) §3.3).

## 4. Options compared

| Question | Option | Verdict |
|---|---|---|
| Removal order | LRU by last use | **Use**: the agreed rule; cheap |
| | Size first (largest frees most) | Tie-break only: removes a pack in daily use |
| | LFU | Needs counts that a crash loses; no gain shown |
| Last-use record | in the journal at every use | Rejected: two syncs per use |
| | in memory, saved with the next commit or `save()` | **Use** |
| | a separate usage file | Rejected: a second file to keep consistent, for order only |
| Free space | the store measures (`statvfs`, `GetDiskFreeSpaceEx`) | Rejected: misses what the OS can clear; a new dependency |
| | the caller passes it | **Use** |
| Dependency removal | refuse removing a needed pack | **Use**, and remove auto packs left unneeded |
| | remove the dependants too | Rejected: surprising, loses data the user asked for |

## 5. What we build ourselves, and why

No crate does this; it is a small policy over our journal. `store::plan_room` is pure, so it is
tested on generated records (every pinned, kept and needed pack survives; the result fits or
nothing is removed). The journal goes to version 2 (marks, dependencies, last use); version 1
is read and upgraded on the next commit, and a newer version is refused, as before.

## 6. Impact on the plan

- 4.6 first item: built here (D-172).
- 4.5 snapshot swap: built here with 4.6 (D-172).
- The CLI (4.9) passes free space from the platform: `install` plans room first.

## 7. Risks and open items

- On Windows a mapped file cannot be deleted, so evicted space returns only when the last
  snapshot drops and the next recovery runs (**unverified**; no Windows run yet).
- Last use is the caller's clock in seconds; a clock set back only reorders eviction.
- Free space can change between the plan and the download; the install still fails cleanly
  with an I/O error, and the journal is untouched.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| apt-mark(8) | APT 2.x | 2026-11-11 | mankier.com |
| flatpak-pin(1) | Flatpak 1.14 | 2026-11-11 | manpages.ubuntu.com |
| `StorageManager` | API 26+ | 2026-11-11 | developer.android.com |
| `volumeAvailableCapacityForImportantUsageKey` | iOS 11+ | 2026-11-11 | developer.apple.com |

## 9. Sources

- [apt-mark(8)](https://www.mankier.com/8/apt-mark), Debian APT, accessed 2026-11-11.
- [flatpak-pin(1)](https://manpages.ubuntu.com/manpages/lunar/man1/flatpak-pin.1.html), Flatpak
  1.14, accessed 2026-11-11.
- [flatpak-uninstall(1)](https://www.mankier.com/1/flatpak-uninstall), accessed 2026-11-11.
- [StorageManager](https://developer.android.com/reference/android/os/storage/StorageManager),
  Android Developers, accessed 2026-11-11.
- [volumeAvailableCapacityForImportantUsageKey](https://developer.apple.com/documentation/foundation/urlresourcekey/volumeavailablecapacityforimportantusagekey),
  Apple Developer, accessed 2026-11-11.
