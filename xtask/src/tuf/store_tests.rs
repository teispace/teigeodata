//! The store (D-171) against a file system that crashes: at every step of an install, with
//! unsynced data lost or kept, recovery leaves the old or the new pack installed, never a mix,
//! and no orphan file (gate 8 of the sync gates).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::rc::Rc;

use teigeodata_format::component::Components;
use teigeodata_sync::catalog::Entry;
use teigeodata_sync::fetch::Mirrors;
use teigeodata_sync::store::{Fs, Record, Room, Store, StoreError, StoreFile, plan_room};

use super::install_tests::{Mem, pack_at, pack_of};

/// What a crash does to data not yet synced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Crash {
    /// Lost: files hold what was last synced, directories what was last synced.
    Lose,
    /// Kept: everything written reached the disk.
    Keep,
}

struct Inode {
    data: Vec<u8>,
    synced: Vec<u8>,
}

/// A disk with POSIX-like durability: file data is durable at `sync`, a directory's entries at
/// `sync_dir` of that directory.
#[derive(Default)]
struct Disk {
    inodes: Vec<Inode>,
    live: BTreeMap<String, usize>,
    durable: BTreeMap<String, usize>,
    dirs: BTreeSet<String>,
    dirs_durable: BTreeSet<String>,
    ops: usize,
    crash_at: Option<usize>,
    crashed: bool,
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn crashed() -> io::Error {
    io::Error::other("crashed")
}

impl Disk {
    /// One step that changes the disk; the crash comes before step `crash_at`.
    fn step(&mut self) -> io::Result<()> {
        if self.crashed || self.crash_at == Some(self.ops) {
            self.crashed = true;
            return Err(crashed());
        }
        self.ops += 1;
        Ok(())
    }

    fn reboot(&mut self, how: Crash) {
        if how == Crash::Lose {
            self.live = self.durable.clone();
            self.dirs = self.dirs_durable.clone();
            for i in &mut self.inodes {
                i.data = i.synced.clone();
            }
        }
        self.crash_at = None;
        self.crashed = false;
    }
}

#[derive(Clone)]
struct MemFs(Rc<RefCell<Disk>>);

struct MemFile {
    disk: Rc<RefCell<Disk>>,
    ino: usize,
    pos: u64,
}

impl Read for MemFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let d = self.disk.borrow();
        if d.crashed {
            return Err(crashed());
        }
        let data = &d.inodes[self.ino].data;
        let at = usize::try_from(self.pos).unwrap().min(data.len());
        let n = buf.len().min(data.len() - at);
        buf[..n].copy_from_slice(&data[at..at + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut d = self.disk.borrow_mut();
        d.step()?;
        let data = &mut d.inodes[self.ino].data;
        let at = usize::try_from(self.pos).unwrap();
        if data.len() < at + buf.len() {
            data.resize(at + buf.len(), 0);
        }
        data[at..at + buf.len()].copy_from_slice(buf);
        self.pos += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for MemFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let len = self.disk.borrow().inodes[self.ino].data.len() as u64;
        self.pos = match to {
            SeekFrom::Start(p) => p,
            SeekFrom::End(o) => len.checked_add_signed(o).unwrap(),
            SeekFrom::Current(o) => self.pos.checked_add_signed(o).unwrap(),
        };
        Ok(self.pos)
    }
}

impl StoreFile for MemFile {
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        let mut d = self.disk.borrow_mut();
        d.step()?;
        d.inodes[self.ino]
            .data
            .resize(usize::try_from(len).unwrap(), 0);
        Ok(())
    }
    fn sync(&mut self) -> io::Result<()> {
        let mut d = self.disk.borrow_mut();
        d.step()?;
        let i = &mut d.inodes[self.ino];
        i.synced = i.data.clone();
        Ok(())
    }
}

impl Fs for MemFs {
    type File = MemFile;

