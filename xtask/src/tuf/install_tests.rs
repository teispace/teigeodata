//! The installer (D-167) against what a release publishes: the catalog entry and transport asset
//! `entry_of` makes from a pack, installed by component through `sync::install`.
#![allow(
    clippy::cast_possible_truncation,
    reason = "test data: bytes from small counters, offsets into buffers sized here"
)]

use std::ops::Range;

use teigeodata_format::component::{Component, Components};
use teigeodata_format::{Encoding, Kind, Pack, Tag, Writer};
use teigeodata_sync::Error;
use teigeodata_sync::install::Install;
use teigeodata_sync::transport::{Fetch, Frames, Layout, SeekTable, encode_table};

/// A pack with a 2.5 MiB NAMES (three frames, covered by `BLOCK_HASHES`), small sections of
/// other components, and an empty one.
fn pack() -> Vec<u8> {
    pack_of("2026.11.0")
}

/// The same pack under another release.
pub(super) fn pack_of(release: &str) -> Vec<u8> {
    pack_at("zz", release)
}

/// The test pack under another address.
pub(super) fn pack_at(address: &str, release: &str) -> Vec<u8> {
    pack_with(address, release, 0)
}

/// The test pack with its PLACES bytes changed by `salt` (0: as [`pack_at`]).
pub(super) fn pack_with(address: &str, release: &str, salt: u8) -> Vec<u8> {
    let ids: Vec<u8> = (0..40_000_u32).map(|i| (i * 7 % 251) as u8).collect();
    let places: Vec<u8> = (0..30_000_u32).map(|i| (i % 13) as u8 ^ salt).collect();
    let names: Vec<u8> = (0..(5_u32 << 19))
        .map(|i| b"Lahachowk Kathmandu Pokhara "[(i % 28) as usize] ^ (i >> 16) as u8)
        .collect();
    let tz = vec![9_u8; 3_000];
    let mut w = Writer::new(address, "places", release).unwrap();
    w.manifest(Tag::LICENCE, b"odbl").unwrap();
    w.pack(
        &[
            (Kind::IDMAP, Encoding::RAW, 1, &ids),
            (Kind::PLACES, Encoding::RAW, 1, &places),
            (Kind::HIERARCHY, Encoding::RAW, 0, &[]),
            (Kind::NAMES, Encoding::RAW, 1, &names),
            (Kind::PLACE_TZ, Encoding::RAW, 1, &tz),
        ],
        None,
    )
    .unwrap()
}

pub(super) fn at(r: &Range<u64>) -> Range<usize> {
    usize::try_from(r.start).unwrap()..usize::try_from(r.end).unwrap()
}

/// Installs `set` from `asset`, every write recorded; the result and the staging file.
fn install(
    entry: &teigeodata_sync::catalog::Entry,
    asset: &[u8],
    pack_len: usize,
    set: Components,
) -> (Result<teigeodata_sync::install::Installed, Error>, Vec<u8>) {
    let mut staged = vec![0_u8; pack_len];
    let mut put = |off: u64, b: &[u8]| -> Result<(), Error> {
        let o = usize::try_from(off).unwrap();
        staged[o..o + b.len()].copy_from_slice(b);
        Ok(())
    };
    let mut inst = Install::new(entry, asset.len() as u64, set);
    let run = (|| {
        let [t, h] = inst.first()?;
        let fetches = inst
            .start(&asset[at(&t)], &asset[at(&h)], &mut put)?
            .to_vec();
        for f in &fetches {
            inst.feed(f, &asset[at(&f.compressed)], &mut put)?;
        }
        inst.finish()
    })();
    (run, staged)
}

