//! `Shared` (D-141): a query sees a whole snapshot, never half an update, and a snapshot keeps
//! its packs answering after they are removed from the current engine.

#![allow(
    clippy::unwrap_used,
    reason = "tests: failures panic with their message"
)]

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use common::synthetic;
use teigeodata_engine::{Engine, Layer, LayerOptions, Shared};

#[test]
fn readers_never_see_half_an_update() {
    // Two packs with disjoint IDs; every update swaps one for the other in one step.
    let model = synthetic(400, 3);
    let parts = model.split(2, |_, id| (id.get() % 2) as usize);
    let in_part = |p: usize| {
        model
            .ids
            .iter()
            .copied()
            .find(|id| (id.get() % 2) as usize == p)
            .unwrap()
    };
    let (a_id, b_id) = (in_part(0), in_part(1));
    let open = |p: usize| Layer::from_bytes(parts[p].clone(), LayerOptions::default()).unwrap();
    let mut first = Engine::new();
    let a = first.add(open(0));
    let shared = Shared::new(first);
    let stop = AtomicBool::new(false);
    let seen = AtomicUsize::new(0);
    thread::scope(|sc| {
        for _ in 0..4 {
            sc.spawn(|| {
                let mut n = 0;
                while !stop.load(Ordering::Relaxed) {
                    let e = shared.load();
                    let has_a = e.get(a_id).unwrap().is_some();
                    let has_b = e.get(b_id).unwrap().is_some();
                    assert!(has_a != has_b, "a snapshot with {has_a} and {has_b}");
                    assert_eq!(e.layers().len(), 1);
                    n += 1;
                }
                seen.fetch_add(n, Ordering::Relaxed);
            });
        }
        let mut current = (a, 0);
        for _ in 0..2_000 {
            let other = 1 - current.1;
            let id = shared.update(|e| {
                e.remove(current.0).unwrap();
                e.add(open(other))
            });
            current = (id, other);
        }
        stop.store(true, Ordering::Relaxed);
    });
    assert!(seen.load(Ordering::Relaxed) > 1_000);
}

#[test]
fn a_snapshot_keeps_its_packs_after_they_are_removed() {
    let model = synthetic(200, 9);
    let mut e = Engine::new();
    let id = e.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
    let shared = Shared::new(e);
    let old = shared.current();
    let removed = shared.update(|e| e.remove(id)).unwrap();
    assert_eq!(shared.load().layers().len(), 0);
    assert!(shared.load().get(model.ids[0]).unwrap().is_none());
    // The old snapshot still answers; the layer lives while either handle does.
    assert!(old.get(model.ids[0]).unwrap().is_some());
    drop(removed);
    assert!(old.get(model.ids[5]).unwrap().is_some());
    // A failed update publishes nothing.
    let r: Result<(), &str> = shared.try_update(|e| {
        e.add(Layer::from_bytes(model.encode(), LayerOptions::default()).unwrap());
        Err("refused")
    });
    assert_eq!(r, Err("refused"));
    assert_eq!(shared.load().layers().len(), 0);
}
