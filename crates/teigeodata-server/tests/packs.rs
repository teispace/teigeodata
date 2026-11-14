//! The pack set (phase 6.1): files and the store's packs behind one shared engine that follows
//! the journal, read without disturbing an install in progress (D-172).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests: failures panic with their message"
)]

#[path = "../../teigeodata-engine/tests/common/mod.rs"]
mod common;

use std::path::Path;

use teigeodata_engine::Components;
use teigeodata_server::packs::{PackError, Packs};

/// Writes a journal listing `files` (each in `packs/`, every component installed).
fn journal(root: &Path, files: &[&str]) {
    let set: Vec<String> = Components::all()
        .closed()
        .iter()
        .map(|c| format!("\"{}\"", c.name()))
        .collect();
    let packs: Vec<String> = files
        .iter()
        .map(|f| {
            let len = std::fs::metadata(root.join("packs").join(f)).unwrap().len();
            format!(
                "{{\"address\":\"{f}\",\"component\":\"places\",\"release\":\"2026.11.0\",\"set\":[{}],\"file\":\"{f}\",\"pack_len\":{len},\"head\":\"{}\",\"deps\":[],\"asked\":true,\"pinned\":false,\"used\":0}}",
                set.join(","),
                "00".repeat(16)
            )
        })
        .collect();
    let body = format!("{{\"version\":2,\"packs\":[{}]}}\n", packs.join(","));
    // As the store commits: a staged file renamed over the journal.
    std::fs::write(root.join("journal.new"), body).unwrap();
    std::fs::rename(root.join("journal.new"), root.join("journal")).unwrap();
}

#[test]
fn the_pack_set_follows_the_store_and_keeps_its_files() {
    let root = std::env::temp_dir().join(format!("tgd-server-packs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("packs")).unwrap();

    let model = common::synthetic(400, 5);
    let parts = model.split(2, |_, id| (id.get() % 2) as usize);
    let of = |p: u64| *model.ids.iter().find(|id| id.get() % 2 == p).unwrap();
    std::fs::write(root.join("packs/a.tgd"), &parts[0]).unwrap();
    std::fs::write(root.join("packs/b.tgd"), &parts[1]).unwrap();
    let own = root.join("own.tgd");
    std::fs::write(&own, common::synthetic(50, 77).encode()).unwrap();
    journal(&root, &["a.tgd"]);

    let (packs, opened) = Packs::open(std::slice::from_ref(&own), Some(&root)).unwrap();
    assert_eq!(opened.opened, [root.join("packs/a.tgd")]);
    assert_eq!(
        packs.load().layers().len(),
        2,
        "the file and the store's pack"
    );
    assert!(packs.load().get(of(0)).unwrap().is_some());

    // An install in progress elsewhere: its staged journal and unlisted pack stay untouched.
    std::fs::write(root.join("journal.tmp"), b"half").unwrap();
    std::fs::write(root.join("packs/c.tgd"), b"partial").unwrap();
    let same = packs.follow().unwrap();
    assert!(same.opened.is_empty() && same.closed.is_empty());
    assert!(root.join("journal.tmp").exists() && root.join("packs/c.tgd").exists());

    // B installed and A removed: a running query keeps A, the next one sees B.
    let running = packs.load();
    journal(&root, &["b.tgd"]);
    let f = packs.follow().unwrap();
    assert_eq!(
        (f.opened, f.closed),
        (
            vec![root.join("packs/b.tgd")],
            vec![root.join("packs/a.tgd")]
        )
    );
    assert!(
        running.get(of(0)).unwrap().is_some(),
        "the old snapshot keeps A"
    );
    let now = packs.load();
    assert!(now.get(of(0)).unwrap().is_none());
    assert!(now.get(of(1)).unwrap().is_some());
    assert_eq!(now.layers().len(), 2, "the file stays");
    drop((running, now));

    // A journal that does not parse: an error, and the engine stays as it was.
    std::fs::write(root.join("journal"), b"{").unwrap();
    let before = packs.shared().current();
    assert!(matches!(packs.follow(), Err(PackError::Store(..))));
    assert!(std::sync::Arc::ptr_eq(&before, &packs.shared().current()));

    // No store there: refused, never created.
    let none = root.join("no-store");
    assert!(matches!(
        Packs::open(&[], Some(&none)),
        Err(PackError::Store(..))
    ));
    assert!(!none.exists());

    // Files only: follow changes nothing.
    let (files, _) = Packs::open(std::slice::from_ref(&own), None).unwrap();
    assert_eq!(files.follow().unwrap().opened.len(), 0, "nothing to follow");
    assert!(matches!(
        Packs::open(&[root.join("missing.tgd")], None),
        Err(PackError::Pack(..))
    ));
    std::fs::remove_dir_all(&root).unwrap();
}