#[test]
fn the_installer_rebuilds_the_pack_from_its_catalog_entry() {
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let p = Pack::open(&pack).unwrap();
    let sets = [
        Components::all(),
        Components::of(&[Component::Point]),
        Components::of(&[Component::Names]),
        Components::of(&[Component::Timezone]),
        Components::of(&[Component::Admin, Component::Timezone]),
    ];
    for set in sets {
        let (done, staged) = install(&entry, &asset, pack.len(), set);
        let done = done.unwrap_or_else(|e| panic!("{set:?}: {e}"));
        // The bytes fetched are the catalog's download size for the set (D-162).
        assert_eq!(done.fetched, entry.size_of(set).wire, "{set:?}");
        // The staged file opens, and holds exactly the pack's bytes for every installed section.
        let s = Pack::open_head(&staged).unwrap();
        for sec in p.sections() {
            if set.closed().holds(sec.kind()) {
                let got = s.section(sec.kind()).unwrap();
                assert_eq!(got.bytes(), sec.bytes(), "{set:?} {:?}", sec.kind());
                got.verify().unwrap();
            }
        }
        if set == Components::all() {
            assert!(staged == pack, "the whole pack, byte for byte");
            assert_eq!(done.written, pack.len() as u64);
        }
    }
}

/// Rebuilds `asset` with frame `i`'s decoded bytes changed by `edit` and recompressed validly,
/// so only the hashes can tell.
fn forge_frame(asset: &[u8], i: usize, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let tail = SeekTable::tail_len(asset).unwrap();
    let table = SeekTable::parse(&asset[asset.len() - tail..]).unwrap();
    let mut entries: Vec<(u32, u32)> = table
        .entries()
        .map(|e| (e.compressed, e.decompressed))
        .collect();
    let start: usize = entries[..i].iter().map(|e| e.0 as usize).sum();
    let end = start + entries[i].0 as usize;
    let mut decoded = zstd::bulk::decompress(&asset[start..end], entries[i].1 as usize).unwrap();
    edit(&mut decoded);
    let frame = zstd::bulk::compress(&decoded, 19).unwrap();
    entries[i].0 = u32::try_from(frame.len()).unwrap();
    let mut out = asset[..start].to_vec();
    out.extend_from_slice(&frame);
    out.extend_from_slice(&asset[end..asset.len() - tail]);
    encode_table(entries.into_iter(), &mut out).unwrap();
    out
}

#[test]
fn nothing_unchecked_is_written() {
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let all = Components::all();
    let tail = SeekTable::tail_len(&asset).unwrap();
    let frames = SeekTable::parse(&asset[asset.len() - tail..])
        .unwrap()
        .entries()
        .len();
    // Each frame, with one byte changed and validly recompressed: the install fails, and the
    // staging file never holds the changed byte. (The asset's length changes with the frame,
    // as a host serving other bytes would; the catalog entry stays the signed one.)
    for i in 0..frames {
        let forged = forge_frame(&asset, i, |d| {
            let mid = d.len() / 2;
            d[mid] ^= 0x40;
        });
        let mut e = entry.clone();
        e.head_frame.wire = u64::from(
            SeekTable::parse(&forged[forged.len() - tail..])
                .unwrap()
                .entry(0)
                .unwrap()
                .compressed,
        );
        let (done, staged) = install(&e, &forged, pack.len(), all);
        assert!(done.is_err(), "frame {i}");
        let wrong = staged
            .iter()
            .zip(&pack)
            .position(|(a, b)| a != b && *a != 0);
        assert_eq!(wrong, None, "frame {i}: a changed byte was written");
    }
    // A padding byte after each section that has padding, and the footer's last byte.
    let starts: Vec<u64> = SeekTable::parse(&asset[asset.len() - tail..])
        .unwrap()
        .entries()
        .scan(0_u64, |at, e| {
            let s = *at;
            *at += u64::from(e.decompressed);
            Some(s)
        })
        .collect();
    let p = Pack::open(&pack).unwrap();
    let mut targets: Vec<u64> = p
        .head()
        .sections()
        .map(|s| s.offset + s.length)
        .filter(|end| end % 64 != 0)
        .collect();
    targets.push(pack.len() as u64 - 1);
    assert!(targets.len() >= 3);
    for t in targets {
        let i = starts.iter().rposition(|&s| s <= t).unwrap();
        let k = usize::try_from(t - starts[i]).unwrap();
        let forged = forge_frame(&asset, i, |d| d[k] ^= 1);
        let mut e = entry.clone();
        e.head_frame.wire = u64::from(
            SeekTable::parse(&forged[forged.len() - tail..])
                .unwrap()
                .entry(0)
                .unwrap()
                .compressed,
        );
        let (done, _) = install(&e, &forged, pack.len(), all);
        assert!(done.is_err(), "byte {t} of frame {i}");
    }
    // A catalog entry for another pack: refused at the head.
    let mut other = entry.clone();
    other.head[0] ^= 1;
    let (done, staged) = install(&other, &asset, pack.len(), all);
    assert_eq!(
        done,
        Err(Error::Install("the head hash is not the catalog's"))
    );
    assert!(staged.iter().all(|&b| b == 0), "nothing written");
    let mut renamed = entry.clone();
    renamed.release = "2026.12.0".into();
    let (done, _) = install(&renamed, &asset, pack.len(), all);
    assert!(done.is_err());
}

