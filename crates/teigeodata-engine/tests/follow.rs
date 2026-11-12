//! The snapshot swap on install (D-172): `Shared::follow` makes the engine match the store's
//! journal in one publish, while a query's snapshot keeps the packs it started with.

#![allow(
    clippy::unwrap_used,
    reason = "tests: failures panic with their message"
)]

mod common;

use std::sync::Arc;

use common::synthetic;
use teigeodata_engine::{Component, Components, Engine, Layer, LayerOptions, Parts, Shared};
use teigeodata_sync::store::{Dir, Record};

fn record(file: &str, len: usize, set: Components) -> Record {
    Record {
        address: file.into(),
        component: "places".into(),
        release: "2026.11.0".into(),
        set,
        parts: Parts::ALL,
        file: file.into(),
        pack_len: len as u64,
        head: [0; 16],
        deps: vec![],
        asked: true,
        pinned: false,
        used: 0,
    }
}

#[test]
fn the_engine_follows_the_journal() {
    let root = std::env::temp_dir().join(format!("tgd-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = Dir::new(&root).unwrap();
    std::fs::create_dir_all(dir.packs_dir()).unwrap();

    let model = synthetic(400, 5);
    let parts = model.split(2, |_, id| (id.get() % 2) as usize);
    let in_part = |p: usize| {
        model
            .ids
            .iter()
            .copied()
            .find(|id| (id.get() % 2) as usize == p)
            .unwrap()
    };
    let all = Components::all().closed();
    let pa = record("a.tgd", parts[0].len(), all);
    let pb = record("b.tgd", parts[1].len(), all);
    std::fs::write(dir.pack_path(&pa), &parts[0]).unwrap();
    std::fs::write(dir.pack_path(&pb), &parts[1]).unwrap();

    // Layers the app opened itself, in memory and from a file outside `packs/`: follow never
    // touches them.
    let mut first = Engine::new();
    first.add(Layer::from_bytes(synthetic(50, 77).encode(), LayerOptions::default()).unwrap());
    let own = root.join("own.tgd");
    std::fs::write(&own, synthetic(60, 78).encode()).unwrap();
    first.add(Layer::open(&own, LayerOptions::default()).unwrap());
    let shared = Shared::new(first);

    let got = shared.follow(&dir, std::slice::from_ref(&pa), LayerOptions::default());
    assert_eq!(got.opened, [dir.pack_path(&pa)]);
    assert!(got.closed.is_empty() && got.failed.is_empty());
    assert_eq!(shared.load().layers().len(), 3);
    assert!(shared.load().get(in_part(0)).unwrap().is_some());

    // The same journal again: nothing changes and nothing is published.
    let before = shared.current();
    let got = shared.follow(&dir, std::slice::from_ref(&pa), LayerOptions::default());
    assert!(got.opened.is_empty() && got.closed.is_empty());
    assert!(
        Arc::ptr_eq(&before, &shared.current()),
        "a publish with no change"
    );

    // A replaced by B: a running query keeps A, the next one sees B; A's file may go.
    let old = shared.load();
    let got = shared.follow(&dir, std::slice::from_ref(&pb), LayerOptions::default());
    assert_eq!(got.opened, [dir.pack_path(&pb)]);
    assert_eq!(got.closed, [dir.pack_path(&pa)]);
    std::fs::remove_file(dir.pack_path(&pa)).unwrap();
    assert!(
        old.get(in_part(0)).unwrap().is_some(),
        "the old snapshot keeps A"
    );
    let now = shared.load();
    assert!(now.get(in_part(0)).unwrap().is_none());
    assert!(now.get(in_part(1)).unwrap().is_some());
    assert_eq!(now.layers().len(), 3, "B and the app's own layers");
    drop((old, now));

    // A damaged file is reported and left out; the rest still opens, with its installed set.
    let pc = record("c.tgd", 9, all);
    std::fs::write(dir.pack_path(&pc), b"not a pack").unwrap();
    let points = Components::of(&[Component::Point]).closed();
    let b_points = Record {
        set: points,
        ..pb.clone()
    };
    // B with a smaller set is the same file: kept as open (a set change is a new file, D-171).
    let got = shared.follow(&dir, &[b_points, pc], LayerOptions::default());
    assert!(got.opened.is_empty() && got.closed.is_empty());
    assert_eq!(got.failed.len(), 1);
    assert_eq!(got.failed[0].0, dir.pack_path(&record("c.tgd", 9, all)));
    let snap = shared.load();
    let installed = snap
        .layers()
        .find(|l| l.path() == Some(dir.pack_path(&pb).as_path()))
        .and_then(|l| l.options().installed);
    assert_eq!(installed, Some(all));

    // An empty journal drops every store layer and nothing else.
    let got = shared.follow(&dir, &[], LayerOptions::default());
    assert_eq!(got.closed, [dir.pack_path(&pb)]);
    assert_eq!(shared.load().layers().len(), 2, "the app's own layers stay");
    std::fs::remove_dir_all(&root).unwrap();
}

/// A repair renames a new file over the same name: follow drops the old mapping and opens the
/// new file, while a running query keeps the old one.
#[test]
fn a_replaced_file_is_opened_again() {
    let root = std::env::temp_dir().join(format!("tgd-follow-re-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = Dir::new(&root).unwrap();
    std::fs::create_dir_all(dir.packs_dir()).unwrap();
    let model = synthetic(300, 11);
    let parts = model.split(2, |_, id| (id.get() % 2) as usize);
    let all = Components::all().closed();
    let pa = record("a.tgd", parts[0].len(), all);
    let path = dir.pack_path(&pa);
    std::fs::write(&path, &parts[0]).unwrap();
    let shared = Shared::new(Engine::new());
    shared.follow(&dir, std::slice::from_ref(&pa), LayerOptions::default());
    let first = model
        .ids
        .iter()
        .copied()
        .find(|id| id.get() % 2 == 0)
        .unwrap();
    let second = model
        .ids
        .iter()
        .copied()
        .find(|id| id.get() % 2 == 1)
        .unwrap();
    let old = shared.load();
    // The repaired file, renamed over the old name (other bytes, so the change shows).
    let tmp = root.join("staging.part");
    std::fs::write(&tmp, &parts[1]).unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    let got = shared.follow(&dir, std::slice::from_ref(&pa), LayerOptions::default());
    assert_eq!(got.closed, std::slice::from_ref(&path));
    assert_eq!(got.opened, std::slice::from_ref(&path));
    assert!(
        old.get(first).unwrap().is_some(),
        "the old mapping still answers"
    );
    let now = shared.load();
    assert_eq!(now.layers().len(), 1);
    assert!(now.get(second).unwrap().is_some(), "the new file answers");
    drop((old, now));
    // And once open, the same journal changes nothing.
    let got = shared.follow(&dir, std::slice::from_ref(&pa), LayerOptions::default());
    assert!(got.opened.is_empty() && got.closed.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}
