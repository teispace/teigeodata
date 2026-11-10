//! `cargo xtask tuf …`: the catalog repository (D-163,
//! [catalog repository tooling](../../docs/02-research/notes/catalog-repository-tooling.md)).
//!
//! The repository is a directory of metadata as GitHub Pages serves it: `N.root.json` for every
//! root, `N.targets.json`, `N.snapshot.json` and `timestamp.json`. Transport assets go to a
//! separate directory, named as the catalog says (`address.component.release.hash8.tgd.zst`),
//! for the release upload. Keys are 32-byte Ed25519 seeds in hex, in a file or in an
//! environment variable (`env:NAME`), never in the repository. Every command that writes ends by
//! verifying the whole repository with the client, from root 1.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use serde_json::Value;
use teigeodata_format::{Pack, Tag};
use teigeodata_sync::catalog::{Entry, file_name};
use teigeodata_sync::repo::{self, RoleKeys, TargetFile};
use teigeodata_sync::trust::{Hashes, RoleName, Trusted};

use crate::wire::Wire;

const DAY: i64 = 86_400;

/// Default lifetimes in days: root and the offline roles a year, the timestamp 14 days
/// (ADR-0037), refreshed daily by the timestamp workflow.
const ROOT_DAYS: i64 = 365;
const OFFLINE_DAYS: i64 = 365;
const TIMESTAMP_DAYS: i64 = 14;
/// `verify` fails when any role expires sooner than this: a week to notice and fix.
const MIN_DAYS: i64 = 7;

const USAGE: &str = "\
cargo xtask tuf <command>
  keygen <out.key>                       a new Ed25519 key (32-byte seed, hex), printed public key
  pubkey <key>                           the public key and key ID of a key
  root <repo> --root PUB… [--root-threshold 2] --timestamp PUB --snapshot PUB --targets PUB
       [--days 365]                      writes the next root, unsigned
  sign <file> --role root|timestamp|snapshot|targets --key KEY
                                         adds a signature in place
  release <repo> --assets DIR --pack P.tgd… --targets-key KEY --snapshot-key KEY
       --timestamp-key KEY               assets and catalog entries from packs; new targets,
                                         snapshot and timestamp
  timestamp <repo> --key KEY [--days 14] a fresh timestamp for the same snapshot
  verify <repo> [--assets DIR] [--min-days 7]
                                         the whole chain with the client, every entry, the assets
KEY is a key file or env:NAME; PUB is a public key in hex.";

pub(crate) fn run(args: &[String]) -> Result<String, Vec<String>> {
    dispatch(args).map_err(|e| vec![e])
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let now: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs()
        .try_into()
        .map_err(|_| "the clock is past 2262".to_string())?;
    let a = Args(args.get(1..).unwrap_or_default());
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(Path::new(a.pos(0)?)),
        Some("pubkey") => {
            let k = load_key(a.pos(0)?)?;
            Ok(describe(&k))
        }
        Some("root") => write_root(
            Path::new(a.pos(0)?),
            &a.all("--root")
                .iter()
                .map(|p| public(p))
                .collect::<Result<Vec<_>, _>>()?,
            a.num("--root-threshold", 2)?,
            [
                public(a.req("--timestamp")?)?,
                public(a.req("--snapshot")?)?,
                public(a.req("--targets")?)?,
            ],
            now + a.num("--days", ROOT_DAYS)? * DAY,
        ),
        Some("sign") => sign_file(
            Path::new(a.pos(0)?),
            role(a.req("--role")?)?,
            &load_key(a.req("--key")?)?,
        ),
        Some("release") => release(
            Path::new(a.pos(0)?),
            Path::new(a.req("--assets")?),
            &a.all("--pack")
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
            &Keys {
                targets: load_key(a.req("--targets-key")?)?,
                snapshot: load_key(a.req("--snapshot-key")?)?,
                timestamp: load_key(a.req("--timestamp-key")?)?,
            },
            now,
        ),
        Some("timestamp") => timestamp(
            Path::new(a.pos(0)?),
            &load_key(a.req("--key")?)?,
            now,
            a.num("--days", TIMESTAMP_DAYS)?,
        ),
        Some("verify") => verify(
            Path::new(a.pos(0)?),
            a.opt("--assets").map(Path::new),
            now,
            a.num("--min-days", MIN_DAYS)?,
        ),
        _ => Err(USAGE.to_string()),
    }
}