#[test]
fn the_caller_is_held_to_the_order() {
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let mut put = |_: u64, _: &[u8]| -> Result<(), Error> { Ok(()) };
    let mut inst = Install::new(&entry, asset.len() as u64, Components::all());
    let none = Fetch {
        frames: 0..1,
        compressed: 0..1,
        decoded: 0..1,
    };
    assert!(inst.feed(&none, &[], &mut put).is_err(), "fed before start");
    assert!(inst.staged(0, &[]).is_err(), "staged before start");
    assert!(inst.finish().is_err(), "never started");
    let [t, h] = inst.first().unwrap();
    // A host that returns a short head frame.
    let short = &asset[at(&h)][1..];
    assert!(inst.start(&asset[at(&t)], short, &mut put).is_err());
    let mut inst = Install::new(&entry, asset.len() as u64, Components::all());
    let fetches = inst
        .start(&asset[at(&t)], &asset[at(&h)], &mut put)
        .unwrap()
        .to_vec();
    // BLOCK_HASHES first, alone; NAMES (covered) before it is refused.
    let names = fetches
        .iter()
        .position(|f| f.frames.len() >= 3)
        .expect("NAMES's three frames, merged with their neighbours");
    assert_ne!(names, 0);
    assert_eq!(
        inst.feed(
            &fetches[names],
            &asset[at(&fetches[names].compressed)],
            &mut put
        ),
        Err(Error::Install("a large section before its block hashes"))
    );
    // A fetch that is not one of this install's: a range one byte off, frames past the table.
    let f0 = &fetches[0];
    let mut off = f0.clone();
    off.compressed.end += 1;
    assert_eq!(
        inst.feed(&off, &asset[at(&off.compressed)], &mut put),
        Err(Error::Install("not a fetch of this install"))
    );
    let mut past = f0.clone();
    past.frames = 1_000..1_001;
    assert!(inst.feed(&past, &[], &mut put).is_err());
    inst.feed(f0, &asset[at(&f0.compressed)], &mut put).unwrap();
    assert_eq!(
        inst.feed(f0, &asset[at(&f0.compressed)], &mut put),
        Err(Error::Install("a frame not wanted, or fed twice"))
    );
    assert!(inst.staged(f0.frames.start, &[]).is_err(), "already done");
    // The rest of the plan, less the frames of the refused NAMES fetch that came before NAMES
    // and were checked and written before it was refused.
    let rest = inst.remaining();
    assert_eq!(rest.len(), 2, "{rest:?}");
    assert_eq!(rest[0].frames.end, fetches[names].frames.end);
    assert!(rest[0].frames.start > fetches[names].frames.start);
    assert_eq!(rest[1], fetches[2]);
    assert!(inst.finish().is_err(), "not every frame fed");
    // A write that fails stops the install with its error.
    let mut full = |_: u64, _: &[u8]| -> Result<(), Error> { Err(Error::Install("disk full")) };
    assert_eq!(
        inst.feed(&rest[1], &asset[at(&rest[1].compressed)], &mut full),
        Err(Error::Install("disk full"))
    );

    // Staged bytes (resume): the list is never kept, even when right; a covered frame is not
    // kept before the list is fed; a small section's right bytes are, and wrong ones are not.
    let mut inst = Install::new(&entry, asset.len() as u64, Components::all());
    let fetches = inst
        .start(&asset[at(&t)], &asset[at(&h)], &mut put)
        .unwrap()
        .to_vec();
    let bytes = |i: usize| {
        let d = inst.frame(i).unwrap().decoded;
        pack[at(&d)].to_vec()
    };
    let hashes = fetches[0].frames.start;
    let (first, last) = (fetches[names].frames.start, fetches[names].frames.end - 1);
    let (lb, cb, sb) = (bytes(hashes), bytes(last - 1), bytes(first));
    assert_eq!(
        inst.staged(hashes, &lb),
        Ok(false),
        "the list is never kept"
    );
    assert_eq!(
        inst.staged(last - 1, &cb),
        Ok(false),
        "covered, before the list"
    );
    let mut wrong = sb.clone();
    wrong[0] ^= 1;
    assert_eq!(inst.staged(first, &wrong), Ok(false));
    assert_eq!(inst.staged(first, &sb[1..]), Ok(false), "a short frame");
    assert_eq!(inst.staged(first, &sb), Ok(true));
    assert!(!inst.todo(first) && inst.todo(hashes) && inst.todo(last - 1));
}

