//! The TUF differential against `tough` 0.24 (D-165). Each case writes a repository with our
//! writer (`teigeodata_sync::repo`) against the real clock, then both clients judge it from
//! root 1: our `trust::Trusted` and tough's `RepositoryLoader`. They must reach the case's
//! verdict, and for a repository both accept, read the same bytes for every target. Exits 1 on
//! any disagreement. tough reads the system clock, so its repositories cannot be checked in.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use serde_json::Value;
use teigeodata_sync::repo::{self, RoleKeys, TargetFile};
use teigeodata_sync::trust::Trusted;
use tough::{RepositoryLoader, TargetName};
use url::Url;

const DAY: i64 = 86_400;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(seed: u8) -> [u8; 32] {
    key(seed).verifying_key().to_bytes()
}

/// How a case's repository departs from a valid one.
#[derive(Clone)]
struct Spec {
    /// Seconds from now until each role expires: root, timestamp, snapshot, targets.
    expires: [i64; 4],
    /// The key that signs the timestamp (the root lists 10).
    timestamp_key: u8,
    /// Root 2, when the case rotates: its root keys, the timestamp key it lists, and who signs it.
    root2: Option<(Vec<u8>, u8, Vec<u8>)>,
    /// The targets version the snapshot names (the file written is version 1).
    snapshot_names_targets: u64,
    /// The snapshot names the targets file with the hash of other bytes.
    snapshot_wrong_hash: bool,
    /// The length the timestamp gives the snapshot, relative to the real one.
    timestamp_length: i64,
    /// One target's bytes are changed after signing.
    tamper_target: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            expires: [365 * DAY, 14 * DAY, 30 * DAY, 30 * DAY],
            timestamp_key: 10,
            root2: None,
            snapshot_names_targets: 1,
            snapshot_wrong_hash: false,
            timestamp_length: 0,
            tamper_target: false,
        }
    }
}

fn roles(root_keys: &[u8], ts: u8) -> [RoleKeys; 4] {
    let one = |s: u8| RoleKeys {
        keys: vec![public(s)],
        threshold: 1,
    };
    [
        RoleKeys {
            keys: root_keys.iter().map(|&s| public(s)).collect(),
            threshold: 2,
        },
        one(ts),
        one(20),
        one(30),
    ]
}

fn signed(v: &Value, keys: &[u8]) -> Vec<u8> {
    let ks: Vec<SigningKey> = keys.iter().map(|&s| key(s)).collect();
    let refs: Vec<&SigningKey> = ks.iter().collect();
    repo::sign(v, &refs).expect("sign")
}

/// The target files: path → bytes.
fn files() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    f.insert("np.places.tgd".to_string(), b"a pack of Nepal".repeat(200));
    f.insert(
        "ae-du.places.tgd".to_string(),
        (0..=255_u8).cycle().take(70_000).collect(),
    );
    f
}

/// Writes `dir/metadata` and `dir/targets` (targets named `<sha256>.<path>`, as consistent
/// snapshots name them).
fn write(dir: &Path, s: &Spec, now: i64, timestamp_version: u64) {
    let (meta, tdir) = (dir.join("metadata"), dir.join("targets"));
    std::fs::create_dir_all(&meta).expect("mkdir");
    std::fs::create_dir_all(&tdir).expect("mkdir");
    let put = |name: &str, b: &[u8]| std::fs::write(meta.join(name), b).expect("write");
    let r1 = repo::root(1, now + s.expires[0], &roles(&[1, 2, 3], 10)).expect("root");
    put("1.root.json", &signed(&r1, &[1, 2]));
    if let Some((rk, ts, signers)) = &s.root2 {
        let r2 = repo::root(2, now + s.expires[0], &roles(rk, *ts)).expect("root 2");
        put("2.root.json", &signed(&r2, signers));
    }
    let mut listed = BTreeMap::new();
    for (path, mut bytes) in files() {
        let h = repo::target_hashes(&bytes);
        let hex = h["sha256"].iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });
        listed.insert(
            path.clone(),
            TargetFile {
                length: bytes.len() as u64,
                hashes: h,
                custom: None,
            },
        );
        if s.tamper_target && path.starts_with("np") {
            bytes[7] ^= 1;
        }
        std::fs::write(tdir.join(format!("{hex}.{path}")), &bytes).expect("target");
    }
    let tg = signed(
        &repo::targets(1, now + s.expires[3], &listed).expect("targets"),
        &[30],
    );
    put("1.targets.json", &tg);
    // Other bytes of the same length, so only the hash can tell.
    let mut named = tg.clone();
    if s.snapshot_wrong_hash {
        named[0] ^= 1;
    }
    let sn_signed =
        repo::snapshot(1, now + s.expires[2], &named, s.snapshot_names_targets).expect("snapshot");
    if s.snapshot_names_targets != 1 {
        // The snapshot names a version whose file holds version 1.
        put(&format!("{}.targets.json", s.snapshot_names_targets), &tg);
    }
    let sn = signed(&sn_signed, &[20]);
    put("1.snapshot.json", &sn);
    let mut ts = repo::timestamp(timestamp_version, now + s.expires[1], &sn, 1).expect("ts");
    if s.timestamp_length != 0 {
        let n = i64::try_from(sn.len()).expect("length") + s.timestamp_length;
        ts["meta"]["snapshot.json"]["length"] = Value::from(n);
    }
    put("timestamp.json", &signed(&ts, &[s.timestamp_key]));
}

