//! Holes in installed pack files (D-189): the only module of `teigeodata-sync` allowed
//! `unsafe`, for the one system call that frees a file's blocks.
//!
//! A partial install writes only its components' sections; the rest of the file is never
//! written. File systems do not all leave that as a hole: APFS fills a gap of under 16 MiB with
//! zeros when a write extends the file (measured 2026-11-11), so a points-only install of a
//! country pack took the whole pack's space. The store therefore frees those ranges itself:
//! `F_PUNCHHOLE` on Apple platforms, `fallocate(FALLOC_FL_PUNCH_HOLE)` on Linux and Android.
//! Where neither exists, or the file system refuses, nothing happens: the bytes stay and read
//! the same, so freeing space is never needed for correctness.

use std::fs::File;
use std::io;

/// Frees the storage of the file-system blocks wholly inside `start..end` of `file`, which then
/// read as zeros, and returns the bytes freed: 0 where the platform or file system cannot.
///
/// # Errors
/// The file system's, except that it does not support holes.
pub(crate) fn punch(file: &File, start: u64, end: u64) -> io::Result<u64> {
    let block = block_of(file);
    let from = start.div_ceil(block).saturating_mul(block);
    let to = end / block * block;
    if to <= from {
        return Ok(0);
    }
    sys(file, from, to - from)
}

/// The file system's block size, to keep a punch inside the range asked for.
fn block_of(file: &File) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        file.metadata()
            .map(|m| m.blksize())
            .ok()
            .filter(|b| b.is_power_of_two() && *b >= 512)
            .unwrap_or(4096)
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        4096
    }
}

/// Whether `e` says only that the file system has no holes.
#[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
fn unsupported(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(libc::EINVAL | libc::ENOTSUP | libc::ENOSYS)
    ) || e.raw_os_error() == Some(libc::EOPNOTSUPP)
}

#[cfg(target_vendor = "apple")]
fn sys(file: &File, offset: u64, len: u64) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    let (Ok(fp_offset), Ok(fp_length)) =
        (libc::off_t::try_from(offset), libc::off_t::try_from(len))
    else {
        return Ok(0);
    };
    let arg = libc::fpunchhole_t {
        fp_flags: 0,
        reserved: 0,
        fp_offset,
        fp_length,
    };
    #[allow(
        unsafe_code,
        reason = "D-189: F_PUNCHHOLE reads one `fpunchhole_t`, which lives on this stack for \
                  the call; the descriptor is borrowed from an open `File`"
    )]
    // SAFETY: `fcntl(F_PUNCHHOLE)` takes a pointer to an `fpunchhole_t`; `arg` is valid and
    // outlives the call, and `file` keeps the descriptor open.
    let r = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_PUNCHHOLE, &raw const arg) };
    finish(r, len)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn sys(file: &File, offset: u64, len: u64) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    let (Ok(off), Ok(n)) = (libc::off_t::try_from(offset), libc::off_t::try_from(len)) else {
        return Ok(0);
    };
    #[allow(
        unsafe_code,
        reason = "D-189: `fallocate` takes only integers; the descriptor is borrowed from an \
                  open `File`"
    )]
    // SAFETY: plain integer arguments; `file` keeps the descriptor open.
    let r = unsafe {
        libc::fallocate(
            file.as_raw_fd(),
            libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
            off,
            n,
        )
    };
    finish(r, len)
}

#[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
fn finish(r: libc::c_int, len: u64) -> io::Result<u64> {
    if r == 0 {
        return Ok(len);
    }
    let e = io::Error::last_os_error();
    if unsupported(&e) { Ok(0) } else { Err(e) }
}

/// No holes here: the bytes stay (Windows needs `FSCTL_SET_SPARSE`, an open item of D-189).
#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
fn sys(_: &File, _: u64, _: u64) -> io::Result<u64> {
    Ok(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::naive_bytecount, reason = "tests")]
    use alloc::format;
    use alloc::vec;
    use alloc::vec::Vec;
    use std::io::{Read as _, Seek as _, SeekFrom, Write as _};

    use super::punch;

    /// On this machine's file system, a punched range reads as zeros, the bytes around it are
    /// kept, and (on a file system with holes) its blocks are freed.
    #[test]
    fn a_punched_range_reads_zero_and_frees_its_blocks() {
        let path = std::env::temp_dir().join(format!("tgd-holes-{}", std::process::id()));
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let len = 3_000_000_usize;
        f.write_all(&vec![7_u8; len]).unwrap();
        f.sync_all().unwrap();
        let (start, end) = (65_537_u64, 2_000_000_u64);
        let freed = punch(&f, start, end).unwrap();
        f.sync_all().unwrap();
        let mut back = Vec::new();
        f.seek(SeekFrom::Start(0)).unwrap();
        f.read_to_end(&mut back).unwrap();
        assert_eq!(back.len(), len, "the length is kept");
        let (s, e) = (
            usize::try_from(start).unwrap(),
            usize::try_from(end).unwrap(),
        );
        assert!(
            back[..s].iter().all(|&b| b == 7),
            "bytes before the range kept"
        );
        assert!(
            back[e..].iter().all(|&b| b == 7),
            "bytes after the range kept"
        );
        let zeros = back[s..e].iter().filter(|&&b| b == 0).count() as u64;
        assert_eq!(zeros, freed, "exactly the freed bytes read as zero");
        #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
        {
            use std::os::unix::fs::MetadataExt;
            assert!(freed > (end - start) * 9 / 10, "freed {freed}");
            let on_disk = f.metadata().unwrap().blocks() * 512;
            assert!(
                on_disk + freed * 9 / 10 <= len as u64 + 65_536,
                "on disk {on_disk}"
            );
        }
        drop(f);
        std::fs::remove_file(&path).unwrap();
    }
}