/// How the test host answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Host {
    Honest,
    /// 200 with the whole asset, as a host that ignores `Range`.
    IgnoresRange,
    /// A `Content-Range` one byte off.
    ShiftsRange,
    /// A new `ETag` from the third request on, as an asset replaced midway.
    ChangesEtag,
    /// One byte more than asked.
    RunsOver,
}

/// A local HTTP/1.1 host for `asset`, answering as `how`; its URL.
fn serve(asset: Vec<u8>, how: Host) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/asset.tgd.zst", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { return };
            let mut range = None;
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    let (a, b) = v.trim().split_once('-').unwrap();
                    range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                }
            }
            let total = asset.len();
            let (a, b) = range.unwrap_or((0, total - 1));
            let etag = if how == Host::ChangesEtag && n >= 2 {
                "\"b\""
            } else {
                "\"a\""
            };
            let (status, cr, body) = match how {
                Host::IgnoresRange => ("200 OK", String::new(), &asset[..]),
                Host::ShiftsRange => (
                    "206 Partial Content",
                    format!("Content-Range: bytes {}-{}/{total}\r\n", a + 1, b + 1),
                    &asset[a..=b],
                ),
                Host::RunsOver => (
                    "206 Partial Content",
                    format!("Content-Range: bytes {a}-{b}/{total}\r\n"),
                    &asset[a..=(b + 1).min(total - 1)],
                ),
                _ => (
                    "206 Partial Content",
                    format!("Content-Range: bytes {a}-{b}/{total}\r\n"),
                    &asset[a..=b],
                ),
            };
            let head = format!(
                "HTTP/1.1 {status}\r\n{cr}Content-Length: {}\r\nETag: {etag}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    url
}

