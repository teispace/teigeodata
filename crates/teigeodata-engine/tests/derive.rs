//! A device builds SUBSTRING instead of downloading it (D-213): on the real Nepal pack, a store
//! holding the pack without SUBSTRING derives it, the file equals the shipped pack byte for
//! byte, and a substring query answers from it.
//!
//! `TGD_PACK=<np-places-lite.tgd> cargo test --release -p teigeodata-engine --features derive,store --test derive -- --ignored`

#![cfg(all(feature = "derive", feature = "store"))]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::print_stdout,
    reason = "tests: failures panic with their message; the timing is printed"
)]

use std::time::Instant;

use teigeodata_engine::{Component, Components, Engine, Kind, Layer, LayerOptions, Scratch};
use teigeodata_format::Pack;
use teigeodata_sync::store::{Dir, Store};

#[test]
#[ignore = "needs TGD_PACK, a built pack with SUBSTRING"]
fn a_store_derives_substring_and_it_is_the_shipped_section() {
    let full = std::fs::read(std::env::var("TGD_PACK").unwrap()).unwrap();
    let pack = Pack::open(&full).unwrap();
    let all = Components::all().closed();
    let set = all
        .without(Components::of(&[Component::Substring]))
        .closed();
    assert!(!set.contains(Component::Substring));
    let tag = |s: Components| {
        if s == all {
            "all".to_owned()
        } else {
            s.iter().map(Component::name).collect::<Vec<_>>().join("+")
        }
    };
    let root = std::env::temp_dir().join(format!("tgd-derive-np-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("packs")).unwrap();
    let file = format!("np.places.2026.11.0.{}.tgd", tag(set));
    let mut bytes = full.clone();
    let s = pack
        .head()
        .sections()
        .find(|s| s.kind == Kind::SUBSTRING)
        .unwrap();
    bytes[s.offset as usize..(s.offset + s.length) as usize].fill(0);
    std::fs::write(root.join("packs").join(&file), &bytes).unwrap();
    let names: Vec<String> = set.iter().map(|c| format!("\"{}\"", c.name())).collect();
    let journal = format!(
        "{{\"version\":2,\"packs\":[{{\"address\":\"np\",\"component\":\"places\",\"release\":\"2026.11.0\",\"set\":[{}],\"file\":\"{file}\",\"pack_len\":{},\"head\":\"{}\",\"deps\":[],\"asked\":true,\"pinned\":false,\"used\":0}}]}}\n",
        names.join(","),
        bytes.len(),
        "00".repeat(16)
    );
    std::fs::write(root.join("journal"), journal).unwrap();
    drop(bytes);

    let (mut store, _) = Store::open(Dir::new(&root).unwrap()).unwrap();
    let t = Instant::now();
    teigeodata_engine::derive::derive_installed(&mut store, "np", "places").unwrap();
    let took = t.elapsed();
    let r = store.packs()[0].clone();
    assert!(r.set.contains(Component::Substring));
    let path = root.join("packs").join(&r.file);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        full,
        "byte for byte the shipped pack"
    );
    println!("derived and installed in {took:?}: {}", r.file);

    let mut e = Engine::new();
    let opts = LayerOptions {
        installed: Some(r.set),
        ..LayerOptions::default()
    };
    e.add(Layer::open(&path, opts).unwrap());
    let (mut scratch, mut hits) = (Scratch::default(), Vec::new());
    e.search("mandu", 5, &mut scratch, &mut hits).unwrap();
    assert!(
        !hits.is_empty(),
        "a substring query answers from the derived section"
    );
    let _ = std::fs::remove_dir_all(root);
}