    fn open(&mut self, path: &str) -> io::Result<MemFile> {
        let mut d = self.0.borrow_mut();
        let ino = if let Some(&i) = d.live.get(path) {
            if d.crashed {
                return Err(crashed());
            }
            i
        } else {
            d.step()?;
            let dir = parent(path);
            if !dir.is_empty() && !d.dirs.contains(dir) {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            d.inodes.push(Inode {
                data: Vec::new(),
                synced: Vec::new(),
            });
            let i = d.inodes.len() - 1;
            d.live.insert(path.to_owned(), i);
            i
        };
        Ok(MemFile {
            disk: Rc::clone(&self.0),
            ino,
            pos: 0,
        })
    }

    fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>> {
        let d = self.0.borrow();
        Ok(d.live.get(path).map(|&i| d.inodes[i].data.clone()))
    }

    fn len(&mut self, path: &str) -> io::Result<Option<u64>> {
        let d = self.0.borrow();
        Ok(d.live.get(path).map(|&i| d.inodes[i].data.len() as u64))
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        let mut d = self.0.borrow_mut();
        d.step()?;
        let i = d
            .live
            .remove(from)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        d.live.insert(to.to_owned(), i);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> io::Result<()> {
        let mut d = self.0.borrow_mut();
        d.step()?;
        d.live.remove(path);
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> io::Result<()> {
        let mut d = self.0.borrow_mut();
        d.step()?;
        d.dirs.insert(path.to_owned());
        Ok(())
    }

    fn sync_dir(&mut self, path: &str) -> io::Result<()> {
        let mut d = self.0.borrow_mut();
        d.step()?;
        let d = &mut *d;
        d.durable.retain(|p, _| parent(p) != path);
        for (p, &i) in &d.live {
            if parent(p) == path {
                d.durable.insert(p.clone(), i);
            }
        }
        if path.is_empty() {
            d.dirs_durable = d.dirs.clone();
        }
        Ok(())
    }

    fn list(&mut self, path: &str) -> io::Result<Vec<String>> {
        let d = self.0.borrow();
        if d.crashed {
            return Err(crashed());
        }
        Ok(d.live
            .keys()
            .filter(|p| parent(p) == path)
            .map(|p| p.rsplit('/').next().unwrap().to_owned())
            .collect())
    }
}

/// A store's packs: (release, file bytes), and the files in `packs/`.
fn state(fs: &MemFs) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let (mut store, rec) = Store::open(fs.clone()).unwrap();
    assert!(rec.damaged.is_empty(), "{rec:?}");
    let packs: Vec<(String, Vec<u8>)> = store
        .packs()
        .to_vec()
        .iter()
        .map(|r| {
            let bytes = store
                .fs()
                .read(&Store::<MemFs>::path_of(r))
                .unwrap()
                .unwrap();
            (r.release.clone(), bytes)
        })
        .collect();
    let files = store.fs().list("packs").unwrap();
    (packs, files)
}

#[test]
fn the_store_survives_a_crash_at_every_step() {
    let (a, b) = (pack_of("2026.11.0"), pack_of("2026.12.0"));
    let (ea, xa, _) = super::entry_of(&a).unwrap();
    let (eb, xb, _) = super::entry_of(&b).unwrap();
    let all = Components::all();
    let install = |fs: &MemFs, e: &Entry, x: &[u8]| {
        let mut store = Store::open(fs.clone())?.0;
        let mut host = Mem::new(x, None);
        store
            .install(
                &mut host,
                &mut Mirrors::new(["m/"]),
                "a",
                e,
                x.len() as u64,
                all,
            )
            .map_err(|e| io::Error::other(e.to_string()))
    };
    // A, installed and then the power cut with everything unsynced lost: A is there.
    let fresh = || {
        let fs = MemFs(Rc::default());
        install(&fs, &ea, &xa).unwrap();
        fs.0.borrow_mut().reboot(Crash::Lose);
        fs
    };
    let fs = fresh();
    let (packs, files) = state(&fs);
    assert_eq!(packs.len(), 1);
    assert!(
        packs[0].0 == "2026.11.0" && packs[0].1 == a,
        "A is durable once installed"
    );
    assert_eq!(files.len(), 1);
    // The steps of installing B over A.
    let before = fs.0.borrow().ops;
    install(&fs, &eb, &xb).unwrap();
    let steps = fs.0.borrow().ops - before;
    assert!(steps > 10, "{steps}");

    let mut seen = BTreeSet::new();
    for how in [Crash::Lose, Crash::Keep] {
        for k in 0..steps {
            let fs = fresh();
            let at = fs.0.borrow().ops + k;
            fs.0.borrow_mut().crash_at = Some(at);
            // After the commit, a crash removing the old file is not the install's failure.
            let committed = install(&fs, &eb, &xb).is_ok();
            fs.0.borrow_mut().reboot(how);
            // Old or new, whole, and nothing else in packs/.
            let (packs, files) = state(&fs);
            assert_eq!(packs.len(), 1, "{how:?} {k}");
            let (release, bytes) = &packs[0];
            let want = if release == "2026.11.0" { &a } else { &b };
            assert!(bytes == want, "{how:?} {k}: {release} is not whole");
            assert_eq!(files.len(), 1, "{how:?} {k}: {files:?}");
            assert!(
                !committed || release == "2026.12.0",
                "{how:?} {k}: Ok, yet not B"
            );
            seen.insert(release.clone());
            // And the install then completes, leaving B alone.
            install(&fs, &eb, &xb).unwrap();
            fs.0.borrow_mut().reboot(Crash::Lose);
            let (packs, files) = state(&fs);
            assert!(
                packs.len() == 1 && packs[0].1 == b,
                "{how:?} {k}: B after retry"
            );
            assert_eq!(files.len(), 1, "{how:?} {k}: {files:?}");
        }
    }
    assert_eq!(
        seen.len(),
        2,
        "crashes before and after the commit both happen"
    );
}

#[test]
fn a_journal_that_does_not_parse_removes_nothing() {
    let a = pack_of("2026.11.0");
    let (ea, xa, _) = super::entry_of(&a).unwrap();
    let fs = MemFs(Rc::default());
    let mut store = Store::open(fs.clone()).unwrap().0;
    let mut host = Mem::new(&xa, None);
    store
        .install(
            &mut host,
            &mut Mirrors::new(["m/"]),
            "a",
            &ea,
            xa.len() as u64,
            Components::all(),
        )
        .unwrap();
    let mut f = fs.clone().open("journal").unwrap();
    f.write_all(b"[").unwrap();
    let Err(err) = Store::open(fs.clone()) else {
        panic!("a broken journal opened")
    };
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        fs.clone().list("packs").unwrap().len(),
        1,
        "nothing removed"
    );
}

/// The real file system: installs, replaces, and recovers from leftovers.
#[test]
fn a_real_directory_installs_replaces_and_recovers() {
    use teigeodata_sync::store::Dir;
    let root = std::env::temp_dir().join(format!("tgd-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (a, b) = (pack_of("2026.11.0"), pack_of("2026.12.0"));
    let (ea, xa, _) = super::entry_of(&a).unwrap();
    let (eb, xb, _) = super::entry_of(&b).unwrap();
    let all = Components::all();
    let mut store = Store::open(Dir::new(&root).unwrap()).unwrap().0;
    let mut host = Mem::new(&xa, None);
    store
        .install(
            &mut host,
            &mut Mirrors::new(["m/"]),
            "a",
            &ea,
            xa.len() as u64,
            all,
        )
        .unwrap();
    let path_a = root.join(Store::<Dir>::path_of(&store.packs()[0]));
    assert!(std::fs::read(&path_a).unwrap() == a, "A on disk");
    // Leftovers of a crash: an orphan pack and a journal never committed.
    std::fs::write(
        root.join("packs/zz.places.2026.10.0.00000000.all.tgd"),
        b"old",
    )
    .unwrap();
    std::fs::write(root.join("journal.tmp"), b"half").unwrap();
    let (mut store, rec) = Store::open(Dir::new(&root).unwrap()).unwrap();
    assert_eq!(rec.removed, ["zz.places.2026.10.0.00000000.all.tgd"]);
    assert!(rec.damaged.is_empty(), "{rec:?}");
    assert!(!root.join("journal.tmp").exists());
    // B replaces A: one record, A's file gone, B's whole.
    let mut host = Mem::new(&xb, None);
    store
        .install(
            &mut host,
            &mut Mirrors::new(["m/"]),
            "a",
            &eb,
            xb.len() as u64,
            all,
        )
        .unwrap();
    assert_eq!(store.packs().len(), 1);
    assert!(!path_a.exists(), "A retired");
    let path_b = root.join(Store::<Dir>::path_of(&store.packs()[0]));
    assert!(std::fs::read(&path_b).unwrap() == b, "B on disk");
    assert_eq!(std::fs::read_dir(root.join("staging")).unwrap().count(), 0);
    // A truncated pack is reported for repair, not removed.
    std::fs::File::options()
        .write(true)
        .open(&path_b)
        .unwrap()
        .set_len(10)
        .unwrap();
    let rec = Store::open(Dir::new(&root).unwrap()).unwrap().1;
    assert_eq!(rec.damaged.len(), 1);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A test pack at `address` depending on `deps`: its entry, asset and bytes.
fn packed(address: &str, deps: &[&str]) -> (Entry, Vec<u8>, Vec<u8>) {
    let pack = pack_at(address, "2026.11.0");
    let (mut e, x, _) = super::entry_of(&pack).unwrap();
    e.deps = deps.iter().map(|d| (*d).to_owned()).collect();
    (e, x, pack)
}

fn put(store: &mut Store<MemFs>, e: &Entry, asset: &[u8]) {
    let mut host = Mem::new(asset, None);
    store
        .install(
            &mut host,
            &mut Mirrors::new(["m/"]),
            "a",
            e,
            asset.len() as u64,
            Components::all(),
        )
        .unwrap();
}

fn addresses(store: &Store<MemFs>) -> Vec<String> {
    store.packs().iter().map(|r| r.address.clone()).collect()
}

/// Removal, dependencies, pins, marks, last use and making room, on one store; the journal
/// keeps them across a reopen.
#[test]
fn the_storage_manager_keeps_what_is_pinned_needed_or_kept() {
    let comp = "places".to_owned();
    let dep = |addr: &str| format!("{addr}.{comp}.tgd");
    let fs = MemFs(Rc::default());
    let mut store = Store::open(fs.clone()).unwrap().0;
    // `city` needs `land`; `old` and `pin` stand alone.
    let land = packed("land", &[]);
    let city = packed("city", &[&dep("land")]);
    let all4 = [
        land.clone(),
        city.clone(),
        packed("old", &[]),
        packed("pin", &[]),
    ];
    let len = all4[0].2.len() as u64;
    assert!(
        all4.iter().all(|p| p.2.len() as u64 == len),
        "packs of one length"
    );
    for (i, (en, x, _)) in all4.iter().enumerate() {
        put(&mut store, en, x);
        assert!(store.touch(&en.address, &comp, 100 + i as u64));
    }
    store.mark("land", &comp, false).unwrap();
    store.pin("pin", &comp, true).unwrap();
    assert!(store.touch("old", &comp, 50));
    assert!(!store.touch("none", &comp, 1));
    // Saved by the marks' commits; this touch is not, until `save`.
    assert!(store.touch("city", &comp, 500));
    let reopened = Store::open(fs.clone()).unwrap().0;
    let used =
        |s: &Store<MemFs>, addr: &str| s.packs().iter().find(|r| r.address == addr).unwrap().used;
    assert_eq!(
        used(&reopened, "city"),
        101,
        "a touch is in memory until saved"
    );
    store.save().unwrap();
    let reopened = Store::open(fs.clone()).unwrap().0;
    assert_eq!(used(&reopened, "city"), 500);
    let r = reopened
        .packs()
        .iter()
        .find(|r| r.address == "city")
        .unwrap();
    assert_eq!(r.deps, [dep("land")]);
    assert!(
        reopened
            .packs()
            .iter()
            .any(|r| r.address == "pin" && r.pinned)
    );
    assert!(
        reopened
            .packs()
            .iter()
            .any(|r| r.address == "land" && !r.asked)
    );

    // A dependency cannot be removed while its dependant stays.
    let Err(StoreError::Needed { by }) = store.remove("land", &comp) else {
        panic!("removed a needed pack")
    };
    assert_eq!(by, dep("city"));
    assert!(matches!(
        store.remove("none", &comp),
        Err(StoreError::NotInstalled)
    ));

    // Room: nothing removable is enough for an impossible install, so nothing goes.
    let next = packed("next", &[]).0;
    let Err(StoreError::Short { need, room }) =
        store.make_room(&next, Components::all(), 0, Some(len))
    else {
        panic!("made room that is not there")
    };
    assert_eq!(need, len);
    assert!(room < need);
    assert_eq!(store.packs().len(), 4);
    // One pack's room: the least recently used unpinned one goes (`old`, used at 50).
    let gone = store.make_room(&next, Components::all(), 0, None).unwrap();
    assert_eq!(
        gone.iter().map(|r| r.address.as_str()).collect::<Vec<_>>(),
        ["old"]
    );
    assert_eq!(addresses(&store), ["city", "land", "pin"]);
    // Enough free space and no quota: nothing goes.
    let gone = store
        .make_room(&next, Components::all(), 10 * len, None)
        .unwrap();
    assert!(gone.is_empty(), "already fits");
    // A quota of two packs with three installed: `city`, then `land` (now unneeded) go;
    // `pin` stays.
    let gone = store
        .make_room(&next, Components::all(), 10 * len, Some(2 * len))
        .unwrap();
    assert_eq!(
        gone.iter().map(|r| r.address.as_str()).collect::<Vec<_>>(),
        ["city", "land"]
    );
    assert_eq!(addresses(&store), ["pin"]);
    // The files went with the records.
    assert_eq!(fs.clone().list("packs").unwrap().len(), 1);
}

/// Removing a pack takes its unneeded, unpinned dependencies along, never asked ones; a pinned
/// pack goes only when asked.
#[test]
fn removal_takes_unneeded_dependencies_along() {
    let comp = "places".to_owned();
    let land = packed("land", &[]);
    let city = packed("city", &[&format!("land.{comp}.tgd")]);
    let fs = MemFs(Rc::default());
    let mut store = Store::open(fs.clone()).unwrap().0;
    put(&mut store, &land.0, &land.1);
    put(&mut store, &city.0, &city.1);
    store.mark("land", &comp, false).unwrap();
    let gone = store.remove("city", &comp).unwrap();
    assert_eq!(gone.len(), 2, "{gone:?}");
    assert_eq!(store.packs(), &[] as &[Record], "all gone");
    assert_eq!(
        fs.clone().list("packs").unwrap(),
        Vec::<String>::new(),
        "no files"
    );
    put(&mut store, &land.0, &land.1);
    put(&mut store, &city.0, &city.1);
    assert_eq!(
        store.remove("city", &comp).unwrap().len(),
        1,
        "land was asked for"
    );
    store.mark("land", &comp, false).unwrap();
    store.pin("land", &comp, true).unwrap();
    put(&mut store, &city.0, &city.1);
    assert_eq!(
        store.remove("city", &comp).unwrap().len(),
        1,
        "land is pinned"
    );
    store.remove("land", &comp).unwrap();
    assert_eq!(store.packs(), &[] as &[Record], "all gone");
}

#[test]
fn a_version_1_journal_is_read_and_upgraded() {
    let pack = pack_of("2026.11.0");
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let fs = MemFs(Rc::default());
    let mut store = Store::open(fs.clone()).unwrap().0;
    put(&mut store, &entry, &asset);
    let r = store.packs()[0].clone();
    let set: Vec<&str> = r
        .set
        .iter()
        .map(teigeodata_format::component::Component::name)
        .collect();
    let head = r.head.iter().fold(String::new(), |mut h, b| {
        let _ = write!(h, "{b:02x}");
        h
    });
    let v1 = serde_json::json!({"version": 1, "packs": [{
        "address": r.address, "component": r.component, "release": r.release, "set": set,
        "file": r.file, "pack_len": r.pack_len, "head": head,
    }]})
    .to_string();
    let mut f = fs.clone().open("journal").unwrap();
    f.set_len(0).unwrap();
    f.write_all(v1.as_bytes()).unwrap();
    let (mut store, rec) = Store::open(fs.clone()).unwrap();
    assert!(rec.removed.is_empty() && rec.damaged.is_empty(), "{rec:?}");
    let got = &store.packs()[0];
    assert!(got.asked && !got.pinned && got.used == 0 && got.deps.is_empty());
    assert_eq!(got.file, r.file);
    store.pin(&r.address, &r.component, true).unwrap();
    let text = String::from_utf8(fs.clone().read("journal").unwrap().unwrap()).unwrap();
    assert!(
        text.starts_with("{\"packs\"") || text.contains("\"version\":2"),
        "{text}"
    );
    // A journal from a newer version is refused, and nothing is removed.
    let mut f = fs.clone().open("journal").unwrap();
    f.set_len(0).unwrap();
    f.write_all(b"{\"version\":3,\"packs\":[]}").unwrap();
    assert!(Store::open(fs.clone()).is_err());
    assert_eq!(fs.clone().list("packs").unwrap().len(), 1);
}

/// Removal commits first and deletes after: a crash at any step leaves the pack listed and
/// whole, or gone with no file left (gate 8).
#[test]
fn a_removal_survives_a_crash_at_every_step() {
    let comp = "places".to_owned();
    let land = packed("land", &[]);
    let city = packed("city", &[&format!("land.{comp}.tgd")]);
    let fresh = || {
        let fs = MemFs(Rc::default());
        let mut store = Store::open(fs.clone()).unwrap().0;
        put(&mut store, &land.0, &land.1);
        put(&mut store, &city.0, &city.1);
        store.mark("land", &comp, false).unwrap();
        fs.0.borrow_mut().reboot(Crash::Lose);
        fs
    };
    let fs = fresh();
    let before = fs.0.borrow().ops;
    Store::open(fs.clone())
        .unwrap()
        .0
        .remove("city", &comp)
        .unwrap();
    let steps = fs.0.borrow().ops - before;
    assert!(steps >= 5, "{steps}");
    let mut seen = BTreeSet::new();
    for how in [Crash::Lose, Crash::Keep] {
        for k in 0..steps {
            let fs = fresh();
            let at = fs.0.borrow().ops + k;
            fs.0.borrow_mut().crash_at = Some(at);
            let done = Store::open(fs.clone())
                .ok()
                .and_then(|(mut s, _)| s.remove("city", &comp).ok())
                .is_some();
            fs.0.borrow_mut().reboot(how);
            let (packs, files) = state(&fs);
            assert!(
                packs.len() == 2 || packs.is_empty(),
                "{how:?} {k}: {}",
                packs.len()
            );
            assert!(
                packs.iter().all(|p| p.1 == land.2 || p.1 == city.2),
                "{how:?} {k}: a pack not whole"
            );
            assert_eq!(files.len(), packs.len(), "{how:?} {k}: {files:?}");
            assert!(!done || packs.is_empty(), "{how:?} {k}: Ok, yet listed");
            seen.insert(packs.len());
        }
    }
    assert_eq!(
        seen.len(),
        2,
        "crashes before and after the commit both happen"
    );
}

/// A small deterministic generator, for the property below.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// `plan_room` on generated stores: what it removes is never pinned, kept or needed by a pack
/// that stays; the result fits, and fits only with its last removal; or it is short, and then
/// even every removable pack was not enough.
#[test]
fn plan_room_never_removes_what_must_stay() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut short = 0;
    let mut removed = 0;
    for _ in 0..3_000 {
        let n = 1 + usize::try_from(rng.next(9)).unwrap();
        let packs: Vec<Record> = (0..n)
            .map(|i| {
                let deps = (0..n)
                    .filter(|&j| j != i && rng.next(4) == 0)
                    .map(|j| format!("p{j}.places.tgd"))
                    .collect();
                Record {
                    address: format!("p{i}"),
                    component: "places".into(),
                    release: "2026.11.0".into(),
                    set: Components::all(),
                    file: format!("p{i}.tgd"),
                    pack_len: 1 + rng.next(100),
                    head: [0; 16],
                    deps,
                    asked: rng.next(2) == 0,
                    pinned: rng.next(5) == 0,
                    used: rng.next(10),
                }
            })
            .collect();
        let keep: Vec<String> = (0..n)
            .filter(|_| rng.next(6) == 0)
            .map(|j| format!("p{j}.places.tgd"))
            .collect();
        let installed: u64 = packs.iter().map(|r| r.pack_len).sum();
        let want = Room {
            need: rng.next(300),
            available: rng.next(200),
            quota: (rng.next(2) == 0).then(|| rng.next(500)),
            keep: &keep,
        };
        let room = |freed: u64| {
            let disk = want.available + freed;
            want.quota
                .map_or(disk, |q| disk.min(q.saturating_sub(installed - freed)))
        };
        // What may never go, given what stays.
        let check = |gone: &[usize]| {
            for &g in gone {
                let r = &packs[g];
                assert!(!r.pinned, "a pinned pack removed");
                assert!(!keep.contains(&r.key()), "a kept pack removed");
                let needed = packs
                    .iter()
                    .enumerate()
                    .any(|(j, o)| !gone.contains(&j) && o.deps.contains(&r.key()));
                assert!(!needed, "a needed pack removed");
            }
        };
        match plan_room(&packs, &want) {
            Ok(order) => {
                let mut set = order.clone();
                set.sort_unstable();
                set.dedup();
                assert_eq!(set.len(), order.len(), "a pack removed twice");
                check(&order);
                let freed: u64 = order.iter().map(|&i| packs[i].pack_len).sum();
                assert!(want.need <= room(freed), "does not fit");
                if let Some(&last) = order.last() {
                    assert!(
                        want.need > room(freed - packs[last].pack_len),
                        "removed one too many"
                    );
                }
                removed += order.len();
            }
            Err(StoreError::Short { need, room: got }) => {
                assert_eq!(need, want.need);
                assert!(got < need);
                // Removing everything removable, by the same rules, is still short.
                let mut gone: Vec<usize> = Vec::new();
                loop {
                    let next = (0..n).find(|&i| {
                        let r = &packs[i];
                        !gone.contains(&i)
                            && !r.pinned
                            && !keep.contains(&r.key())
                            && !packs.iter().enumerate().any(|(j, o)| {
                                j != i && !gone.contains(&j) && o.deps.contains(&r.key())
                            })
                    });
                    match next {
                        Some(i) => gone.push(i),
                        None => break,
                    }
                }
                let freed: u64 = gone.iter().map(|&i| packs[i].pack_len).sum();
                assert_eq!(room(freed), got, "short, yet more could be removed");
                short += 1;
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(short > 100 && removed > 500, "{short} {removed}");
}