/// The driver over the default transport (D-168): an honest host installs the pack byte for
/// byte; each kind of dishonest host is refused for its own reason.
#[test]
fn the_default_transport_installs_and_refuses_bad_hosts() {
    use teigeodata_sync::fetch::{FetchError, Ureq, install};
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let n = asset.len() as u64;
    let run = |how: Host, set: Components| {
        let url = serve(asset.clone(), how);
        let mut staged = std::io::Cursor::new(Vec::new());
        let r = install(&mut Ureq::new(), &url, &entry, n, set, &mut staged);
        (r, staged.into_inner())
    };
    let (r, staged) = run(Host::Honest, Components::all());
    let done = r.unwrap();
    assert!(staged == pack, "the pack, byte for byte");
    assert_eq!(done.fetched, entry.size_of(Components::all()).wire);
    // Points only: the staged file is the pack's length, its sections the pack's.
    let (r, staged) = run(Host::Honest, Components::of(&[Component::Point]));
    r.unwrap();
    assert_eq!(staged.len(), pack.len());
    let s = Pack::open_head(&staged).unwrap();
    s.section(Kind::IDMAP).unwrap().verify().unwrap();
    assert!(matches!(
        run(Host::IgnoresRange, Components::all()).0,
        Err(FetchError::Status(200))
    ));
    for (how, why) in [
        (
            Host::ShiftsRange,
            "Content-Range is not the range asked for",
        ),
        (
            Host::ChangesEtag,
            "the asset changed on the host during the install",
        ),
        (Host::RunsOver, "the body is not the range's length"),
    ] {
        match run(how, Components::all()).0 {
            Err(FetchError::Response(p)) => assert_eq!(p, why),
            other => panic!("{why}: {other:?}"),
        }
    }
    // The catalog says another length: the first response is refused.
    let url = serve(asset.clone(), Host::Honest);
    let mut staged = std::io::Cursor::new(Vec::new());
    let r = install(
        &mut Ureq::new(),
        &url,
        &entry,
        n - 1,
        Components::all(),
        &mut staged,
    );
    assert!(
        matches!(
            r,
            Err(FetchError::Response(
                "the asset's length is not the catalog's"
            ))
        ),
        "{r:?}"
    );
    // Nobody listening.
    let mut staged = std::io::Cursor::new(Vec::new());
    let r = install(
        &mut Ureq::new(),
        "http://127.0.0.1:9/none",
        &entry,
        n,
        Components::all(),
        &mut staged,
    );
    assert!(matches!(r, Err(FetchError::Transport(_))), "{r:?}");
}

/// An in-memory host that counts the bytes it serves and can drop the connection at request `cut`.
pub(super) struct Mem<'a> {
    asset: &'a [u8],
    pub(super) served: u64,
    calls: usize,
    cut: Option<usize>,
}

impl<'a> Mem<'a> {
    pub(super) fn new(asset: &'a [u8], cut: Option<usize>) -> Self {
        Self {
            asset,
            served: 0,
            calls: 0,
            cut,
        }
    }
}

impl teigeodata_sync::fetch::Transport for Mem<'_> {
    fn get(
        &mut self,
        _: &str,
        range: Range<u64>,
        out: &mut Vec<u8>,
    ) -> Result<teigeodata_sync::fetch::Got, teigeodata_sync::fetch::FetchError> {
        if self.cut == Some(self.calls) {
            return Err(teigeodata_sync::fetch::FetchError::Transport("cut".into()));
        }
        self.calls += 1;
        self.served += range.end - range.start;
        out.clear();
        out.extend_from_slice(&self.asset[at(&range)]);
        Ok(teigeodata_sync::fetch::Got {
            etag: Some("\"a\"".into()),
            total: self.asset.len() as u64,
        })
    }
}

/// The asset's frames, checked against the pack.
fn frames_of(pack: &[u8], asset: &[u8]) -> Frames {
    let tail = SeekTable::tail_len(asset).unwrap();
    let table = SeekTable::parse(&asset[asset.len() - tail..]).unwrap();
    let layout = Layout::of(&Pack::open(pack).unwrap()).unwrap();
    table.check(asset.len() as u64, layout).unwrap()
}

