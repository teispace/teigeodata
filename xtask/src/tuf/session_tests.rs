//! The store's client core (D-374) as a browser runs it: every file system call and every
//! request waits once before it answers, as OPFS and `fetch` promises do, so each await in the
//! core is taken. Set up, refresh, install with dependencies, status, a reopen, removal.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use teigeodata_sync::fetch::{AsyncTransport, FetchError, Got};
use teigeodata_sync::meta::block_on;
use teigeodata_sync::policy::{ClientError, Want};
use teigeodata_sync::session::{self, Session};
use teigeodata_sync::store::{AsyncFs, Fs, Health};

use super::install_tests::at;
use super::policy_tests::published;
use super::refresh_tests::{NOW, root_file};
use super::store_tests::{MemFile, MemFs};

/// A future that is pending once, counting each wait in `waits`.
struct Wait<'a>(bool, &'a Cell<u32>);

impl Future for Wait<'_> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        self.1.set(self.1.get() + 1);
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// A file system whose every directory call waits once (OPFS).
struct Web(MemFs, Rc<Cell<u32>>);

impl AsyncFs for Web {
    type File = MemFile;

    async fn open(&mut self, path: &str) -> io::Result<MemFile> {
        Wait(false, &self.1).await;
        self.0.open(path)
    }
    async fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>> {
        Wait(false, &self.1).await;
        self.0.read(path)
    }
    async fn len(&mut self, path: &str) -> io::Result<Option<u64>> {
        Wait(false, &self.1).await;
        self.0.len(path)
    }
    async fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        Wait(false, &self.1).await;
        self.0.rename(from, to)
    }
    async fn remove(&mut self, path: &str) -> io::Result<()> {
        Wait(false, &self.1).await;
        self.0.remove(path)
    }
    async fn create_dir(&mut self, path: &str) -> io::Result<()> {
        Wait(false, &self.1).await;
        self.0.create_dir(path)
    }
    async fn sync_dir(&mut self, path: &str) -> io::Result<()> {
        Wait(false, &self.1).await;
        self.0.sync_dir(path)
    }
    async fn list(&mut self, path: &str) -> io::Result<Vec<String>> {
        Wait(false, &self.1).await;
        self.0.list(path)
    }
}

/// A metadata mirror `m/` and an asset mirror `a/`, each request waiting once (`fetch`).
struct Net {
    meta: BTreeMap<String, Vec<u8>>,
    assets: BTreeMap<String, Vec<u8>>,
    waits: Rc<Cell<u32>>,
    got: u64,
}

impl AsyncTransport for Net {
    async fn get(
        &mut self,
        url: &str,
        range: Range<u64>,
        out: &mut Vec<u8>,
    ) -> Result<Got, FetchError> {
        Wait(false, &self.waits).await;
        let asset = url
            .strip_prefix("a/")
            .and_then(|f| self.assets.get(f))
            .ok_or(FetchError::Status(404))?;
        self.got += range.end - range.start;
        out.clear();
        out.extend_from_slice(&asset[at(&range)]);
        Ok(Got {
            etag: Some("\"a\"".into()),
            total: asset.len() as u64,
        })
    }

    async fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError> {
        Wait(false, &self.waits).await;
        let file = url
            .strip_prefix("m/")
            .and_then(|f| self.meta.get(f))
            .ok_or(FetchError::Status(404))?;
        if file.len() as u64 > max {
            return Err(FetchError::Response("longer than allowed"));
        }
        out.clear();
        out.extend_from_slice(file);
        Ok(())
    }
}

#[test]
fn the_client_core_runs_over_a_file_system_and_network_that_wait() {
    let disk = MemFs(Rc::default());
    let waits = Rc::new(Cell::new(0));
    let (meta, assets) = published("2026.11.0", &[("alpha", &["beta"]), ("beta", &[])]);
    let net = || Net {
        meta: meta.clone(),
        assets: assets.clone(),
        waits: Rc::clone(&waits),
        got: 0,
    };

    // Not set up: refused, naming init.
    let Err(e) = block_on(Session::open(Web(disk.clone(), Rc::clone(&waits)), net())) else {
        panic!("a store not set up opened");
    };
    assert!(
        matches!(&e, ClientError::Refused(m) if m.contains("set the store up with init first")),
        "{e}"
    );

    let mut fs = Web(disk.clone(), Rc::clone(&waits));
    block_on(session::init(
        &mut fs,
        &["m".into()],
        &["a/".into()],
        &root_file(1, 10),
        NOW,
    ))
    .unwrap();
    let (mut s, damaged) = block_on(Session::open(fs, net())).unwrap();
    assert_eq!(damaged.len(), 0);

    // alpha needs beta (neither a country's): beta first, both installed, alpha pinned; every byte from the asset mirror.
    let before = waits.get();
    let r = block_on(s.install(&["alpha.places.tgd".into()], &Want::default(), true, NOW)).unwrap();
    let done: Vec<_> = r.done.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(done, ["beta.places.tgd", "alpha.places.tgd"]);
    assert!(r.stale_catalog.is_none());
    assert!(waits.get() > before, "the core awaited none of its calls");
    let wire: u64 = assets.values().map(|a| a.len() as u64).sum();
    assert!(s.net().got > 0 && s.net().got <= wire);

    let st = block_on(s.status(true, NOW)).unwrap();
    assert_eq!(st.catalog.map(|(v, _)| v), Some(1));
    assert!(st.packs.iter().all(|(_, h)| *h == Health::Good));
    assert_eq!(st.packs.len(), 2);

    // A reopen of the same disk reads the same packs from the journal; offline works.
    let (mut s, _) = block_on(Session::open(Web(disk, Rc::clone(&waits)), net())).unwrap();
    assert_eq!(s.packs().len(), 2);
    assert!(block_on(s.offline(NOW)).is_ok());

    // beta is needed by alpha: refused; alpha goes, and beta with it, as nothing else needs it.
    let e = block_on(s.remove("beta.places.tgd")).unwrap_err();
    assert!(matches!(e, ClientError::Refused(_)), "{e}");
    let mut gone: Vec<_> = block_on(s.remove("alpha.places.tgd"))
        .unwrap()
        .into_iter()
        .map(|r| r.address)
        .collect();
    gone.sort();
    assert_eq!(gone, ["alpha", "beta"]);
    assert_eq!(s.packs().len(), 0);
}
