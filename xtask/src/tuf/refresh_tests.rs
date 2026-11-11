//! The catalog refresh (D-178) against an in-memory metadata host and the crash disk of the
//! store tests: what each refresh fetches, rotation and fast-forward recovery, lying and dead
//! mirrors, and a crash at every write.

use std::collections::BTreeMap;
use std::rc::Rc;

use ed25519_dalek::SigningKey;
use teigeodata_sync::fetch::{FetchError, Got, Mirrors, Transport};
use teigeodata_sync::refresh::{RefreshError, Refreshed, load, refresh};
use teigeodata_sync::repo::{
    self, Files, RoleKeys, TargetFile, country_role, sign, snapshot_listing, targets,
    targets_delegating, timestamp,
};
use teigeodata_sync::store::Fs;
use teigeodata_sync::trust::{self, Trusted};

use super::store_tests::{Crash, MemFs};

const NOW: i64 = 1_794_268_800;
const YEAR: i64 = 365 * 86_400;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn one(seed: u8) -> RoleKeys {
    RoleKeys {
        keys: vec![key(seed).verifying_key().to_bytes()],
        threshold: 1,
    }
}

/// Root `version`: root keys 1, 2, 3 (threshold 2), timestamp key `ts`, snapshot 20, targets 30;
/// signed by keys 1 and 2.
fn root_file(version: u64, ts: u8) -> Vec<u8> {
    root_keys(version, &[ts])
}

/// [`root_file`] with the timestamp keys `ts` (threshold 1).
fn root_keys(version: u64, ts: &[u8]) -> Vec<u8> {
    let roles = [
        RoleKeys {
            keys: [1, 2, 3]
                .map(|s| key(s).verifying_key().to_bytes())
                .to_vec(),
            threshold: 2,
        },
        RoleKeys {
            keys: ts
                .iter()
                .map(|s| key(*s).verifying_key().to_bytes())
                .collect(),
            threshold: 1,
        },
        one(20),
        one(30),
    ];
    sign(
        &repo::root(version, NOW + YEAR, &roles).unwrap(),
        &[&key(1), &key(2)],
    )
    .unwrap()
}

fn files(paths: &[&str]) -> Files {
    paths
        .iter()
        .map(|p| {
            (
                (*p).to_string(),
                TargetFile {
                    length: p.len() as u64,
                    hashes: repo::target_hashes(p.as_bytes()),
                    custom: None,
                },
            )
        })
        .collect()
}

/// A release on the host: top-level targets, snapshot and timestamp at `v` (timestamp
/// version `ts_v`, signed by `ts_key`), and each role (name, version, packs), written when new.
fn publish(
    host: &mut BTreeMap<String, Vec<u8>>,
    v: u64,
    ts_v: u64,
    ts_key: u8,
    roles: &[(&str, u64, &[&str])],
) {
    let delegated: Vec<_> = roles
        .iter()
        .map(|(n, _, _)| country_role(n.as_bytes().try_into().unwrap(), one(30)))
        .collect();
    let tg = sign(
        &targets_delegating(v, NOW + YEAR, &files(&["earth.places.tgd"]), &delegated).unwrap(),
        &[&key(30)],
    )
    .unwrap();
    let mut listed: Vec<(String, Vec<u8>, u64)> = vec![("targets".into(), tg.clone(), v)];
    for (name, rv, paths) in roles {
        let b = sign(
            &targets(*rv, NOW + YEAR, &files(paths)).unwrap(),
            &[&key(30)],
        )
        .unwrap();
        host.insert(format!("{rv}.{name}.json"), b.clone());
        listed.push(((*name).to_string(), b, *rv));
    }
    let refs: Vec<(&str, &[u8], u64)> = listed
        .iter()
        .map(|(n, b, v)| (n.as_str(), b.as_slice(), *v))
        .collect();
    let sn = sign(
        &snapshot_listing(v, NOW + YEAR, &refs).unwrap(),
        &[&key(20)],
    )
    .unwrap();
    let ts = sign(
        &timestamp(ts_v, NOW + 14 * 86_400, &sn, v).unwrap(),
        &[&key(ts_key)],
    )
    .unwrap();
    host.insert(format!("{v}.targets.json"), tg);
    host.insert(format!("{v}.snapshot.json"), sn);
    host.insert("timestamp.json".into(), ts);
}