/// Resume (D-169): whatever an earlier attempt left in staging, the install ends with the pack
/// byte for byte, keeps every frame that checks, and fetches only the rest.
#[test]
fn resume_keeps_what_checks_and_fetches_the_rest() {
    use std::io::Cursor;
    use teigeodata_sync::fetch::install;
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let n = asset.len() as u64;
    let all = Components::all();
    let frames = frames_of(&pack, &asset);
    let tail = SeekTable::tail_len(&asset).unwrap() as u64;
    let head = frames.frame(0).unwrap().compressed;
    // Every attempt pays for the seek table and the head frame.
    let base = tail + (head.end - head.start);
    let run = |file: &mut Cursor<Vec<u8>>, cut: Option<usize>| {
        let mut host = Mem::new(&asset, cut);
        let r = install(&mut host, "mem:", &entry, n, all, file);
        if let Ok(done) = &r {
            assert_eq!(done.fetched, host.served, "the count is what was served");
            file.get_mut()
                .truncate(usize::try_from(done.pack_len).unwrap());
        }
        (r, host.served)
    };
    let mut fresh = Cursor::new(Vec::new());
    let (r, full) = run(&mut fresh, None);
    let done = r.unwrap();
    assert!(fresh.get_ref() == &pack, "fresh: the pack");
    assert_eq!(done.kept, 0);
    let requests = Mem::new(&asset, None);
    let mut count = Cursor::new(Vec::new());
    let mut host = requests;
    install(&mut host, "mem:", &entry, n, all, &mut count).unwrap();
    let calls = host.calls;
    assert!(calls >= 4, "table, head, block hashes, the rest");

    // Cut at every request, then resume: the pack, byte for byte, and never more than a fresh
    // install; once anything past the head was written, less.
    for cut in 0..calls {
        let mut file = Cursor::new(Vec::new());
        assert!(run(&mut file, Some(cut)).0.is_err());
        let (r, served) = run(&mut file, None);
        let done = r.unwrap_or_else(|e| panic!("cut at {cut}: {e}"));
        assert!(file.get_ref() == &pack, "cut at {cut}");
        assert!(served <= full, "cut at {cut}");
        if cut > 3 {
            assert!(
                served < full && done.kept > 0,
                "cut at {cut}: {served} of {full}"
            );
        }
    }

    // The block-hash list's frame: always fetched again on a resume.
    let p = Pack::open(&pack).unwrap();
    let list = p.section(Kind::BLOCK_HASHES).unwrap().bytes();
    let list_at = list.as_ptr() as usize - pack.as_ptr() as usize;
    let lf = frames
        .iter()
        .find(|f| f.decoded.contains(&(list_at as u64)))
        .unwrap();
    let base = base + (lf.compressed.end - lf.compressed.start);

    // A whole staged file: only the table, the head and the list are fetched; the rest is kept.
    let mut file = Cursor::new(pack.clone());
    let (r, served) = run(&mut file, None);
    assert_eq!(served, base);
    let head_len = frames.frame(0).unwrap().decoded.end;
    assert_eq!(
        r.unwrap().written,
        head_len + (lf.decoded.end - lf.decoded.start),
        "nothing written but the head and the list"
    );

    // A torn frame (one byte flipped inside NAMES): only its frame is fetched again.
    let names = p.section(Kind::NAMES).unwrap().bytes();
    let names_at = names.as_ptr() as usize - pack.as_ptr() as usize;
    let torn_at = names_at + names.len() / 2;
    let torn = frames
        .iter()
        .find(|f| f.decoded.contains(&(torn_at as u64)))
        .unwrap();
    let mut bad = pack.clone();
    bad[torn_at] ^= 1;
    let mut file = Cursor::new(bad);
    let (r, served) = run(&mut file, None);
    r.unwrap();
    assert!(file.get_ref() == &pack, "resumed: the pack");
    assert_eq!(served, base + (torn.compressed.end - torn.compressed.start));

    // A stale block-hash list: the fresh one replaces it, and the NAMES frames are still kept,
    // checked against it.
    let mut bad = pack.clone();
    bad[list_at + 5] ^= 1;
    let mut file = Cursor::new(bad);
    let (r, served) = run(&mut file, None);
    let done = r.unwrap();
    assert!(file.get_ref() == &pack, "resumed: the pack");
    assert_eq!(served, base);
    assert!(done.kept >= names.len() as u64);

    // A foreign file, longer than the pack: everything is fetched and the file cut to size.
    let mut file = Cursor::new((0..pack.len() + 4_096).map(|i| (i % 251) as u8).collect());
    let (r, served) = run(&mut file, None);
    assert_eq!(r.unwrap().kept, 0);
    assert!(file.get_ref() == &pack, "resumed: the pack");
    assert_eq!(served, full);

    // Points only, cut midway, then resumed: its sections check.
    let pts = Components::of(&[Component::Point]);
    let mut file = Cursor::new(Vec::new());
    let mut host = Mem::new(&asset, Some(2));
    assert!(install(&mut host, "mem:", &entry, n, pts, &mut file).is_err());
    let mut host = Mem::new(&asset, None);
    install(&mut host, "mem:", &entry, n, pts, &mut file).unwrap();
    let s = Pack::open_head(file.get_ref()).unwrap();
    s.section(Kind::IDMAP).unwrap().verify().unwrap();
    s.section(Kind::PLACES).unwrap().verify().unwrap();
}