/// The command's arguments after its name.
struct Args<'a>(&'a [String]);

impl Args<'_> {
    /// The `i`-th positional argument (not an option or an option's value).
    fn pos(&self, i: usize) -> Result<&str, String> {
        let mut skip = false;
        self.0
            .iter()
            .filter(|a| {
                let keep = !skip && !a.starts_with("--");
                skip = a.starts_with("--");
                keep
            })
            .nth(i)
            .map(String::as_str)
            .ok_or_else(|| USAGE.to_string())
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.0
            .windows(2)
            .filter(|w| w.first().is_some_and(|f| f == name))
            .filter_map(|w| w.get(1).map(String::as_str))
            .collect()
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.all(name).last().copied()
    }

    fn req(&self, name: &str) -> Result<&str, String> {
        self.opt(name)
            .ok_or_else(|| format!("{name} is required\n\n{USAGE}"))
    }

    fn num(&self, name: &str, default: i64) -> Result<i64, String> {
        self.opt(name).map_or(Ok(default), |v| {
            v.parse()
                .map_err(|_| format!("{name}: `{v}` is not a number"))
        })
    }
}

/// The signing keys of a release.
pub(crate) struct Keys {
    pub(crate) targets: SigningKey,
    pub(crate) snapshot: SigningKey,
    pub(crate) timestamp: SigningKey,
}

fn hex32(s: &str) -> Result<[u8; 32], String> {
    let s = s.trim();
    let bad = || "a key is 64 hex digits".to_string();
    if s.len() != 64 {
        return Err(bad());
    }
    let mut out = [0_u8; 32];
    for (o, i) in out.iter_mut().zip((0..64).step_by(2)) {
        *o = u8::from_str_radix(s.get(i..i + 2).ok_or_else(bad)?, 16).map_err(|_| bad())?;
    }
    Ok(out)
}

/// A key from `env:NAME` or a file.
fn load_key(source: &str) -> Result<SigningKey, String> {
    let text = match source.strip_prefix("env:") {
        Some(var) => std::env::var(var).map_err(|_| format!("{var} is not set"))?,
        None => fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))?,
    };
    Ok(SigningKey::from_bytes(
        &hex32(&text).map_err(|e| format!("{source}: {e}"))?,
    ))
}

fn public(hex: &str) -> Result<[u8; 32], String> {
    hex32(hex).map_err(|e| format!("public key `{hex}`: {e}"))
}

fn role(name: &str) -> Result<RoleName, String> {
    Ok(match name {
        "root" => RoleName::Root,
        "timestamp" => RoleName::Timestamp,
        "snapshot" => RoleName::Snapshot,
        "targets" => RoleName::Targets,
        _ => return Err(format!("no role `{name}`")),
    })
}

