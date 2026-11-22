//! The store policy's core (D-369) over an in-memory store and a signed catalog: dependencies
//! first, each once, a cycle cut, what is installed as asked skipped, and an update.

use std::collections::BTreeMap;
use std::rc::Rc;

use teigeodata_format::component::Selection;
use teigeodata_sync::catalog::Entry;
use teigeodata_sync::fetch::Mirrors;
use teigeodata_sync::meta::block_on;
use teigeodata_sync::policy::{self, ClientError, Outcome, Packs, Want};
use teigeodata_sync::repo::{
    self, TargetFile, sign, snapshot_listing, targets_delegating, timestamp,
};
use teigeodata_sync::store::{Record, Store};
use teigeodata_sync::trust::Trusted;

use super::install_tests::{Mem, pack_at};
use super::refresh_tests::{NOW, YEAR, key, root_file};
use super::store_tests::MemFs;

/// A catalog at `release` of packs (address, deps): the trusted metadata and each asset.
fn catalog(release: &str, packs: &[(&str, &[&str])]) -> (Trusted, BTreeMap<String, Vec<u8>>) {
    let mut files = BTreeMap::new();
    let mut assets = BTreeMap::new();
    for (address, deps) in packs {
        let (mut e, asset, _) = super::entry_of(&pack_at(address, release)).unwrap();
        e.deps = deps.iter().map(|d| format!("{d}.places.tgd")).collect();
        files.insert(
            e.path(),
            TargetFile {
                length: asset.len() as u64,
                hashes: repo::target_hashes(&asset),
                custom: Some(e.to_custom()),
            },
        );
        assets.insert(e.file.clone(), asset);
    }
    let tg = sign(
        &targets_delegating(1, NOW + YEAR, &files, &[]).unwrap(),
        &[&key(30)],
    )
    .unwrap();
    let sn = sign(
        &snapshot_listing(1, NOW + YEAR, &[("targets", &tg, 1)]).unwrap(),
        &[&key(20)],
    )
    .unwrap();
    let ts = sign(&timestamp(1, NOW + YEAR, &sn, 1).unwrap(), &[&key(10)]).unwrap();
    let mut t = Trusted::new(&root_file(1, 10), NOW).unwrap();
    t.update_timestamp(&ts).unwrap();
    t.update_snapshot(&sn, false).unwrap();
    t.update_targets(&tg).unwrap();
    (t, assets)
}

/// A store whose catalog is `t` and whose mirror serves `assets`; what it installs and marks.
struct Fake {
    store: Store<MemFs>,
    t: Trusted,
    assets: BTreeMap<String, Vec<u8>>,
    offline: bool,
    log: Vec<String>,
}

impl Fake {
    fn new(t: Trusted, assets: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            store: Store::open(MemFs(Rc::default())).unwrap().0,
            t,
            assets,
            offline: false,
            log: Vec::new(),
        }
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "written as a driver that awaits would write it"
)]
impl Packs for Fake {
    fn packs(&self) -> &[Record] {
        self.store.packs()
    }

    async fn refresh(&mut self, _: &[String]) -> policy::Result<Trusted> {
        if self.offline {
            return Err(ClientError::Network("offline".into()));
        }
        Ok(self.t.clone())
    }

    async fn offline(&mut self) -> policy::Result<Trusted> {
        Ok(self.t.clone())
    }

    async fn install(
        &mut self,
        entry: &Entry,
        asset_len: u64,
        set: Selection,
    ) -> policy::Result<Outcome> {
        self.log.push(format!("install {}", entry.address));
        let asset = &self.assets[&entry.file];
        let mut host = Mem::new(asset, None);
        let done = self.store.install(
            &mut host,
            &mut Mirrors::new(["m/"]),
            &entry.file,
            entry,
            asset_len,
            set,
        )?;
        Ok(Outcome::Installed(done))
    }

    async fn mark(&mut self, address: &str, component: &str, asked: bool) -> policy::Result<()> {
        self.log.push(format!("mark {address} {asked}"));
        Ok(self.store.mark(address, component, asked)?)
    }