/// How a test mirror behaves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Behaves {
    Honest,
    /// Unreachable.
    Down,
    /// Serves the asset with one byte flipped at this offset.
    Flips(usize),
    /// Drops every request after this many.
    Cuts(usize),
}

/// Test mirrors at `m0/`, `m1/`, ...: the asset at each, as each behaves; bytes served by each.
struct Hosts<'a> {
    asset: &'a [u8],
    behave: Vec<Behaves>,
    served: Vec<u64>,
    calls: Vec<usize>,
}

impl teigeodata_sync::fetch::Transport for Hosts<'_> {
    fn get(
        &mut self,
        url: &str,
        range: Range<u64>,
        out: &mut Vec<u8>,
    ) -> Result<teigeodata_sync::fetch::Got, teigeodata_sync::fetch::FetchError> {
        use teigeodata_sync::fetch::FetchError;
        let m: usize = url[1..url.find('/').unwrap()].parse().unwrap();
        self.calls[m] += 1;
        let cut = matches!(self.behave[m], Behaves::Cuts(n) if self.calls[m] > n);
        if self.behave[m] == Behaves::Down || cut {
            return Err(FetchError::Transport("unreachable".into()));
        }
        out.clear();
        out.extend_from_slice(&self.asset[at(&range)]);
        if let Behaves::Flips(o) = self.behave[m]
            && range.contains(&(o as u64))
        {
            out[o - range.start as usize] ^= 0x40;
        }
        self.served[m] += range.end - range.start;
        Ok(teigeodata_sync::fetch::Got {
            etag: Some(format!("\"m{m}\"")),
            total: self.asset.len() as u64,
        })
    }
}