fn describe(k: &SigningKey) -> String {
    let p = k.verifying_key().to_bytes();
    format!("public {}\nkey ID {}", hex(&p), repo::key_id(&p))
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

fn keygen(out: &Path) -> Result<String, String> {
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no randomness: {e}"))?;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts
        .open(out)
        .map_err(|e| format!("{}: {e}", out.display()))?;
    std::io::Write::write_all(&mut f, format!("{}\n", hex(&seed)).as_bytes())
        .map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(format!(
        "wrote {} (keep it offline)\n{}",
        out.display(),
        describe(&SigningKey::from_bytes(&seed))
    ))
}

fn read(p: &Path) -> Result<Vec<u8>, String> {
    fs::read(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write(p: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(p, bytes).map_err(|e| format!("{}: {e}", p.display()))
}

/// The highest `N` of the `N.<role>.json` files in `repo`, 0 when none.
fn latest(repo: &Path, role: &str) -> Result<u64, String> {
    let suffix = format!(".{role}.json");
    let mut n = 0;
    for e in fs::read_dir(repo).map_err(|e| format!("{}: {e}", repo.display()))? {
        let name = e.map_err(|e| e.to_string())?.file_name();
        if let Some(v) = name
            .to_str()
            .and_then(|s| s.strip_suffix(&suffix))
            .and_then(|v| v.parse().ok())
        {
            n = n.max(v);
        }
    }
    Ok(n)
}

pub(crate) fn write_root(
    repo: &Path,
    root_keys: &[[u8; 32]],
    threshold: i64,
    [ts, sn, tg]: [[u8; 32]; 3],
    expires: i64,
) -> Result<String, String> {
    fs::create_dir_all(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    let version = latest(repo, "root")? + 1;
    let threshold = usize::try_from(threshold).map_err(|_| "a negative threshold")?;
    let one = |k: [u8; 32]| RoleKeys {
        keys: vec![k],
        threshold: 1,
    };
    let roles = [
        RoleKeys {
            keys: root_keys.to_vec(),
            threshold,
        },
        one(ts),
        one(sn),
        one(tg),
    ];
    let signed = repo::root(version, expires, &roles).map_err(|e| e.to_string())?;
    let path = repo.join(format!("{version}.root.json"));
    write(&path, &repo::sign(&signed, &[]).map_err(|e| e.to_string())?)?;
    Ok(format!(
        "wrote {} unsigned: it needs {threshold} of its own root keys{}",
        path.display(),
        if version > 1 {
            " and the threshold of the previous root"
        } else {
            ""
        }
    ))
}

pub(crate) fn sign_file(file: &Path, role: RoleName, key: &SigningKey) -> Result<String, String> {
    let signed = repo::add_signature(&read(file)?, role, key).map_err(|e| e.to_string())?;
    write(file, &signed)?;
    Ok(format!(
        "signed {} with key {}",
        file.display(),
        repo::key_id(key.verifying_key().as_bytes())
    ))
}

/// The `signed` object of a metadata file.
fn signed_of(bytes: &[u8]) -> Result<Value, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    v.get("signed")
        .cloned()
        .ok_or_else(|| "no signed object".to_string())
}

fn version_of(signed: &Value) -> Result<u64, String> {
    signed
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "no version".to_string())
}

fn hashes_of(v: &Value) -> Result<Hashes, String> {
    let mut out = Hashes::new();
    for (alg, h) in v.as_object().ok_or("hashes is not an object")? {
        let h = h.as_str().ok_or("a hash is not a string")?;
        let bytes = (0..h.len())
            .step_by(2)
            .map(|i| h.get(i..i + 2).and_then(|p| u8::from_str_radix(p, 16).ok()))
            .collect::<Option<Vec<u8>>>()
            .ok_or("a hash is not hex")?;
        out.insert(alg.clone(), bytes);
    }
    Ok(out)
}

/// The current targets (by path) and versions of targets, snapshot and timestamp; empty and 0
/// before the first release.
fn current(repo: &Path) -> Result<(BTreeMap<String, TargetFile>, [u64; 3]), String> {
    let ts_path = repo.join("timestamp.json");
    if !ts_path.exists() {
        return Ok((BTreeMap::new(), [0; 3]));
    }
    let ts = signed_of(&read(&ts_path)?)?;
    let sn_version = ts
        .pointer("/meta/snapshot.json/version")
        .and_then(Value::as_u64)
        .ok_or("the timestamp names no snapshot")?;
    let sn = signed_of(&read(&repo.join(format!("{sn_version}.snapshot.json")))?)?;
    let tg_version = sn
        .pointer("/meta/targets.json/version")
        .and_then(Value::as_u64)
        .ok_or("the snapshot names no targets")?;
    let tg = signed_of(&read(&repo.join(format!("{tg_version}.targets.json")))?)?;
    let mut files = BTreeMap::new();
    for (path, t) in tg
        .get("targets")
        .and_then(Value::as_object)
        .ok_or("no targets")?
    {
        files.insert(
            path.clone(),
            TargetFile {
                length: t
                    .get("length")
                    .and_then(Value::as_u64)
                    .ok_or("a target without length")?,
                hashes: hashes_of(t.get("hashes").ok_or("a target without hashes")?)?,
                custom: t.get("custom").cloned(),
            },
        );
    }
    Ok((files, [tg_version, sn_version, version_of(&ts)?]))
}

/// The SPDX identifier of a manifest's licence class (ADR-0002).
fn spdx(class: &str) -> Result<&'static str, String> {
    match class {
        "odbl" => Ok("ODbL-1.0"),
        "cc-by-4.0" => Ok("CC-BY-4.0"),
        other => Err(format!("licence `{other}` has no SPDX mapping")),
    }
}

/// The transport asset and catalog entry of a pack.
pub(crate) fn entry_of(bytes: &[u8]) -> Result<(Entry, Vec<u8>, Hashes), String> {
    let pack = Pack::open_verified(bytes).map_err(|e| e.to_string())?;
    let m = pack.manifest();
    let (wire, asset) = Wire::measure_asset(bytes)?;
    let c = wire.catalog_sizes();
    let hashes = repo::target_hashes(&asset);
    let sha = hashes.get("sha256").ok_or("no sha256")?;
    let licence = spdx(m.text(Tag::LICENCE).ok_or("the pack states no licence")?)?;
    let mut head = [0_u8; 16];
    head.copy_from_slice(pack.head_digest().get(..16).ok_or("short digest")?);
    let entry = Entry {
        address: m.address().to_string(),
        component: m.component().to_string(),
        release: m.release().to_string(),
        file: file_name(m.address(), m.component(), m.release(), sha),
        pack_length: bytes.len() as u64,
        head,
        seek_table: c.seek_table,
        head_frame: c.head_frame,
        block_hashes: c.block_hashes,
        sizes: c.sizes,
        deps: vec![],
        min_reader: pack.min_reader_major(),
        bbox: m.bbox(),
        licence: licence.to_string(),
    };
    Ok((entry, asset, hashes))
}

pub(crate) fn release(
    repo: &Path,
    assets: &Path,
    packs: &[PathBuf],
    keys: &Keys,
    now: i64,
) -> Result<String, String> {
    if packs.is_empty() {
        return Err("no --pack".into());
    }
    fs::create_dir_all(assets).map_err(|e| format!("{}: {e}", assets.display()))?;
    let (mut files, [targets_v, snapshot_v, timestamp_v]) = current(repo)?;
    let mut out = String::new();
    for p in packs {
        let (entry, asset, hashes) =
            entry_of(&read(p)?).map_err(|e| format!("{}: {e}", p.display()))?;
        let file = TargetFile {
            length: asset.len() as u64,
            hashes,
            custom: Some(entry.to_custom()),
        };
        // The client must read what we write.
        let target = teigeodata_sync::trust::Target {
            length: file.length,
            hashes: file.hashes.clone(),
            custom: file.custom.clone(),
        };
        Entry::from_target(&entry.path(), &target).map_err(|e| format!("{}: {e}", p.display()))?;
        let dest = assets.join(&entry.file);
        match fs::read(&dest) {
            Ok(old) if old != asset => {
                return Err(format!("{} exists with other bytes", dest.display()));
            }
            Ok(_) => {}
            Err(_) => write(&dest, &asset)?,
        }
        let _ = writeln!(out, "{} → {} ({} B)", entry.path(), entry.file, asset.len());
        files.insert(entry.path(), file);
    }
    let tg = repo::sign(
        &repo::targets(targets_v + 1, now + OFFLINE_DAYS * DAY, &files)
            .map_err(|e| e.to_string())?,
        &[&keys.targets],
    )
    .map_err(|e| e.to_string())?;
    let sn = repo::sign(
        &repo::snapshot(snapshot_v + 1, now + OFFLINE_DAYS * DAY, &tg, targets_v + 1)
            .map_err(|e| e.to_string())?,
        &[&keys.snapshot],
    )
    .map_err(|e| e.to_string())?;
    let ts = repo::sign(
        &repo::timestamp(
            timestamp_v + 1,
            now + TIMESTAMP_DAYS * DAY,
            &sn,
            snapshot_v + 1,
        )
        .map_err(|e| e.to_string())?,
        &[&keys.timestamp],
    )
    .map_err(|e| e.to_string())?;
    let written = [
        (repo.join(format!("{}.targets.json", targets_v + 1)), tg),
        (repo.join(format!("{}.snapshot.json", snapshot_v + 1)), sn),
    ];
    commit(repo, &written, &ts, || {
        verify(repo, Some(assets), now, MIN_DAYS)
    })
    .map(|v| out + &v)
}

pub(crate) fn timestamp(
    repo: &Path,
    key: &SigningKey,
    now: i64,
    days: i64,
) -> Result<String, String> {
    let (_, [_, sn_v, ts_v]) = current(repo)?;
    if ts_v == 0 {
        return Err("no timestamp yet: run a release first".into());
    }
    let sn = read(&repo.join(format!("{sn_v}.snapshot.json")))?;
    let ts = repo::sign(
        &repo::timestamp(ts_v + 1, now + days * DAY, &sn, sn_v).map_err(|e| e.to_string())?,
        &[key],
    )
    .map_err(|e| e.to_string())?;
    commit(repo, &[], &ts, || verify(repo, None, now, MIN_DAYS))
}

/// Writes `files` and then `timestamp.json` (the file that makes them current), runs `check`,
/// and on failure puts the repository back as it was: the new files removed, the old timestamp
/// restored. A refused release never leaves a version behind.
fn commit(
    repo: &Path,
    files: &[(PathBuf, Vec<u8>)],
    ts: &[u8],
    check: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    let ts_path = repo.join("timestamp.json");
    let old_ts = fs::read(&ts_path).ok();
    if let Some((p, _)) = files.iter().find(|(p, _)| p.exists()) {
        return Err(format!(
            "{} exists: versions are never rewritten",
            p.display()
        ));
    }
    let undo = || {
        for (p, _) in files {
            let _ = fs::remove_file(p);
        }
        let _ = match &old_ts {
            Some(b) => fs::write(&ts_path, b),
            None => fs::remove_file(&ts_path),
        };
    };
    let result = files
        .iter()
        .try_for_each(|(p, b)| write(p, b))
        .and_then(|()| write(&ts_path, ts))
        .and_then(|()| check());
    if result.is_err() {
        undo();
    }
    result
}

/// Walks the repository as a new client would, from root 1, and checks every catalog entry,
/// the assets when given, and that nothing expires within `min_days`.
pub(crate) fn verify(
    repo: &Path,
    assets: Option<&Path>,
    now: i64,
    min_days: i64,
) -> Result<String, String> {
    let e = |what: &str| {
        let what = what.to_string();
        move |err: teigeodata_sync::trust::Error| format!("{what}: {err}")
    };
    let roots = latest(repo, "root")?;
    let mut t = Trusted::new(&read(&repo.join("1.root.json"))?, now).map_err(e("1.root.json"))?;
    for v in 2..=roots {
        t.update_root(&read(&repo.join(format!("{v}.root.json")))?)
            .map_err(e(&format!("{v}.root.json")))?;
    }
    t.update_timestamp(&read(&repo.join("timestamp.json"))?)
        .map_err(e("timestamp.json"))?;
    let sn = t.snapshot_name().map_err(e("snapshot name"))?;
    t.update_snapshot(&read(&repo.join(&sn))?, false)
        .map_err(e(&sn))?;
    let tg = t.targets_name().map_err(e("targets name"))?;
    t.update_targets(&read(&repo.join(&tg))?).map_err(e(&tg))?;
    let targets = t.targets().ok_or("no targets")?;
    let mut bytes = 0;
    for (path, target) in &targets.targets {
        let entry = Entry::from_target(path, target).map_err(|err| format!("{path}: {err}"))?;
        if let Some(dir) = assets {
            t.verify_target(path, &read(&dir.join(&entry.file))?)
                .map_err(e(path))?;
        }
        bytes += target.length;
    }
    let mut soonest = i64::MAX;
    for role in [
        RoleName::Root,
        RoleName::Timestamp,
        RoleName::Snapshot,
        RoleName::Targets,
    ] {
        let left = t.expires(role).ok_or("a role not loaded")? - now;
        if left < min_days * DAY {
            return Err(format!(
                "{} expires in {} days, under {min_days}",
                role.as_str(),
                left.div_euclid(DAY)
            ));
        }
        soonest = soonest.min(left);
    }
    Ok(format!(
        "verified: root {roots}, {sn}, {tg}, {} targets ({bytes} B){}; the soonest expiry is in {} days",
        targets.targets.len(),
        if assets.is_some() {
            ", every asset"
        } else {
            ""
        },
        soonest.div_euclid(DAY)
    ))
}

#[cfg(test)]
mod install_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