/// How a test mirror behaves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Behaves {
    Honest,
    Down,
    /// Serves this file (by its name's start) with one byte changed.
    Tampers(&'static str),
}

/// Metadata mirrors `m0/`, `m1/`, ... serving one repository, each as it behaves.
struct Host {
    files: BTreeMap<String, Vec<u8>>,
    behave: Vec<Behaves>,
    requests: Vec<String>,
}

impl Host {
    fn new(files: BTreeMap<String, Vec<u8>>, behave: &[Behaves]) -> Self {
        Self {
            files,
            behave: behave.to_vec(),
            requests: Vec::new(),
        }
    }
    fn mirrors(&self) -> Mirrors {
        Mirrors::new((0..self.behave.len()).map(|i| format!("m{i}/")))
    }
}

impl Transport for Host {
    fn get(
        &mut self,
        _: &str,
        _: std::ops::Range<u64>,
        _: &mut Vec<u8>,
    ) -> Result<Got, FetchError> {
        Err(FetchError::Response("metadata only"))
    }
    fn fetch(&mut self, url: &str, max: u64, out: &mut Vec<u8>) -> Result<(), FetchError> {
        let (m, name) = url.split_once('/').unwrap();
        let m: usize = m[1..].parse().unwrap();
        self.requests.push(url.to_string());
        out.clear();
        if self.behave[m] == Behaves::Down {
            return Err(FetchError::Transport("unreachable".into()));
        }
        let Some(b) = self.files.get(name) else {
            return Err(FetchError::Status(404));
        };
        out.extend_from_slice(b);
        if let Behaves::Tampers(p) = self.behave[m]
            && name.starts_with(p)
        {
            let at = out.len() / 2;
            out[at] ^= 1;
        }
        if out.len() as u64 > max {
            out.clear();
            return Err(FetchError::Response("longer than allowed"));
        }
        Ok(())
    }
}

fn run(fs: &MemFs, host: &mut Host, want: &[&str]) -> Result<Refreshed, RefreshError> {
    let mut mirrors = host.mirrors();
    let mut fs = fs.clone();
    refresh(&mut fs, host, &mut mirrors, &root_file(1, 10), NOW, want)
}

fn names(t: &Trusted) -> Vec<String> {
    let mut v: Vec<String> = t.trusted_targets().map(|(p, _)| p.to_string()).collect();
    v.sort();
    v
}

/// The steps `refresh` spends making sure its directories exist.
const DIRS: usize = 4;

const NP1: &[&str] = &["np.places.tgd"];
const IN1: &[&str] = &["in.places.tgd"];

#[test]
fn a_refresh_fetches_only_what_changed_and_the_countries_held() {
    let mut files = BTreeMap::new();
    publish(&mut files, 1, 1, 10, &[("np", 1, NP1), ("in", 1, IN1)]);
    let mut host = Host::new(files, &[Behaves::Honest]);
    let fs = MemFs(Rc::default());

    // First run: the chain and Nepal's role, nothing of India.
    let r = run(&fs, &mut host, &["np"]).unwrap();
    assert_eq!(
        r.fetched,
        [
            "timestamp.json",
            "1.snapshot.json",
            "1.targets.json",
            "1.np.json"
        ]
    );
    assert_eq!(names(&r.trusted), ["earth.places.tgd", "np.places.tgd"]);
    assert_eq!(
        host.requests[0], "m0/2.root.json",
        "the root walk asks first"
    );

    // Nothing new: one file, and nothing written but the directory checks.
    let before = fs.0.borrow().ops;
    let r = run(&fs, &mut host, &[]).unwrap();
    assert_eq!(r.fetched, ["timestamp.json"]);
    assert_eq!(
        fs.0.borrow().ops - before,
        DIRS,
        "a write when nothing changed"
    );
    assert!(r.bytes < 1_000, "{}", r.bytes);
    assert_eq!(
        names(&r.trusted),
        ["earth.places.tgd", "np.places.tgd"],
        "Nepal is held"
    );

    // India releases: the chain moves, Nepal's role stays, India's is never fetched.
    publish(
        &mut host.files,
        2,
        2,
        10,
        &[
            ("np", 1, NP1),
            ("in", 2, &["in.places.tgd", "in-mh.places.tgd"]),
        ],
    );
    let r = run(&fs, &mut host, &[]).unwrap();
    assert_eq!(
        r.fetched,
        ["timestamp.json", "2.snapshot.json", "2.targets.json"]
    );

    // Nepal releases: its role comes too.
    publish(
        &mut host.files,
        3,
        3,
        10,
        &[
            ("np", 2, &["np.places.tgd", "np-p1.places.tgd"]),
            ("in", 2, &["in.places.tgd", "in-mh.places.tgd"]),
        ],
    );
    let r = run(&fs, &mut host, &[]).unwrap();
    assert_eq!(
        r.fetched,
        [
            "timestamp.json",
            "3.snapshot.json",
            "3.targets.json",
            "2.np.json"
        ]
    );
    assert_eq!(
        names(&r.trusted),
        ["earth.places.tgd", "np-p1.places.tgd", "np.places.tgd"]
    );

    // Offline: the same catalog from the store alone.
    let t = load(&mut fs.clone(), &root_file(1, 10), NOW).unwrap();
    assert_eq!(names(&t), names(&r.trusted));

    // Asking for India fetches its role once.
    let r = run(&fs, &mut host, &["in"]).unwrap();
    assert_eq!(r.fetched, ["timestamp.json", "2.in.json"]);
    let r = run(&fs, &mut host, &["in", "fj"]).unwrap_err();
    assert!(
        matches!(
            &r,
            RefreshError::Refused {
                error: trust::Error::NoRole,
                ..
            }
        ),
        "{r}"
    );
}

