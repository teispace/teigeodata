//! M1 for the engine: a warm query writes into caller-owned scratch and output buffers and does
//! not allocate. `get`, `reverse`, `admin_at`, `tz_at` and `within_bbox` allocate nothing;
//! `search` allocates only its folded query (`teigeodata_names::fold` returns a `String`).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::synthetic;
use teigeodata_engine::{Engine, Layer, LayerOptions, Rect, Scratch};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

// SAFETY: forwards to the system allocator; only counts.
#[allow(unsafe_code, reason = "a counting global allocator for the M1 test")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.with(Cell::get) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The allocations `f` makes on this thread.
fn count(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

#[test]
fn warm_queries_do_not_allocate() {
    // The counter sees an allocation, so a zero below means none.
    assert_eq!(
        count(|| drop(std::hint::black_box(Vec::<u8>::with_capacity(9)))),
        1
    );
    let model = synthetic(2000, 21);
    let whole = model.encode();
    let parts = model.split(3, |r, _| r as usize % 3);
    for packs in [vec![whole], parts] {
        let mut e = Engine::new();
        for p in packs {
            e.add(Layer::from_bytes(p, LayerOptions::default()).unwrap());
        }
        let mut s = Scratch::default();
        let (mut near, mut admin, mut hits) = (
            Vec::with_capacity(64),
            Vec::with_capacity(64),
            Vec::with_capacity(64),
        );
        let points: Vec<_> = model.points.iter().step_by(97).copied().collect();
        let queries = ["kamapur", "Ramkot", "kam", "dhara pani", "ramkat"];
        let run = |e: &Engine, s: &mut Scratch, near: &mut Vec<_>, admin: &mut Vec<_>| {
            for (i, &at) in points.iter().enumerate() {
                let id = model.ids[i * 7 % model.ids.len()];
                assert!(e.get(id).unwrap().is_some());
                e.reverse(at, 10, s, near).unwrap();
                e.admin_at(at.to_e7(), s, admin).unwrap();
                e.tz_at(at.to_e7()).unwrap();
                let rect = Rect {
                    min_lat: at.lat() - 50_000,
                    max_lat: at.lat() + 50_000,
                    min_lon: at.lon() - 50_000,
                    max_lon: at.lon() + 50_000,
                };
                e.within_bbox(rect, 20, s, admin).unwrap();
            }
        };
        // Warm: first use verifies sections and grows the scratch.
        run(&e, &mut s, &mut near, &mut admin);
        for q in queries {
            e.search(q, 20, &mut s, &mut hits).unwrap();
        }
        assert_eq!(count(|| run(&e, &mut s, &mut near, &mut admin)), 0);
        for q in queries {
            let n = count(|| e.search(q, 20, &mut s, &mut hits).unwrap());
            assert!(n <= 1, "{q}: {n} allocations");
        }
    }
}

/// The same budget on a real pack with query rules (D-120): unit suffixes, native generics,
/// droppable suffixes, a context and the phonetic tier. `TGD_PACK=<.tgd> cargo test --release
/// -p teigeodata-engine --test alloc -- --ignored`.
#[test]
#[ignore = "needs TGD_PACK, a real pack with query rules"]
fn warm_search_on_a_real_pack_allocates_only_the_folds() {
    let path = std::env::var("TGD_PACK").expect("set TGD_PACK to a .tgd file");
    let mut e = Engine::new();
    e.add(Layer::open(std::path::Path::new(&path), LayerOptions::default()).unwrap());
    let mut s = Scratch::default();
    let mut hits = Vec::with_capacity(64);
    let queries = [
        "Kathmandu",
        "Janaki RM",
        "Bhimsenगाउँ",
        "Janakpurdham",
        "piuthan",
        "Athmandu",
        "Rangapur, Rautahat",
    ];
    for q in queries {
        e.search(q, 20, &mut s, &mut hits).unwrap();
        assert!(!hits.is_empty(), "{q}: no answer");
    }
    for q in queries {
        // The budget is what folding the name and the context allocates; search adds none.
        let budget = count(|| {
            for part in q.splitn(2, ',') {
                drop(std::hint::black_box(teigeodata_names::fold(part)));
            }
        });
        let n = count(|| e.search(q, 20, &mut s, &mut hits).unwrap());
        assert!(n <= budget, "{q}: {n} allocations, the folds {budget}");
    }
}