/// A verdict and, when accepted, every target's bytes.
type Judged = Result<BTreeMap<String, Vec<u8>>, String>;

/// Our client, as a new install walks: root 1, the roots that follow, timestamp, snapshot,
/// targets, then each target. `stored` is a timestamp kept from an earlier session.
fn ours(dir: &Path, now: i64, stored: Option<&[u8]>) -> Judged {
    let meta = dir.join("metadata");
    let read = |n: &str| std::fs::read(meta.join(n)).map_err(|e| format!("{n}: {e}"));
    let e = |what: &str| {
        let what = what.to_string();
        move |err: teigeodata_sync::trust::Error| format!("{what}: {err}")
    };
    let mut t = Trusted::new(&read("1.root.json")?, now).map_err(e("1.root.json"))?;
    loop {
        let next = t.next_root_name();
        let Ok(data) = read(&next) else { break };
        t.update_root(&data).map_err(e(&next))?;
    }
    if let Some(old) = stored {
        t.update_timestamp(old).map_err(e("stored timestamp"))?;
    }
    t.update_timestamp(&read("timestamp.json")?)
        .map_err(e("timestamp.json"))?;
    let sn = t.snapshot_name().map_err(e("snapshot name"))?;
    t.update_snapshot(&read(&sn)?, false).map_err(e(&sn))?;
    let tg = t.targets_name().map_err(e("targets name"))?;
    t.update_targets(&read(&tg)?).map_err(e(&tg))?;
    let paths: Vec<String> = t
        .targets()
        .ok_or("no targets")?
        .targets
        .keys()
        .cloned()
        .collect();
    let mut out = BTreeMap::new();
    for p in paths {
        let name = t.target_name(&p).map_err(e(&p))?;
        let data = std::fs::read(dir.join("targets").join(&name)).map_err(|x| x.to_string())?;
        t.verify_target(&p, &data).map_err(e(&p))?;
        out.insert(p, data);
    }
    Ok(out)
}

/// tough, from root 1 with `datastore` (kept across sessions for the rollback case).
async fn theirs(dir: &Path, datastore: &Path) -> Judged {
    let url = |p: PathBuf| Url::from_directory_path(p).map_err(|()| "url".to_string());
    std::fs::create_dir_all(datastore).map_err(|e| e.to_string())?;
    let root = std::fs::read(dir.join("metadata/1.root.json")).map_err(|e| e.to_string())?;
    let repo = RepositoryLoader::new(&root, url(dir.join("metadata"))?, url(dir.join("targets"))?)
        .datastore(datastore)
        .load()
        .await
        .map_err(|e| e.to_string())?;
    let names: Vec<TargetName> = repo.all_targets().map(|(n, _)| n.clone()).collect();
    let mut out = BTreeMap::new();
    for n in names {
        let stream = repo
            .read_target(&n)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no such target")?;
        let bytes = tough::IntoVec::into_vec(stream)
            .await
            .map_err(|e| e.to_string())?;
        out.insert(n.raw().to_string(), bytes);
    }
    Ok(out)
}

fn verdict(j: &Judged) -> &'static str {
    if j.is_ok() { "ok" } else { "reject" }
}

/// The verdict and, for a refusal, why: a refusal for the wrong reason is a failure too.
fn show(j: &Judged) -> String {
    match j {
        Ok(_) => "ok".to_string(),
        Err(e) => {
            // The end of the message: tough puts the URL first and the cause last.
            let n = e.chars().count();
            format!(
                "reject: …{}",
                e.chars().skip(n.saturating_sub(70)).collect::<String>()
            )
        }
    }
}

