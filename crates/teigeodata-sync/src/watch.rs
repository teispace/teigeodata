//! Progress and cancellation of the store's long work (D-274,
//! [store API](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/store-api.md)).
//!
//! A [`Watch`] is shared between the thread doing the work and anyone observing it: the work
//! adds the bytes each request brings, an observer reads a snapshot or waits for the next one,
//! and anyone may cancel. [`Watched`] wraps a [`Transport`] so that the installer and the
//! catalog refresh report and stop without knowing it: it checks the flag before every
//! request and inside every streamed body, so a cancel ends the work at the next chunk. The
//! store commits an install last (D-171), so cancelled work leaves the journal as it was.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::Range;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use std::sync::{Condvar, Mutex, MutexGuard};

use crate::fetch::{FetchError, Got, Transport};

/// A snapshot of the work: bytes received of those expected, and a sequence number that moves
/// on with every change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Bytes received so far.
    pub done: u64,
    /// Bytes expected, from the catalog's sizes; never below `done`, and equal to it once the
    /// work ends.
    pub total: u64,
    /// Moves on with every change; [`Watch::wait`] waits for it to move.
    pub seq: u64,
    /// The work has ended (done, failed or cancelled).
    pub ended: bool,
}

/// The shared state of one piece of work.
#[derive(Debug, Default)]
pub struct Watch {
    progress: Mutex<Progress>,
    moved: Condvar,
    cancelled: AtomicBool,
}

impl Watch {
    /// A watch on work not started.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Progress> {
        // A panic while holding the lock leaves plain numbers: still valid.
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn change(&self, f: impl FnOnce(&mut Progress)) {
        let mut p = self.lock();
        f(&mut p);
        p.total = p.total.max(p.done);
        p.seq += 1;
        drop(p);
        self.moved.notify_all();
    }

    /// Asks the work to stop; it ends at its next request or chunk.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        // Wake waiters, so an observer sees the request at once.
        self.change(|_| {});
    }

    /// Whether [`Watch::cancel`] was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// The current snapshot.
    #[must_use]
    pub fn progress(&self) -> Progress {
        *self.lock()
    }

    /// The work expects `bytes` more.
    pub fn expect(&self, bytes: u64) {
        self.change(|p| p.total = p.total.saturating_add(bytes));
    }

    /// The work received `bytes`.
    pub fn add(&self, bytes: u64) {
        self.change(|p| p.done = p.done.saturating_add(bytes));
    }

    /// The work ended: its total becomes what was done.
    pub fn end(&self) {
        self.change(|p| {
            p.total = p.done;
            p.ended = true;
        });
    }

    /// Waits until the snapshot's sequence differs from `seq`, or the work has ended, or
    /// `timeout` passes; returns the snapshot then.
    #[must_use]
    pub fn wait(&self, seq: u64, timeout: Duration) -> Progress {
        let p = self.lock();
        let (p, _) = self
            .moved
            .wait_timeout_while(p, timeout, |p| p.seq == seq && !p.ended)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *p
    }
}

/// A transport that reports to a [`Watch`] and stops when it is cancelled. Without a watch it
/// is the transport it wraps.
#[derive(Debug, Default)]
pub struct Watched<T> {
    inner: T,
    watch: Option<Arc<Watch>>,
}

impl<T> Watched<T> {
    /// `inner`, unwatched.
    pub const fn new(inner: T) -> Self {
        Self { inner, watch: None }
    }

    /// Reports to `watch` from now on, or to nothing.
    pub fn watch(&mut self, watch: Option<Arc<Watch>>) {
        self.watch = watch;
    }

    fn check(&self) -> Result<(), FetchError> {
        match &self.watch {
            Some(w) if w.is_cancelled() => Err(FetchError::Cancelled),
            _ => Ok(()),
        }
    }

    fn add(&self, bytes: usize) {
        if let Some(w) = &self.watch {
            w.add(bytes as u64);
        }
    }
}

impl<T: Transport> Transport for Watched<T> {
    fn get(&mut self, url: &str, range: Range<u64>, out: &mut Vec<u8>) -> Result<Got, FetchError> {
        self.check()?;
        let got = self.inner.get(url, range, out)?;
        self.add(out.len());
        Ok(got)
    }

