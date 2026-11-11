---
title: "Partial installs on disk: holes, and where file systems leave them"
status: research
updated: 2026-11-11
owner: sync
revisit: 2027-05-01
---

# Partial installs on disk: holes, and where file systems leave them

## 1. Question

A partial install (D-136) writes the head, its components' sections at their offsets and the
footer; the rest of the file is never written. Step 4.1b asks for "the partial file as a sparse
file (or offsets in a compact file where a platform has no holes)". Does the store's file
actually take only the installed bytes on disk, and if not, what is the best way to make it?

## 2. Summary and recommendation

It did not. On APFS (macOS 27, this machine, 2026-11-11) a points-only install of a 2.85 MB
test pack took 2.85 MB on disk, the same as the whole pack: APFS fills a gap of under 16 MiB
with zeros when a write extends the file, and `ftruncate` to a larger length allocates too.
Country packs have sections well under 16 MiB, so on Apple platforms a partial install saved
download but no disk.

We **punch holes explicitly**: after an install, an added component or a repair, the store frees
the 4 KiB-aligned interior of every range no held section covers (`StoreFile::punch`,
`sync::store`), with `F_PUNCHHOLE` on Apple platforms and `fallocate(FALLOC_FL_PUNCH_HOLE |
FALLOC_FL_KEEP_SIZE)` on Linux and Android. The head's offsets stay as signed, so the engine's
open (D-136) is unchanged. Measured after the change: points only 1.50 MB on disk (its sections
are 1.49 MB), points and names 1.99 MB, against 2.94 MB for the whole pack. Adding names to the
hollowed file reused the 1.49 MB of point frames from it and fetched only NAMES.

Windows keeps the default (no holes) until a Windows run exists: it needs `FSCTL_SET_SPARSE`
before `FSCTL_SET_ZERO_DATA` (**unverified**, open item). A compact file with an offset map is
not needed anywhere we ship today.

## 3. Findings

### 3.1 APFS (measured 2026-11-11, macOS 27, `/System/Volumes/Data`)

- A write 100 MB past the end of an empty file: 24 blocks on disk (a hole).
- A write past the end after a gap: the gap is filled with zeros below 16 MiB (threshold
  between 15.97 and 16.02 MiB by bisection), and is a hole above it, whatever the data before
  the gap (4 KiB, 1 MiB or 8 MiB tried).
- `ftruncate` to 3 MB, then two 4 KiB writes: 2.93 MB on disk.
- `fcntl(F_PUNCHHOLE)` on a written 3 MB file frees exactly the range asked for when the offset
  and length are 4 KiB-aligned (1,228,800 bytes freed, then read as zeros); an unaligned length
  is refused with `EINVAL`. Through `ctypes` on arm64 the variadic call must pass the struct by
  pointer as the third argument, or it fails with `EFAULT`.

### 3.2 Linux

ext4, XFS and Btrfs leave holes for gaps written past the end, and support
`FALLOC_FL_PUNCH_HOLE` with `FALLOC_FL_KEEP_SIZE` (fallocate(2)). Unaligned edges are zeroed,
not freed; we align anyway, so the same range is freed everywhere. File systems without it
return `EOPNOTSUPP`, which the store treats as "no holes" (tmpfs supports it; FAT and exFAT do
not).

### 3.3 Windows

NTFS files are not sparse until `FSCTL_SET_SPARSE`; then `FSCTL_SET_ZERO_DATA` frees a range
(Cygwin implements `fallocate(PUNCH_HOLE)` this way). Not built: no Windows target or run here.

### 3.4 Rust crates (checked 2026-11-11)

No safe wrapper covers Apple: `rustix` 1.x wraps `fallocate` (Linux) but its Apple `fcntl`
set is `F_RDADVISE`, `F_FULLFSYNC`, `F_NOCACHE` and `F_GLOBAL_NOCACHE`. `hole-punch` 0.0.3 and
its fork `drill-press` 0.1.2 call `fcntl(F_PUNCHHOLE)`, `fallocate` and `FSCTL_SET_ZERO_DATA`
with `unsafe`, through `libc` and `winapi`, and are small and little used. `libc` 0.2.191
already in the lock file has `F_PUNCHHOLE`, `fpunchhole_t` and `fallocate`.