/// A staging file whose writes fail.
struct Full;
impl std::io::Read for Full {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}
impl std::io::Write for Full {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("disk full"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl std::io::Seek for Full {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

/// Mirrors (D-170): a mirror that serves a changed byte is refused and demoted, one that is
/// down or drops midway is passed over, the next resumes from what is staged, and the pack
/// is installed byte for byte; a failing staging file stops at once.
#[test]
fn mirrors_fail_over_and_demote() {
    use std::io::Cursor;
    use teigeodata_sync::fetch::{FetchError, Mirrors, install_from};
    let pack = pack();
    let (entry, asset, _) = super::entry_of(&pack).unwrap();
    let asset_len = asset.len() as u64;
    let all = Components::all();
    let frames = frames_of(&pack, &asset);
    // A byte inside the compressed NAMES frames.
    let opened = Pack::open(&pack).unwrap();
    let names = opened.section(Kind::NAMES).unwrap().bytes();
    let mid = (names.as_ptr() as usize - pack.as_ptr() as usize + names.len() / 2) as u64;
    let frame = frames.iter().find(|f| f.decoded.contains(&mid)).unwrap();
    let flip = usize::try_from(
        frame.compressed.start + (frame.compressed.end - frame.compressed.start) / 2,
    )
    .unwrap();
    let run = |behave: Vec<Behaves>, mirrors: &mut Mirrors| {
        let k = behave.len();
        let mut hosts = Hosts {
            asset: &asset,
            behave,
            served: vec![0; k],
            calls: vec![0; k],
        };
        let mut file = Cursor::new(Vec::new());
        let r = install_from(
            &mut hosts,
            mirrors,
            "/a.tgd.zst",
            &entry,
            asset_len,
            all,
            &mut file,
        );
        (r, file.into_inner(), hosts.served)
    };
    let bases = |k: usize| Mirrors::new((0..k).map(|i| format!("m{i}")));

    // A flipped byte: refused, demoted behind the honest mirror, which installs the pack.
    let mut m = bases(2);
    let (r, file, served) = run(vec![Behaves::Flips(flip), Behaves::Honest], &mut m);
    r.unwrap();
    assert!(file == pack, "the pack, byte for byte");
    assert_eq!((m.mirrors()[0].refused, m.mirrors()[1].refused), (1, 0));
    assert_eq!(m.order(), [1, 0], "demoted");
    // The honest mirror resumed: it did not serve what the bad one served and was kept.
    let full = entry.size_of(all).wire;
    assert!(served[1] < full, "{} of {full}", served[1]);
    // With the demotion kept, the next install goes to the honest mirror alone.
    let (r, _, served) = run(vec![Behaves::Flips(flip), Behaves::Honest], &mut m);
    r.unwrap();
    assert_eq!(served, [0, full]);

    // Down, then dropping midway: both passed over; the third resumes.
    let mut m = bases(3);
    let (r, file, served) = run(
        vec![Behaves::Down, Behaves::Cuts(4), Behaves::Honest],
        &mut m,
    );
    r.unwrap();
    assert!(file == pack, "the pack, byte for byte");
    assert_eq!(
        m.mirrors().iter().map(|x| x.failures).collect::<Vec<_>>(),
        [1, 1, 0]
    );
    assert_eq!(m.order(), [2, 0, 1]);
    assert!(served[2] < full);

    // Every mirror bad: the last error, nothing wrong written where anything was written.
    let mut m = bases(2);
    let (r, file, _) = run(vec![Behaves::Flips(flip), Behaves::Down], &mut m);
    assert!(matches!(r, Err(FetchError::Transport(_))), "{r:?}");
    for (i, (&a, &b)) in file.iter().zip(&pack).enumerate() {
        assert!(a == 0 || a == b, "byte {i} written wrong");
    }
    assert!(matches!(
        run(vec![], &mut Mirrors::default()).0,
        Err(FetchError::Response("no mirror"))
    ));

    // A staging file that cannot be written: stops at the first mirror, which is not blamed.
    let mut m = bases(2);
    let mut hosts = Hosts {
        asset: &asset,
        behave: vec![Behaves::Honest, Behaves::Honest],
        served: vec![0; 2],
        calls: vec![0; 2],
    };
    let r = install_from(&mut hosts, &mut m, "/a", &entry, asset_len, all, &mut Full);
    assert!(matches!(r, Err(FetchError::Io(_))), "{r:?}");
    assert_eq!(hosts.calls[1], 0, "no second mirror for a local failure");
    assert_eq!(
        m.mirrors()[0],
        teigeodata_sync::fetch::Mirror {
            base: "m0".into(),
            failures: 0,
            refused: 0,
        }
    );
}