#[test]
fn a_rotation_drops_a_fast_forwarded_timestamp() {
    // The timestamp key 10 is stolen and a timestamp of version 1000 reaches the client.
    let mut files = BTreeMap::new();
    publish(&mut files, 1, 1000, 10, &[("np", 1, NP1)]);
    let mut host = Host::new(files, &[Behaves::Honest]);
    let fs = MemFs(Rc::default());
    run(&fs, &mut host, &["np"]).unwrap();
    // The repository rotates the timestamp key to 11 in root 2 and starts again at version 2.
    // Root 2 is signed by keys 1 and 2, which root 1's threshold also takes.
    host.files.insert("2.root.json".into(), root_file(2, 11));
    publish(&mut host.files, 2, 2, 11, &[("np", 1, NP1)]);
    let r = run(&fs, &mut host, &[]).unwrap();
    assert_eq!(r.trusted.root().common.version, 2);
    assert_eq!(
        r.trusted.timestamp().unwrap().common.version,
        2,
        "recovered"
    );
    assert!(r.fetched.contains(&"2.root.json".to_string()));
    assert_eq!(names(&r.trusted), ["earth.places.tgd", "np.places.tgd"]);
    // And the stored root is now 2: the next run starts there.
    let stored = fs.clone().read("meta/root.json").unwrap().unwrap();
    assert_eq!(stored, host.files["2.root.json"]);
}

#[test]
fn a_rotation_drops_the_stored_timestamp_even_when_it_still_verifies() {
    // Spec 5.3.11: any change of the timestamp keys drops the stored timestamp. Here the
    // stolen key 10 stays listed beside the new key 11, so the fast-forwarded timestamp would
    // still verify and hold the client at version 1000 for good.
    let mut files = BTreeMap::new();
    publish(&mut files, 1, 1000, 10, &[("np", 1, NP1)]);
    let mut host = Host::new(files, &[Behaves::Honest]);
    let fs = MemFs(Rc::default());
    run(&fs, &mut host, &["np"]).unwrap();
    host.files
        .insert("2.root.json".into(), root_keys(2, &[10, 11]));
    publish(&mut host.files, 2, 2, 11, &[("np", 1, NP1)]);
    let r = run(&fs, &mut host, &[]).unwrap();
    assert_eq!(r.trusted.timestamp().unwrap().common.version, 2);
}

