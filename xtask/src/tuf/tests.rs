use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use teigeodata_format::{Encoding, Kind, Tag, Writer};
use teigeodata_sync::trust::RoleName;

use super::{Keys, release, sign_file, timestamp, verify, write_root};

const NOW: i64 = 1_794_268_800; // 2026-11-10
const DAY: i64 = 86_400;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(seed: u8) -> [u8; 32] {
    key(seed).verifying_key().to_bytes()
}

/// A fresh directory under the system's temporary directory.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("teigeodata-tuf-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pack(dir: &Path, release: &str, fill: u8) -> PathBuf {
    let ids = vec![fill; 40_000];
    let tz = vec![fill.wrapping_add(1); 3_000];
    let mut w = Writer::new("zz", "places", release).unwrap();
    w.manifest(Tag::LICENCE, b"odbl").unwrap();
    let bytes = w
        .pack(
            &[
                (Kind::IDMAP, Encoding::RAW, 1, &ids),
                (Kind::PLACE_TZ, Encoding::RAW, 1, &tz),
            ],
            None,
        )
        .unwrap();
    let p = dir.join(format!("zz-{release}.tgd"));
    std::fs::write(&p, bytes).unwrap();
    p
}

fn keys(ts: u8) -> Keys {
    Keys {
        targets: key(30),
        snapshot: key(20),
        timestamp: key(ts),
    }
}

/// Root 1: root keys 1, 2, 3 (threshold 2), timestamp `ts`, snapshot 20, targets 30.
fn root(repo: &Path, root_keys: &[u8], ts: u8) {
    let rk: Vec<[u8; 32]> = root_keys.iter().map(|&s| public(s)).collect();
    write_root(
        repo,
        &rk,
        2,
        [public(ts), public(20), public(30)],
        NOW + 365 * DAY,
    )
    .unwrap();
}

#[test]
fn a_release_publishes_a_verified_catalog_and_the_drill_rotates_keys() {
    let d = scratch("drill");
    let (repo, assets) = (d.join("metadata"), d.join("assets"));
    root(&repo, &[1, 2, 3], 10);
    let r1 = repo.join("1.root.json");
    // An unsigned root does not publish.
    let p1 = pack(&d, "2026.11.0", 7);
    assert!(release(&repo, &assets, std::slice::from_ref(&p1), &keys(10), NOW).is_err());
    // The refused release left nothing behind.
    let left: Vec<_> = std::fs::read_dir(&repo)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["1.root.json"]);
    sign_file(&r1, RoleName::Root, &key(1)).unwrap();
    sign_file(&r1, RoleName::Root, &key(2)).unwrap();
    let out = release(&repo, &assets, &[p1], &keys(10), NOW).unwrap();
    assert!(out.contains("1 targets"), "{out}");

    // A second release replaces the pack's entry; versions rise.
    let p2 = pack(&d, "2026.12.0", 9);
    let out = release(&repo, &assets, &[p2], &keys(10), NOW + DAY).unwrap();
    assert!(
        out.contains("2.snapshot.json") && out.contains("2.targets.json"),
        "{out}"
    );

    // The daily timestamp, signed with the wrong key, is refused by the self-check.
    let before = std::fs::read(repo.join("timestamp.json")).unwrap();
    assert!(timestamp(&repo, &key(20), NOW + 2 * DAY, 14).is_err());
    assert_eq!(std::fs::read(repo.join("timestamp.json")).unwrap(), before);
    timestamp(&repo, &key(10), NOW + 2 * DAY, 14).unwrap();

    // The drill: root 2 drops root key 3 for 4 and rotates the timestamp key to 11.
    root(&repo, &[1, 2, 4], 11);
    let r2 = repo.join("2.root.json");
    for k in [4, 1] {
        sign_file(&r2, RoleName::Root, &key(k)).unwrap();
    }
    // Key 4 and key 1: root 2's threshold is met, root 1's is not (4 is not in it).
    assert!(verify(&repo, None, NOW + 3 * DAY, 7).is_err());
    sign_file(&r2, RoleName::Root, &key(2)).unwrap();
    // Root 2 is valid now, but the timestamp is still signed by the old key 10.
    let err = verify(&repo, None, NOW + 3 * DAY, 7).unwrap_err();
    assert!(err.contains("timestamp"), "{err}");
    timestamp(&repo, &key(11), NOW + 3 * DAY, 14).unwrap();
    let out = verify(&repo, Some(&assets), NOW + 3 * DAY, 7).unwrap();
    assert!(
        out.contains("root 2") && out.contains("every asset"),
        "{out}"
    );

    // Expiry: 8 days later the timestamp has 6 days left, under the week verify demands.
    let err = verify(&repo, None, NOW + 11 * DAY, 7).unwrap_err();
    assert!(err.contains("timestamp expires in 6 days"), "{err}");

    // A tampered asset of the current release fails.
    let any = std::fs::read_dir(&assets)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().contains("2026.12.0"))
        .unwrap();
    let mut b = std::fs::read(&any).unwrap();
    let last = b.len() - 1;
    b[last] ^= 1;
    std::fs::write(&any, b).unwrap();
    assert!(verify(&repo, Some(&assets), NOW + 3 * DAY, 7).is_err());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn an_asset_is_never_overwritten_with_other_bytes() {
    let d = scratch("immutable");
    let (repo, assets) = (d.join("metadata"), d.join("assets"));
    root(&repo, &[1, 2, 3], 10);
    let r1 = repo.join("1.root.json");
    sign_file(&r1, RoleName::Root, &key(1)).unwrap();
    sign_file(&r1, RoleName::Root, &key(2)).unwrap();
    let p = pack(&d, "2026.11.0", 7);
    release(&repo, &assets, std::slice::from_ref(&p), &keys(10), NOW).unwrap();
    let file = std::fs::read_dir(&assets)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&file, b"other bytes").unwrap();
    let err = release(&repo, &assets, &[p], &keys(10), NOW).unwrap_err();
    assert!(err.contains("exists with other bytes"), "{err}");
    std::fs::remove_dir_all(&d).unwrap();
}

