//! L5 snapshot swap A/B (engine note §3.3): how a query reaches the current set of packs.
//! `arc-swap` against `std` `RwLock<Arc<Engine>>` and `Mutex<Arc<Engine>>`, each wrapping a
//! `get` on a synthetic pack: one thread, then 8 reader threads while a writer publishes a new
//! snapshot every 100 µs. Run: `cargo run --release -p teigeodata-engine --example snapshot-ab`.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    reason = "a measurement binary"
)]

#[path = "../tests/common/mod.rs"]
mod common;

use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use teigeodata_engine::{Engine, Layer, LayerOptions};

/// One way of holding the current snapshot.
trait Holder: Sync {
    const NAME: &'static str;
    fn new(e: Engine) -> Self;
    /// Runs `f` on the current snapshot.
    fn with<R>(&self, f: impl FnOnce(&Engine) -> R) -> R;
    fn publish(&self, e: Engine);
    fn current(&self) -> Arc<Engine>;
}

struct Swap(ArcSwap<Engine>);
impl Holder for Swap {
    const NAME: &'static str = "arc-swap load()";
    fn new(e: Engine) -> Self {
        Self(ArcSwap::from_pointee(e))
    }
    fn with<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        f(&self.0.load())
    }
    fn publish(&self, e: Engine) {
        self.0.store(Arc::new(e));
    }
    fn current(&self) -> Arc<Engine> {
        self.0.load_full()
    }
}

/// The read lock held for the whole query.
struct Rw(RwLock<Arc<Engine>>);
impl Holder for Rw {
    const NAME: &'static str = "RwLock, held";
    fn new(e: Engine) -> Self {
        Self(RwLock::new(Arc::new(e)))
    }
    fn with<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        f(&self.0.read().unwrap())
    }
    fn publish(&self, e: Engine) {
        *self.0.write().unwrap() = Arc::new(e);
    }
    fn current(&self) -> Arc<Engine> {
        self.0.read().unwrap().clone()
    }
}

/// The read lock held only to clone the `Arc`.
struct RwClone(RwLock<Arc<Engine>>);
impl Holder for RwClone {
    const NAME: &'static str = "RwLock, Arc cloned";
    fn new(e: Engine) -> Self {
        Self(RwLock::new(Arc::new(e)))
    }
    fn with<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        let e = self.0.read().unwrap().clone();
        f(&e)
    }
    fn publish(&self, e: Engine) {
        *self.0.write().unwrap() = Arc::new(e);
    }
    fn current(&self) -> Arc<Engine> {
        self.0.read().unwrap().clone()
    }
}

struct Mx(Mutex<Arc<Engine>>);
impl Holder for Mx {
    const NAME: &'static str = "Mutex, Arc cloned";
    fn new(e: Engine) -> Self {
        Self(Mutex::new(Arc::new(e)))
    }
    fn with<R>(&self, f: impl FnOnce(&Engine) -> R) -> R {
        let e = self.0.lock().unwrap().clone();
        f(&e)
    }
    fn publish(&self, e: Engine) {
        *self.0.lock().unwrap() = Arc::new(e);
    }
    fn current(&self) -> Arc<Engine> {
        self.0.lock().unwrap().clone()
    }
}

const READERS: usize = 8;
const RUN: Duration = Duration::from_millis(1500);

fn run<H: Holder>(engine: &Engine, ids: &[teigeodata_core::PlaceId]) {
    let h = H::new(engine.clone());
    // One thread: ns per query.
    let n = 2_000_000;
    let t = Instant::now();
    for i in 0..n {
        black_box(h.with(|e| e.get(ids[i % ids.len()]).unwrap()));
    }
    let one = t.elapsed().as_nanos() as f64 / n as f64;
    // Readers against a writer publishing every 100 µs.
    let stop = AtomicBool::new(false);
    let done = AtomicU64::new(0);
    let (published, worst) = thread::scope(|sc| {
        for r in 0..READERS {
            let (h, stop, done) = (&h, &stop, &done);
            sc.spawn(move || {
                let mut i = r * 7919;
                let mut k = 0_u64;
                while !stop.load(Ordering::Relaxed) {
                    for _ in 0..256 {
                        black_box(h.with(|e| e.get(ids[i % ids.len()]).unwrap()));
                        i += 1;
                    }
                    k += 256;
                }
                done.fetch_add(k, Ordering::Relaxed);
            });
        }
        let writer = sc.spawn(|| {
            let (mut published, mut worst) = (0_u64, Duration::ZERO);
            let end = Instant::now() + RUN;
            while Instant::now() < end {
                let next = (*h.current()).clone();
                let t = Instant::now();
                h.publish(next);
                worst = worst.max(t.elapsed());
                published += 1;
                thread::sleep(Duration::from_micros(100));
            }
            stop.store(true, Ordering::Relaxed);
            (published, worst)
        });
        writer.join().unwrap()
    });
    let per_reader = done.load(Ordering::Relaxed) as f64 / READERS as f64 / RUN.as_secs_f64();
    println!(
        "| {} | {one:.1} | {:.0} | {published} | {:.1} |",
        H::NAME,
        1e9 / per_reader,
        worst.as_secs_f64() * 1e6
    );
}

fn main() {
    let model = common::synthetic(50_000, 7);
    let mut engine = Engine::new();
    for p in model.split(3, |_, id| (id.get() % 3) as usize) {
        engine.add(Layer::from_bytes(p, LayerOptions::default()).unwrap());
    }
    let ids = model.ids.clone();
    let t = Instant::now();
    let n = 2_000_000;
    for i in 0..n {
        black_box(engine.get(ids[i % ids.len()]).unwrap());
    }
    println!(
        "get on 3 layers, no holder: {:.1} ns",
        t.elapsed().as_nanos() as f64 / n as f64
    );
    println!(
        "\n| Holder | 1 thread (ns/get) | {READERS} readers + writer (ns/get per reader) | snapshots published | worst publish (µs) |"
    );
    println!("|---|---:|---:|---:|---:|");
    for _ in 0..2 {
        run::<Swap>(&engine, &ids);
        run::<Rw>(&engine, &ids);
        run::<RwClone>(&engine, &ids);
        run::<Mx>(&engine, &ids);
    }
}
