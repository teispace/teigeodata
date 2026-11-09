//! Frame decoding (feature `zstd`) with `ruzstd`, the pure-Rust decoder the reader already
//! carries for BLOBS (D-079). Output is capped at the frame's stated size, and the window at
//! [`MAX_FRAME`](super::MAX_FRAME), so a crafted frame can neither expand past its size nor
//! make the decoder allocate more than about two frames.

use alloc::vec::Vec;

use super::{Frame, MAX_FRAME};
use crate::Error;

/// Decodes transport frames, one at a time, reusing its buffers.
pub struct Decoder {
    zstd: ruzstd::decoding::FrameDecoder,
}

impl core::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Decoder").finish_non_exhaustive()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// A decoder whose window is at most one frame.
    #[must_use]
    pub fn new() -> Self {
        let mut zstd = ruzstd::decoding::FrameDecoder::new();
        zstd.set_max_window_size(MAX_FRAME);
        Self { zstd }
    }

    /// Decodes `frame` of a checked table from its bytes `src` (exactly its compressed range of
    /// the asset) into `out`, which it clears first.
    ///
    /// # Errors
    /// [`Error::Frame`] if `src` is not the frame's length, or does not decode to exactly its
    /// decoded length.
    pub fn frame(
        &mut self,
        index: usize,
        frame: &Frame,
        src: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let fail = |problem| Error::Frame { index, problem };
        let c = frame.compressed.end - frame.compressed.start;
        if u64::try_from(src.len()).ok() != Some(c) {
            return Err(fail("its bytes are not its compressed length"));
        }
        let n = usize::try_from(frame.decoded.end - frame.decoded.start)
            .map_err(|_| fail("too large"))?;
        self.decode(src, n, out).map_err(fail)
    }

    /// Decodes one zstd frame that must fill all of `src` and give exactly `n` bytes, into
    /// `out` (cleared first). Never holds more than its window, one step and `n` bytes, and
    /// stops at the first byte past `n`.
    ///
    /// # Errors
    /// What is wrong, if `src` is not one zstd frame of `n` decoded bytes (`n` at most
    /// [`MAX_FRAME`]).
    pub fn decode(
        &mut self,
        mut src: &[u8],
        n: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), &'static str> {
        use ruzstd::decoding::BlockDecodingStrategy;
        use ruzstd::io::Read;
        const STEP: usize = 128 << 10;
        const BAD: &str = "it is not a zstd frame that decodes";
        if u64::try_from(n).map_or(true, |n| n > MAX_FRAME) {
            return Err("its decoded length is over 1 MiB");
        }
        out.clear();
        out.resize(n, 0);
        self.zstd.reset(&mut src).map_err(|_| BAD)?;
        let mut at = 0_usize;
        loop {
            let finished = self.zstd.is_finished();
            if !finished {
                let before = src.len();
                self.zstd
                    .decode_blocks(&mut src, BlockDecodingStrategy::UptoBytes(STEP))
                    .map_err(|_| BAD)?;
                // Every call reads a block header at least; a call that reads nothing and does
                // not finish would loop forever.
                if src.len() == before && !self.zstd.is_finished() {
                    return Err(BAD);
                }
            }
            let ready = self.zstd.can_collect();
            let dst = out
                .get_mut(at..at.saturating_add(ready))
                .ok_or("it decodes to more than its stated length")?;
            let got = self.zstd.read(dst).map_err(|_| BAD)?;
            at += got;
            if finished && got == 0 {
                break;
            }
        }
        if at != n {
            return Err("it decodes to fewer bytes than its stated length");
        }
        if !src.is_empty() {
            return Err("bytes follow the frame");
        }
        Ok(())
    }
}