/// The commands as a maintainer types them, through the argument parser and the real clock;
/// `env:` keys are read from the environment, as in the workflow.
#[test]
fn the_command_line_runs_a_ceremony_and_a_release() {
    let d = scratch("cli");
    let path = |p: &str| d.join(p).to_string_lossy().into_owned();
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        super::dispatch(&args)
    };
    let mut publics = Vec::new();
    for name in ["r1", "r2", "r3", "ts", "sn", "tg"] {
        let out = run(&["keygen", &path(&format!("{name}.key"))]).unwrap();
        publics.push(
            out.lines()
                .nth(1)
                .unwrap()
                .trim_start_matches("public ")
                .to_string(),
        );
    }
    // A key file is never overwritten.
    assert!(run(&["keygen", &path("r1.key")]).is_err());
    let shown = run(&["pubkey", &path("r1.key")]).unwrap();
    assert!(shown.contains(&publics[0]));
    let repo = path("metadata");
    let p = |i: usize| publics[i].as_str();
    run(&[
        "root",
        &repo,
        "--root",
        p(0),
        "--root",
        p(1),
        "--root",
        p(2),
        "--timestamp",
        p(3),
        "--snapshot",
        p(4),
        "--targets",
        p(5),
    ])
    .unwrap();
    let r1 = format!("{repo}/1.root.json");
    run(&["sign", &r1, "--role", "root", "--key", &path("r1.key")]).unwrap();
    run(&["sign", &r1, "--role", "root", "--key", &path("r3.key")]).unwrap();
    let pack = pack(&d, "2026.11.0", 3).to_string_lossy().into_owned();
    // `env:` reads the variable; PATH is set and is not a key.
    let err = run(&["timestamp", &repo, "--key", "env:PATH"]).unwrap_err();
    assert!(err.contains("PATH: a key is 64 hex digits"), "{err}");
    let env_key = path("ts.key");
    let out = run(&[
        "release",
        &repo,
        "--assets",
        &path("assets"),
        "--pack",
        &pack,
        "--targets-key",
        &path("tg.key"),
        "--snapshot-key",
        &path("sn.key"),
        "--timestamp-key",
        &env_key,
    ])
    .unwrap();
    assert!(out.contains("verified: root 1"), "{out}");
    run(&["timestamp", &repo, "--key", &env_key]).unwrap();
    let out = run(&["verify", &repo, "--assets", &path("assets")]).unwrap();
    assert!(out.contains("every asset"), "{out}");
    // Fifteen days ask for more than a 14-day timestamp has.
    assert!(run(&["verify", &repo, "--min-days", "15"]).is_err());
    assert!(run(&["frobnicate"]).is_err());
    std::fs::remove_dir_all(&d).unwrap();
}