    async fn pin(&mut self, address: &str, component: &str, on: bool) -> policy::Result<()> {
        self.log.push(format!("pin {address} {on}"));
        Ok(self.store.pin(address, component, on)?)
    }
}

fn done(r: &[policy::Done]) -> Vec<(String, bool)> {
    r.iter()
        .map(|d| (d.path.clone(), d.installed.is_some()))
        .collect()
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|a| format!("{a}.places.tgd")).collect()
}

#[test]
fn dependencies_install_first_once_each_marked_and_a_cycle_ends() {
    let (t, assets) = catalog(
        "2026.11.0",
        &[
            ("aa", &["bb", "cc"]),
            ("bb", &["cc"]),
            ("cc", &[]),
            ("dd", &["cc"]),
            ("ee", &["ff"]),
            ("ff", &["ee"]),
        ],
    );
    let mut f = Fake::new(t, assets);
    let want = Want::default();

    // aa needs bb and cc, bb needs cc: cc first, once; the two dependencies are marked.
    let r = block_on(policy::install(&mut f, &paths(&["aa"]), &want, true)).unwrap();
    assert_eq!(
        done(&r.done),
        [
            ("cc.places.tgd".into(), true),
            ("bb.places.tgd".into(), true),
            ("aa.places.tgd".into(), true)
        ]
    );
    assert_eq!(
        f.log,
        [
            "install cc",
            "mark cc false",
            "install bb",
            "mark bb false",
            "install aa",
            "pin aa true"
        ]
    );
    let rec = |f: &Fake, a: &str| f.packs().iter().find(|r| r.address == a).unwrap().clone();
    assert!(rec(&f, "aa").asked && rec(&f, "aa").pinned);
    assert!(!rec(&f, "cc").asked && !rec(&f, "cc").pinned);

    // Again: nothing fetched, the pack reported as installed; dependencies held are skipped.
    f.log.clear();
    let r = block_on(policy::install(&mut f, &paths(&["aa", "dd"]), &want, false)).unwrap();
    assert_eq!(
        done(&r.done),
        [
            ("aa.places.tgd".into(), false),
            ("dd.places.tgd".into(), true)
        ]
    );
    assert_eq!(f.log, ["install dd"]);

    // A cycle ends: ff is ee's dependency, ee is not taken again as ff's.
    f.log.clear();
    let r = block_on(policy::install(&mut f, &paths(&["ee"]), &want, false)).unwrap();
    assert_eq!(
        done(&r.done),
        [
            ("ff.places.tgd".into(), true),
            ("ee.places.tgd".into(), true)
        ]
    );
}

#[test]
fn a_stale_catalog_names_the_network_and_an_update_takes_a_new_release() {
    let (t, assets) = catalog("2026.11.0", &[("aa", &[])]);
    let mut f = Fake::new(t, assets);
    let want = Want::default();

    // The network down, a pack the stored catalog lacks: the error is the network's.
    f.offline = true;
    let e = block_on(policy::install(&mut f, &paths(&["zz"]), &want, false)).unwrap_err();
    assert!(
        matches!(&e, ClientError::Network(m) if m.contains("zz.places.tgd is not in the stored catalog")),
        "{e}"
    );
    // One it has installs from the stored catalog, and the report says it was stale.
    let r = block_on(policy::install(&mut f, &paths(&["aa"]), &want, false)).unwrap();
    assert_eq!(r.stale_catalog.as_deref(), Some("offline"));
    assert_eq!(done(&r.done), [("aa.places.tgd".into(), true)]);

    // An update: nothing to do at the same release; the new release installs.
    f.offline = false;
    assert!(block_on(policy::update(&mut f, &[])).unwrap().is_empty());
    let (t2, assets2) = catalog("2026.12.0", &[("aa", &[])]);
    f.t = t2;
    f.assets = assets2;
    let up = block_on(policy::update(&mut f, &[])).unwrap();
    assert_eq!(done(&up), [("aa.places.tgd".into(), true)]);
    assert_eq!(f.packs()[0].release, "2026.12.0");
    // Not installed: refused.
    let e = block_on(policy::update(&mut f, &paths(&["bb"]))).unwrap_err();
    assert!(matches!(e, ClientError::Refused(_)), "{e}");
}