## 4. Options compared

| Option | Strengths | Weaknesses | Verdict |
|---|---|---|---|
| Rely on the file system's holes | no code | APFS fills gaps under 16 MiB: no saving for country packs on Apple platforms | refused (measured) |
| Punch holes after writing (ours, `libc`) | head and offsets unchanged; engine open unchanged; ~40 lines; works on APFS, ext4, XFS, Btrfs, tmpfs | one `unsafe` call per platform; none on FAT, exFAT or Windows yet | **chosen** |
| `drill-press` / `hole-punch` | ready | `winapi` (unmaintained) and an extra dependency for two calls; little review | refused |
| A compact file with an offset map | works everywhere | the engine maps sections at their signed offsets: needs a remapping reader, a second layout, and repair and update over it | not needed now |

## 5. What we build ourselves, and why

`sync::holes::punch` (the one module of `teigeodata-sync` allowed `unsafe`, as `map.rs` is in the
engine, D-113) rounds the range inward to the file system's block size (`st_blksize`) and makes
the call; "not supported" is 0 bytes freed, never an error. `Store::install` and
`Store::repair` call `hollow` after cutting the staged file to length and before `fsync`: every
range between held sections (and before the footer) is punched; the head, held sections, their
`BLOCK_HASHES` and the footer never are. Punched bytes read as zeros, which is what a never
written range reads anyway, so resume, update (D-174), `verify(deep)` and repair are unchanged.

Gate: `tests/sync.rs::a_partial_install_takes_only_its_bytes_on_disk` (Apple and Linux): points
only under 60% of the file on disk, more with names but under 80%, a repair keeping the holes,
every step `status --deep` Good. Four planted faults fail it: no punch after install, none after
repair, held sections punched, a punch that frees nothing.

## 6. Impact on the plan

- 4.1b "Storage on disk": done for Apple platforms, Linux and Android; Windows sparse files
  open (D-189).
- The disk-peak formula of the storage manager counts a partial install at its sections' size
  on these platforms.

## 7. Risks and open items

- Windows: `FSCTL_SET_SPARSE` and `FSCTL_SET_ZERO_DATA`, once a Windows run exists.
- FAT and exFAT (SD cards on Android): no holes; the bytes stay, nothing breaks.
- The browser (OPFS) is not this store; whether OPFS files are sparse is unknown.

## 8. Versions checked

| Thing | Version | Checked on | Where |
|---|---|---|---|
| `libc` | 0.2.191 | 2026-11-11 | Cargo.lock, crates.io |
| `rustix` | 1.1.2 | 2026-11-11 | docs.rs |
| `drill-press` | 0.1.2 | 2026-11-11 | docs.rs |
| `hole-punch` | 0.0.3 | 2026-11-11 | docs.rs |
| macOS | 27 (Darwin 27.2), APFS | 2026-11-11 | this machine |

## 9. Sources

- [fcntl(2), `F_PUNCHHOLE`](https://git.saurik.com/apple/xnu.git/blobdiff/39236c6e673c41db228275375ab7fdb0f837b292..c3c9b80d004dbbfdf763edeb97968c6997e3b45b:/bsd/man/man2/fcntl.2),
  Apple xnu, accessed 2026-11-11.
- [fallocate(2)](https://man7.org/linux/man-pages/man2/fallocate.2.html), Linux man-pages,
  accessed 2026-11-11.
- [rustix `fcntl_apple.rs`](https://docs.rs/crate/rustix/1.1.2/source/src/fs/fcntl_apple.rs),
  rustix 1.1.2, accessed 2026-11-11.
- [drill-press](https://docs.rs/crate/drill-press/latest/source/src/unix.rs) and
  [hole-punch](https://docs.rs/crate/hole-punch/latest), accessed 2026-11-11.
- [Cygwin: fallocate(PUNCH_HOLE) by FSCTL_SET_ZERO_DATA](https://inbox.sourceware.org/cygwin-cvs/20231128100325.B7538384CB9D@sourceware.org/),
  2023-11-28.
