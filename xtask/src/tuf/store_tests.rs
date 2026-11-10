//! The store (D-171) against a file system that crashes: at every step of an install, with
//! unsynced data lost or kept, recovery leaves the old or the new pack installed, never a mix,
//! and no orphan file (gate 8 of the sync gates).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::rc::Rc;

use teigeodata_format::component::Components;
use teigeodata_sync::fetch::Mirrors;
use teigeodata_sync::store::{Fs, Store, StoreFile};

use super::install_tests::{Mem, pack_of};

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
    let install = |fs: &MemFs, e: &teigeodata_sync::catalog::Entry, x: &[u8]| {
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