/// Each case: its name, the verdict both clients must reach, and its repository.
#[allow(clippy::too_many_lines, reason = "a table of cases")]
fn cases() -> Vec<(&'static str, &'static str, Spec)> {
    let d = Spec::default;
    let past = |role: usize| {
        let mut s = d();
        s.expires[role] = -DAY;
        s
    };
    vec![
        ("valid", "ok", d()),
        (
            "target-tampered",
            "reject",
            Spec {
                tamper_target: true,
                ..d()
            },
        ),
        (
            "timestamp-wrong-key",
            "reject",
            Spec {
                timestamp_key: 20,
                ..d()
            },
        ),
        ("root-expired", "reject", past(0)),
        ("timestamp-expired", "reject", past(1)),
        ("snapshot-expired", "reject", past(2)),
        ("targets-expired", "reject", past(3)),
        (
            // Root 2 drops key 3 for 4 and rotates the timestamp key: keys 1 and 2 meet both
            // roots' thresholds; the timestamp is signed by the new key.
            "root-rotation",
            "ok",
            Spec {
                root2: Some((vec![1, 2, 4], 11, vec![1, 2])),
                timestamp_key: 11,
                ..d()
            },
        ),
        (
            "root-rotation-old-timestamp-key",
            "reject",
            Spec {
                root2: Some((vec![1, 2, 4], 11, vec![1, 2])),
                ..d()
            },
        ),
        (
            "root-new-keys-only",
            "reject",
            Spec {
                root2: Some((vec![4, 5, 6], 10, vec![4, 5])),
                ..d()
            },
        ),
        (
            "root-below-old-threshold",
            "reject",
            Spec {
                root2: Some((vec![1, 4, 5], 10, vec![1, 4])),
                ..d()
            },
        ),
        (
            "root-one-key-twice",
            "reject",
            Spec {
                root2: Some((vec![1, 2, 3], 10, vec![1, 1])),
                ..d()
            },
        ),
        (
            "snapshot-wrong-targets-hash",
            "reject",
            Spec {
                snapshot_wrong_hash: true,
                ..d()
            },
        ),
        (
            "snapshot-names-a-version-the-file-is-not",
            "reject",
            Spec {
                snapshot_names_targets: 2,
                ..d()
            },
        ),
        (
            // The length is the most to download; the hashes bind the bytes. Both clients
            // accept a longer one; the spec asks that hashes and version match (5.5.2).
            "timestamp-length-above",
            "ok",
            Spec {
                timestamp_length: 1,
                ..d()
            },
        ),
        (
            "timestamp-length-below",
            "reject",
            Spec {
                timestamp_length: -1,
                ..d()
            },
        ),
    ]
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        .cast_signed();
    let base = std::env::temp_dir().join(format!("tough-oracle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let cases = cases();
    let d = Spec::default;
    let mut wrong = 0;
    println!("| case | expected | ours | tough |");
    println!("|---|---|---|---|");
    let mut report = |name: &str, want: &str, a: &Judged, b: &Judged| {
        let same_bytes = match (a, b) {
            (Ok(x), Ok(y)) => x == y && x.len() == files().len(),
            _ => true,
        };
        let ok = verdict(a) == want && verdict(b) == want && same_bytes;
        println!(
            "| {name} | {want} | {} | {} |{}",
            show(a),
            show(b),
            if ok { "" } else { " **DISAGREES**" }
        );
        if !ok {
            wrong += 1;
            eprintln!(
                "{name}:\n  ours:  {:?}\n  tough: {:?}",
                a.as_ref().err(),
                b.as_ref().err()
            );
        }
    };
    for (name, want, spec) in &cases {
        let dir = base.join(name);
        write(&dir, spec, now, 1);
        let a = ours(&dir, now, None);
        let b = theirs(&dir, &dir.join("datastore")).await;
        report(name, want, &a, &b);
    }
    // Rollback: a client that saw timestamp 2 is then served timestamp 1.
    let (new, old) = (base.join("rollback-new"), base.join("rollback-old"));
    write(&new, &d(), now, 2);
    write(&old, &d(), now, 1);
    let store = base.join("rollback-datastore");
    let first = theirs(&new, &store).await;
    report(
        "rollback (first session)",
        "ok",
        &ours(&new, now, None),
        &first,
    );
    let stored = std::fs::read(new.join("metadata/timestamp.json")).expect("ts");
    let a = ours(&old, now, Some(&stored));
    let b = theirs(&old, &store).await;
    report("timestamp-rollback", "reject", &a, &b);
    let _ = std::fs::remove_dir_all(&base);
    if wrong > 0 {
        eprintln!("{wrong} case(s) disagree");
        std::process::exit(1);
    }
    println!("\nall {} cases agree", cases.len() + 2);
}