    fn get_streamed(
        &mut self,
        url: &str,
        range: Range<u64>,
        buf: &mut Vec<u8>,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), FetchError>,
    ) -> Result<Got, FetchError> {
        self.check()?;
        let watch = self.watch.clone();
        self.inner.get_streamed(url, range, buf, &mut |bytes| {
            if let Some(w) = &watch {
                if w.is_cancelled() {
                    return Err(FetchError::Cancelled);
                }
                w.add(bytes.len() as u64);
            }
            sink(bytes)
        })
    }

    fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError> {
        self.fetch_streamed(url, max, out, &mut |_| Ok(()))
    }

    fn fetch_streamed(
        &mut self,
        url: &str,
        max: u64,
        out: &mut Vec<u8>,
        on_chunk: &mut dyn FnMut(usize) -> Result<(), FetchError>,
    ) -> Result<(), FetchError> {
        self.check()?;
        let watch = self.watch.clone();
        self.inner.fetch_streamed(url, max, out, &mut |n| {
            if let Some(w) = &watch {
                if w.is_cancelled() {
                    return Err(FetchError::Cancelled);
                }
                w.add(n as u64);
            }
            on_chunk(n)
        })
    }

    fn expect(&mut self, bytes: u64) {
        if let Some(w) = &self.watch {
            w.expect(bytes);
        }
        self.inner.expect(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::{Progress, Watch, Watched};
    use crate::fetch::{FetchError, Got, Transport};
    use alloc::string::String;
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::ops::Range;
    use core::time::Duration;

    /// Serves zeros in chunks of 4 bytes, and records each URL asked for.
    #[derive(Default)]
    struct Zeros(Vec<String>);

    impl Transport for Zeros {
        fn get(
            &mut self,
            url: &str,
            range: Range<u64>,
            out: &mut Vec<u8>,
        ) -> Result<Got, FetchError> {
            self.0.push(url.into());
            out.clear();
            out.resize(usize::try_from(range.end - range.start).unwrap(), 0);
            Ok(Got::default())
        }

        fn get_streamed(
            &mut self,
            url: &str,
            range: Range<u64>,
            buf: &mut Vec<u8>,
            sink: &mut dyn FnMut(&[u8]) -> Result<(), FetchError>,
        ) -> Result<Got, FetchError> {
            let got = self.get(url, range, buf)?;
            for chunk in buf.chunks(4) {
                sink(chunk)?;
            }
            Ok(got)
        }

        fn fetch(&mut self, url: &str, _max: u64, out: &mut Vec<u8>) -> Result<(), FetchError> {
            self.0.push(url.into());
            out.clear();
            out.extend_from_slice(b"{}");
            Ok(())
        }
    }

    #[test]
    fn progress_counts_every_byte_and_ends_at_its_total() {
        let w = Arc::new(Watch::new());
        let mut t = Watched::new(Zeros::default());
        t.watch(Some(w.clone()));
        t.expect(20);
        let (mut buf, mut out) = (Vec::new(), Vec::new());
        t.get_streamed("a", 0..10, &mut buf, &mut |_| Ok(()))
            .unwrap();
        t.get("b", 0..6, &mut out).unwrap();
        t.fetch("c", 100, &mut out).unwrap();
        let p = w.progress();
        assert_eq!((p.done, p.total, p.ended), (18, 20, false));
        // Ten bytes in chunks of four, one get, one fetch, one expect: each moved the seq.
        assert_eq!(p.seq, 1 + 3 + 1 + 1);
        w.end();
        assert_eq!(
            w.progress(),
            Progress {
                done: 18,
                total: 18,
                seq: 7,
                ended: true
            }
        );
        // A wait on an ended watch returns at once.
        assert!(w.wait(7, Duration::from_secs(60)).ended);
    }

    #[test]
    fn a_cancel_stops_the_next_request_and_the_body_in_flight() {
        let w = Arc::new(Watch::new());
        let mut t = Watched::new(Zeros::default());
        t.watch(Some(w.clone()));
        let mut buf = Vec::new();
        let mut chunks = 0;
        let r = t.get_streamed("a", 0..40, &mut buf, &mut |_| {
            chunks += 1;
            if chunks == 2 {
                w.cancel();
            }
            Ok(())
        });
        assert!(matches!(r, Err(FetchError::Cancelled)));
        assert_eq!(chunks, 2, "no chunk after the cancel reaches the sink");
        assert!(matches!(
            t.fetch("b", 10, &mut buf),
            Err(FetchError::Cancelled)
        ));
        assert_eq!(t.inner.0.len(), 1, "no request after the cancel");
    }

    #[test]
    fn a_wait_returns_when_the_work_moves_or_the_time_passes() {
        let w = Arc::new(Watch::new());
        let seq = w.progress().seq;
        let p = w.wait(seq, Duration::from_millis(10));
        assert_eq!(p.seq, seq, "timed out unchanged");
        let mover = {
            let w = w.clone();
            std::thread::spawn(move || w.add(5))
        };
        let p = w.wait(seq, Duration::from_secs(60));
        mover.join().unwrap();
        assert_eq!((p.done, p.seq), (5, seq + 1));
    }
}