#[test]
fn bad_and_dead_mirrors_are_passed_over_and_offline_keeps_what_is_stored() {
    let mut files = BTreeMap::new();
    publish(&mut files, 1, 1, 10, &[("np", 1, NP1)]);
    // m0 changes the snapshot, m1 is down, m2 is honest.
    let mut host = Host::new(
        files,
        &[
            Behaves::Tampers("1.snapshot"),
            Behaves::Down,
            Behaves::Honest,
        ],
    );
    let fs = MemFs(Rc::default());
    let mut mirrors = host.mirrors();
    let r = refresh(
        &mut fs.clone(),
        &mut host,
        &mut mirrors,
        &root_file(1, 10),
        NOW,
        &["np"],
    )
    .unwrap();
    assert_eq!(names(&r.trusted), ["earth.places.tgd", "np.places.tgd"]);
    let m = mirrors.mirrors();
    assert_eq!((m[0].refused, m[1].failures, m[2].refused), (1, 1, 0));
    assert_eq!(mirrors.order()[0], 2, "the honest mirror first now");

    // Every mirror tampers: refused, and nothing unverified is stored.
    publish(&mut host.files, 2, 2, 10, &[("np", 1, NP1)]);
    host.behave = vec![Behaves::Tampers("2.snapshot"); 2];
    let err = run(&fs, &mut host, &[]).unwrap_err();
    assert!(
        matches!(&err, RefreshError::Refused { file, .. } if file == "2.snapshot.json"),
        "{err}"
    );
    // The network gone: an error, and the store still serves the last good catalog.
    host.behave = vec![Behaves::Down];
    let err = run(&fs, &mut host, &[]).unwrap_err();
    assert!(matches!(&err, RefreshError::Network { .. }), "{err}");
    let t = load(&mut fs.clone(), &root_file(1, 10), NOW).unwrap();
    assert_eq!(names(&t), ["earth.places.tgd", "np.places.tgd"]);
}

#[test]
fn a_crash_at_any_write_leaves_a_store_the_next_refresh_completes() {
    let mut first = BTreeMap::new();
    publish(&mut first, 1, 1, 10, &[("np", 1, NP1), ("in", 1, IN1)]);
    let mut second = first.clone();
    publish(
        &mut second,
        2,
        2,
        10,
        &[
            ("np", 2, &["np.places.tgd", "np-p1.places.tgd"]),
            ("in", 1, IN1),
        ],
    );
    // A store that holds release 1 for Nepal, durably.
    let fresh = || {
        let fs = MemFs(Rc::default());
        run(
            &fs,
            &mut Host::new(first.clone(), &[Behaves::Honest]),
            &["np"],
        )
        .unwrap();
        fs.0.borrow_mut().reboot(Crash::Lose);
        fs
    };
    let old = names(&load(&mut fresh(), &root_file(1, 10), NOW).unwrap());
    assert_eq!(old, ["earth.places.tgd", "np.places.tgd"]);
    let mut old_in = old.clone();
    old_in.push("in.places.tgd".to_string());
    old_in.sort();
    let mut host = Host::new(second, &[Behaves::Honest]);
    let fs = fresh();
    let before = fs.0.borrow().ops;
    let want = names(&run(&fs, &mut host, &["in"]).unwrap().trusted);
    let steps = fs.0.borrow().ops - before;
    assert!(steps > 15, "{steps}");
    for how in [Crash::Lose, Crash::Keep] {
        for k in 0..steps {
            let fs = fresh();
            let at = fs.0.borrow().ops + k;
            fs.0.borrow_mut().crash_at = Some(at);
            assert!(
                run(&fs, &mut host, &["in"]).is_err(),
                "{how:?} {k}: no crash"
            );
            fs.0.borrow_mut().reboot(how);
            // Offline, release 1 whole (with India when its role, unchanged in release 2,
            // was stored before the commit) or release 2 whole; never a mix, never nothing.
            let offline = names(&load(&mut fs.clone(), &root_file(1, 10), NOW).unwrap());
            assert!(
                offline == old || offline == old_in || offline == want,
                "{how:?} {k}: {offline:?}"
            );
            // And the next refresh ends where an uncrashed one does.
            let got = run(&fs, &mut host, &["in"]).unwrap();
            assert_eq!(names(&got.trusted), want, "{how:?} {k}");
        }
    }
    // No file is left behind that the chain does not name (no rotation: no stored root).
    let list = |dir: &str| {
        let mut v = fs.clone().list(dir).unwrap();
        v.sort();
        v
    };
    assert_eq!(
        list("meta"),
        ["2.snapshot.json", "2.targets.json", "timestamp.json"]
    );
    assert_eq!(list("meta/roles"), ["1.in.json", "2.np.json"]);
}
