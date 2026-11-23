//! `cargo xtask tuf …`: the catalog repository (D-163,
//! [catalog repository tooling](../../docs/02-research/notes/catalog-repository-tooling.md)).
//!
//! The repository is a directory of metadata as GitHub Pages serves it: `N.root.json` for every
//! root, `N.targets.json`, `N.snapshot.json`, `timestamp.json`, and `N.cc.json` for each
//! country's delegated role (D-175): the top-level targets keep the packs of no country and
//! delegate each country's (`cc.*`, `cc-*`) to a role signed with the targets key, so a
//! client fetches only the countries it needs. A release re-signs only the roles whose packs
//! changed, or that would expire within half their lifetime. Transport assets go to a
//! separate directory, named as the catalog says (`address.component.release.hash8.tgd.zst`),
//! for the release upload. Keys are 32-byte Ed25519 seeds in hex, in a file encrypted to the
//! maintainer's vault (`*.key.age`, D-383, [`vault`]) or in an environment variable
//! (`env:NAME`), never in the repository. Every command that writes ends by
//! verifying the whole repository with the client, from root 1.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use serde_json::Value;
use teigeodata_format::Pack;
use teigeodata_sync::catalog::{Delta, Entry};
use teigeodata_sync::repo::{self, Files, RoleKeys, TargetFile};
use teigeodata_sync::trust::{Envelope, Hashes, RoleName, Trusted};

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
  ceremony <dir> [--identity FILE [--to RECIPIENT]] [--days 365]
                                         every key, encrypted to a new passphrase vault (or to
                                         an identity you hold, such as a hardware key), and
                                         root 1 signed; nothing plain touches the disk
  keygen <out.key.age> [--to RECIPIENT]  a new Ed25519 key, encrypted to the vault beside it
                                         (a name without .age writes it plain, for tests)
  secret <key.age> --env ENV --name NAME [--repo OWNER/REPO]
                                         puts a key into a GitHub environment secret via gh,
                                         never printed
  pubkey <key>                           the public key and key ID of a key
  root <repo> --root PUB… [--root-threshold 2] --timestamp PUB --snapshot PUB --targets PUB
       [--days 365]                      writes the next root, unsigned
  sign <file> --role root|timestamp|snapshot|targets --key KEY
                                         adds a signature in place
  release <repo> --assets DIR --pack P.tgd… --targets-key KEY --snapshot-key KEY
       --timestamp-key KEY [--folder TAG] [--delta D.tgd…]
                                         assets and catalog entries from packs; new targets,
                                         snapshot and timestamp; --folder: the assets sit in
                                         the GitHub release TAG under the mirror base;
                                         --delta: a delta pack (`tgd delta`) of a pack
                                         released here, listed in its entry (D-395)
  timestamp <repo> --key KEY [--days 14] a fresh timestamp for the same snapshot
  verify <repo> [--assets DIR] [--min-days 7]
                                         the whole chain with the client, every entry, the assets
  world-catalog [--targets 5000] [--runs 5]
                                         sync gate 12: a generated world catalog delegated by
                                         country; each file's gzip bytes against 512 KiB, and
                                         the time to verify and read it
KEY is a key file (*.key.age opens with its vault) or env:NAME; PUB is a public key in hex.";

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
        Some("keygen") => keygen(Path::new(a.pos(0)?), a.opt("--to")),
        Some("ceremony") => ceremony(
            Path::new(a.pos(0)?),
            a.opt("--identity").map(Path::new),
            a.opt("--to"),
            now + a.num("--days", ROOT_DAYS)? * DAY,
        ),
        Some("secret") => secret(
            Path::new(a.pos(0)?),
            a.req("--env")?,
            a.req("--name")?,
            a.opt("--repo"),
        ),
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
        Some("release") => release_in(
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
            a.opt("--folder"),
            &a.all("--delta")
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>(),
        ),
        Some("world-catalog") => world_catalog(&a),
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

/// Sync gate 12: a generated world catalog against its budget.
fn world_catalog(a: &Args<'_>) -> Result<String, String> {
    let n = usize::try_from(a.num("--targets", i64::try_from(world::WORLD).unwrap_or(5_000))?)
        .map_err(|e| e.to_string())?;
    let runs = usize::try_from(a.num("--runs", 5)?).map_err(|e| e.to_string())?;
    let m = world::measure(n, runs)?;
    let line = |what: &str, b: world::Bytes| {
        format!(
            "  {what}: {} B of JSON, {} B gzip{}",
            b.json,
            b.gzip,
            if b.gzip <= world::BUDGET { "" } else { " OVER" }
        )
    };
    let (name, count, largest) = &m.largest;
    let out = [
        format!(
            "{} targets in {} country roles; each file within {} B gzip: {}",
            m.targets,
            m.roles,
            world::BUDGET,
            if m.within() { "yes" } else { "NO" }
        ),
        line("top-level targets", m.top),
        line("snapshot", m.snapshot),
        line(&format!("largest role, {name} ({count} targets)"), *largest),
        format!(
            "  verified and read: every role {:.1} ms, {name} alone {:.1} ms",
            m.read_all_ms, m.read_one_ms
        ),
    ]
    .join("\n");
    if m.within() { Ok(out) } else { Err(out) }
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

/// A key from `env:NAME`, an encrypted file (`*.age`, opened with its vault) or a plain file.
fn load_key(source: &str) -> Result<SigningKey, String> {
    let text = match source.strip_prefix("env:") {
        Some(var) => std::env::var(var).map_err(|_| format!("{var} is not set"))?,
        None if source.ends_with(vault::SUFFIX) => vault::decrypt(Path::new(source))?,
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

fn new_seed() -> Result<[u8; 32], String> {
    let mut seed = [0_u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no randomness: {e}"))?;
    Ok(seed)
}

/// A new key at `out`: encrypted to `to` (or the vault's recipient beside it) when `out` ends
/// in `.age`, plain otherwise.
fn keygen(out: &Path, to: Option<&str>) -> Result<String, String> {
    let seed = new_seed()?;
    let text = format!("{}\n", hex(&seed));
    let bytes = if out.to_string_lossy().ends_with(vault::SUFFIX) {
        let recipient = match to {
            Some(r) => r.to_string(),
            None => vault::recipient_near(out)?,
        };
        vault::encrypt(&recipient, text.as_bytes())?
    } else {
        text.into_bytes()
    };
    vault::write_new(out, &bytes)?;
    Ok(format!(
        "wrote {}\n{}",
        out.display(),
        describe(&SigningKey::from_bytes(&seed))
    ))
}

/// The roles' keys a ceremony makes, in the order root 1 lists them.
const CEREMONY_KEYS: [&str; 6] = [
    "root-1",
    "root-2",
    "root-3",
    "timestamp",
    "snapshot",
    "targets",
];

/// The first ceremony (D-383): a vault, every key encrypted to it, root 1 signed by two root
/// keys, and the public keys. Signing happens in memory; the plain keys are never written.
/// Ends by opening one key through the vault, so a vault that cannot be opened (a mistyped
/// passphrase, a missing hardware key) fails here, not at the first release.
fn ceremony(
    dir: &Path,
    identity: Option<&Path>,
    to: Option<&str>,
    expires: i64,
) -> Result<String, String> {
    if dir.exists()
        && fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .next()
            .is_some()
    {
        return Err(format!(
            "{} is not empty: a ceremony starts in a new directory",
            dir.display()
        ));
    }
    let keys_dir = dir.join("keys");
    fs::create_dir_all(&keys_dir).map_err(|e| format!("{}: {e}", keys_dir.display()))?;
    let recipient = match identity {
        Some(id) => vault::adopt(dir, id, to)?,
        None => vault::create(dir)?,
    };
    let mut keys = Vec::new();
    for name in CEREMONY_KEYS {
        let seed = new_seed()?;
        let sealed = vault::encrypt(&recipient, format!("{}\n", hex(&seed)).as_bytes())?;
        vault::write_new(
            &keys_dir.join(format!("{name}.key{}", vault::SUFFIX)),
            &sealed,
        )?;
        keys.push(SigningKey::from_bytes(&seed));
    }
    let [r1, r2, r3, ts, sn, tg] = keys.as_slice() else {
        return Err("six keys".into());
    };
    let pk = |k: &SigningKey| k.verifying_key().to_bytes();
    let repo = dir.join("metadata");
    write_root(
        &repo,
        &[pk(r1), pk(r2), pk(r3)],
        2,
        [pk(ts), pk(sn), pk(tg)],
        expires,
    )?;
    let root = repo.join("1.root.json");
    sign_file(&root, RoleName::Root, r1)?;
    sign_file(&root, RoleName::Root, r2)?;
    // The vault opens: root-3 is read back through it.
    let back = load_key(
        &keys_dir
            .join(format!("root-3.key{}", vault::SUFFIX))
            .to_string_lossy(),
    )?;
    if back.verifying_key() != r3.verifying_key() {
        return Err("root-3 read back through the vault differs".into());
    }
    let mut table = String::from("| Key | Public key | Key ID |\n|---|---|---|\n");
    for (name, k) in CEREMONY_KEYS.iter().zip(&keys) {
        let p = pk(k);
        let _ = writeln!(table, "| {name} | `{}` | `{}` |", hex(&p), repo::key_id(&p));
    }
    vault::write_new(&dir.join("public-keys.md"), table.as_bytes())?;
    Ok(format!(
        "ceremony done in {}: {} signed by root-1 and root-2; the vault opens.\n\
         Copy the whole directory to at least two places (it holds no plain key).\n{table}",
        dir.display(),
        root.display()
    ))
}

/// Puts the key in `file` into the GitHub environment secret `name` of `env`, through
/// `gh secret set` on its standard input: the key is never printed or passed as an argument.
fn secret(file: &Path, env: &str, name: &str, repo_name: Option<&str>) -> Result<String, String> {
    let key = load_key(&file.to_string_lossy())?;
    let mut args = vec!["secret", "set", name, "--env", env];
    if let Some(r) = repo_name {
        args.extend(["--repo", r]);
    }
    let mut child = std::process::Command::new("gh")
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("gh: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        std::io::Write::write_all(&mut stdin, hex(key.as_bytes()).as_bytes())
            .map_err(|e| format!("gh: {e}"))?;
    }
    let status = child.wait().map_err(|e| format!("gh: {e}"))?;
    if !status.success() {
        return Err(format!("gh secret set {name} --env {env} failed"));
    }
    Ok(format!(
        "set {name} in the environment {env}: {}",
        repo::key_id(key.verifying_key().as_bytes())
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

/// A delegated role as published: its version, its files and when it expires.
struct Published {
    version: u64,
    files: Files,
    expires: i64,
}

/// The repository's current state: every target (top-level and delegated), the versions of the
/// top-level targets, snapshot and timestamp, and each delegated role as published.
struct Current {
    files: Files,
    versions: [u64; 3],
    roles: BTreeMap<String, Published>,
}

/// The target files a `targets` role lists.
fn files_of(signed: &Value) -> Result<Files, String> {
    let mut files = BTreeMap::new();
    for (path, t) in signed
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
    Ok(files)
}

fn current(repo: &Path) -> Result<Current, String> {
    let ts_path = repo.join("timestamp.json");
    if !ts_path.exists() {
        return Ok(Current {
            files: BTreeMap::new(),
            versions: [0; 3],
            roles: BTreeMap::new(),
        });
    }
    let ts = signed_of(&read(&ts_path)?)?;
    let sn_version = ts
        .pointer("/meta/snapshot.json/version")
        .and_then(Value::as_u64)
        .ok_or("the timestamp names no snapshot")?;
    let sn = signed_of(&read(&repo.join(format!("{sn_version}.snapshot.json")))?)?;
    let mut tg_version = None;
    let mut roles = BTreeMap::new();
    let mut files = BTreeMap::new();
    for (file, m) in sn
        .get("meta")
        .and_then(Value::as_object)
        .ok_or("a snapshot without meta")?
    {
        let version = m
            .get("version")
            .and_then(Value::as_u64)
            .ok_or("a snapshot entry without a version")?;
        let name = file
            .strip_suffix(".json")
            .ok_or("a snapshot entry not named .json")?;
        if name == "targets" {
            tg_version = Some(version);
            continue;
        }
        let bytes = read(&repo.join(format!("{version}.{file}")))?;
        let role_files = files_of(&signed_of(&bytes)?)?;
        files.extend(role_files.clone());
        roles.insert(
            name.to_string(),
            Published {
                version,
                files: role_files,
                expires: Envelope::parse(&bytes, RoleName::Targets)
                    .and_then(|e| e.targets())
                    .map_err(|e| format!("{version}.{file}: {e}"))?
                    .common
                    .expires,
            },
        );
    }
    let tg_version = tg_version.ok_or("the snapshot names no targets")?;
    let tg = signed_of(&read(&repo.join(format!("{tg_version}.targets.json")))?)?;
    files.extend(files_of(&tg)?);
    Ok(Current {
        files,
        versions: [tg_version, sn_version, version_of(&ts)?],
        roles,
    })
}

/// The transport asset and catalog entry of a pack.
pub(crate) fn entry_of(bytes: &[u8]) -> Result<(Entry, Vec<u8>, Hashes), String> {
    Pack::open_verified(bytes).map_err(|e| e.to_string())?;
    let asset = crate::wire::asset(bytes)?;
    let entry = teigeodata_sync::measure::entry(bytes, &asset).map_err(|e| e.to_string())?;
    let hashes = repo::target_hashes(&asset);
    Ok((entry, asset, hashes))
}

/// Each country's role for a release: kept when its packs are the same and it has half its
/// life left, else re-signed at the next version and added to `written`. As (name, bytes,
/// version), by name.
fn country_roles(
    repo: &Path,
    countries: &BTreeMap<[u8; 2], Files>,
    published: &BTreeMap<String, Published>,
    keys: &Keys,
    now: i64,
    written: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<Vec<(String, Vec<u8>, u64)>, String> {
    let e = |err: teigeodata_sync::trust::Error| err.to_string();
    let expires = now + OFFLINE_DAYS * DAY;
    let mut roles: Vec<(String, Vec<u8>, u64)> = Vec::with_capacity(countries.len());
    for (cc, role_files) in countries {
        let name: String = cc.iter().map(|&b| char::from(b)).collect();
        let old = published.get(&name);
        let keep = old
            .is_some_and(|p| p.files == *role_files && p.expires - now >= OFFLINE_DAYS / 2 * DAY);
        let (version, bytes) = match old {
            Some(p) if keep => (
                p.version,
                read(&repo.join(format!("{}.{name}.json", p.version)))?,
            ),
            _ => {
                let v = old.map_or(1, |p| p.version + 1);
                let b = repo::sign(
                    &repo::targets(v, expires, role_files).map_err(e)?,
                    &[&keys.targets],
                )
                .map_err(e)?;
                written.push((repo.join(format!("{v}.{name}.json")), b.clone()));
                (v, b)
            }
        };
        roles.push((name, bytes, version));
    }
    if let Some(gone) = published
        .keys()
        .find(|n| !roles.iter().any(|(r, _, _)| r == *n))
    {
        return Err(format!(
            "the role {gone} would be dropped: packs are never unpublished"
        ));
    }
    Ok(roles)
}

#[cfg(test)]
pub(crate) fn release(
    repo: &Path,
    assets: &Path,
    packs: &[PathBuf],
    keys: &Keys,
    now: i64,
) -> Result<String, String> {
    release_in(repo, assets, packs, keys, now, None, &[])
}

/// Writes `asset` as `name` in `assets`, never over an asset with other bytes.
fn put_asset(assets: &Path, name: &str, asset: &[u8]) -> Result<(), String> {
    let dest = assets.join(name);
    match fs::read(&dest) {
        Ok(old) if old != asset => Err(format!("{} exists with other bytes", dest.display())),
        Ok(_) => Ok(()),
        Err(_) => write(&dest, asset),
    }
}

/// The delta packs `deltas` (made by `tgd delta`, D-391) as catalog deltas by pack path, their
/// assets written to `assets` (D-395). Each must be of a pack in `packs` and of its release.
fn deltas_of(
    deltas: &[PathBuf],
    packs: &[(String, String, [u8; 16])],
    assets: &Path,
    folder: Option<&str>,
) -> Result<BTreeMap<String, Vec<Delta>>, String> {
    let mut out: BTreeMap<String, Vec<Delta>> = BTreeMap::new();
    for p in deltas {
        let at = |e: String| format!("{}: {e}", p.display());
        let bytes = read(p)?;
        let from = Pack::open(&bytes)
            .map_err(|e| at(e.to_string()))?
            .manifest()
            .delta_of()
            .map(str::to_owned)
            .ok_or_else(|| at("not a delta pack (`tgd delta` makes one)".into()))?;
        let (mut entry, asset, hashes) = entry_of(&bytes).map_err(at)?;
        let Some(&(_, _, answers)) = packs
            .iter()
            .find(|(p, r, _)| *p == entry.path() && *r == entry.release)
        else {
            return Err(at(format!(
                "a delta to {} {}, which this release does not publish",
                entry.path(),
                entry.release
            )));
        };
        let sha256: [u8; 32] = hashes
            .get("sha256")
            .and_then(|h| h.as_slice().try_into().ok())
            .ok_or_else(|| at("no sha256".into()))?;
        put_asset(assets, entry.name(), &asset)?;
        if let Some(f) = folder {
            entry.file = format!("{f}/{}", entry.file);
        }
        let path = entry.path();
        out.entry(path).or_default().push(Delta::new(
            from,
            asset.len() as u64,
            sha256,
            entry,
            answers,
        ));
    }
    Ok(out)
}

/// A release whose assets sit in `folder` under the mirror base (D-384): the GitHub release
/// of that tag. The assets are still written flat into `assets`, for its upload. Each of
/// `deltas` is listed in its pack's entry (D-395).
pub(crate) fn release_in(
    repo: &Path,
    assets: &Path,
    packs: &[PathBuf],
    keys: &Keys,
    now: i64,
    folder: Option<&str>,
    deltas: &[PathBuf],
) -> Result<String, String> {
    if packs.is_empty() {
        return Err("no --pack".into());
    }
    fs::create_dir_all(assets).map_err(|e| format!("{}: {e}", assets.display()))?;
    let mut entries = Vec::with_capacity(packs.len());
    for p in packs {
        entries.push(entry_of(&read(p)?).map_err(|e| format!("{}: {e}", p.display()))?);
    }
    let published: Vec<(String, String, [u8; 16])> = entries
        .iter()
        .map(|(e, _, _)| (e.path(), e.release.clone(), e.head))
        .collect();
    let mut deltas = deltas_of(deltas, &published, assets, folder)?;
    let Current {
        mut files,
        versions: [targets_v, snapshot_v, timestamp_v],
        roles: published,
    } = current(repo)?;
    let mut out = String::new();
    for (p, (mut entry, asset, hashes)) in packs.iter().zip(entries) {
        if let Some(f) = folder {
            entry.file = format!("{f}/{}", entry.file);
        }
        let mut custom = entry.to_custom();
        let listed = deltas.remove(&entry.path()).unwrap_or_default();
        if let (false, Value::Object(m)) = (listed.is_empty(), &mut custom) {
            let list: Vec<Value> = listed.iter().map(Delta::to_json).collect();
            m.insert("deltas".to_owned(), Value::Array(list));
        }
        let file = TargetFile {
            length: asset.len() as u64,
            hashes,
            custom: Some(custom),
        };
        // The client must read what we write.
        let target = teigeodata_sync::trust::Target {
            length: file.length,
            hashes: file.hashes.clone(),
            custom: file.custom.clone(),
        };
        Entry::from_target(&entry.path(), &target).map_err(|e| format!("{}: {e}", p.display()))?;
        let read_back = Delta::list(&entry.path(), &target, &entry)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        if read_back != listed {
            return Err(format!("{}: its deltas do not read back", p.display()));
        }
        put_asset(assets, entry.name(), &asset)?;
        let _ = writeln!(out, "{} → {} ({} B)", entry.path(), entry.file, asset.len());
        for d in &listed {
            let _ = writeln!(
                out,
                "  delta from {} → {} ({} B)",
                d.from, d.entry.file, d.length
            );
        }
        files.insert(entry.path(), file);
    }
    let expires = now + OFFLINE_DAYS * DAY;
    let e = |err: teigeodata_sync::trust::Error| err.to_string();
    let (top, countries) = repo::by_country(files);
    let targets_key = RoleKeys {
        keys: vec![keys.targets.verifying_key().to_bytes()],
        threshold: 1,
    };
    let mut written = Vec::new();
    let roles = country_roles(repo, &countries, &published, keys, now, &mut written)?;
    let delegated: Vec<repo::DelegatedRole> = countries
        .keys()
        .map(|&cc| repo::country_role(cc, targets_key.clone()))
        .collect();
    let tg = repo::sign(
        &repo::targets_delegating(targets_v + 1, expires, &top, &delegated).map_err(e)?,
        &[&keys.targets],
    )
    .map_err(e)?;
    let mut listed: Vec<(&str, &[u8], u64)> = vec![("targets", &tg, targets_v + 1)];
    listed.extend(roles.iter().map(|(n, b, v)| (n.as_str(), b.as_slice(), *v)));
    let sn = repo::sign(
        &repo::snapshot_listing(snapshot_v + 1, expires, &listed).map_err(e)?,
        &[&keys.snapshot],
    )
    .map_err(e)?;
    let ts = repo::sign(
        &repo::timestamp(
            timestamp_v + 1,
            now + TIMESTAMP_DAYS * DAY,
            &sn,
            snapshot_v + 1,
        )
        .map_err(e)?,
        &[&keys.timestamp],
    )
    .map_err(e)?;
    written.push((repo.join(format!("{}.targets.json", targets_v + 1)), tg));
    written.push((repo.join(format!("{}.snapshot.json", snapshot_v + 1)), sn));
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
    let Current {
        versions: [_, sn_v, ts_v],
        ..
    } = current(repo)?;
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
    let names: Vec<String> = t
        .delegations()
        .map(|d| d.roles.iter().map(|r| r.name.clone()).collect())
        .unwrap_or_default();
    let mut listed = t.targets().map_or(0, |x| x.targets.len());
    for name in &names {
        let file = t.delegated_name(name).map_err(e(name))?;
        t.update_delegated(name, &read(&repo.join(&file))?)
            .map_err(e(&file))?;
        listed += t.delegated(name).map_or(0, |x| x.targets.len());
    }
    let mut bytes = 0;
    let mut count = 0;
    for (path, target) in t.trusted_targets() {
        let entry = Entry::from_target(path, target).map_err(|err| format!("{path}: {err}"))?;
        if let Some(dir) = assets {
            t.verify_target(path, &read(&dir.join(entry.name()))?)
                .map_err(e(path))?;
        }
        // Its deltas (D-395), and each one's asset.
        for d in Delta::list(path, target, &entry).map_err(|err| format!("{path}: {err}"))? {
            if let Some(dir) = assets {
                let asset = read(&dir.join(d.entry.name()))?;
                let sha = repo::target_hashes(&asset);
                if asset.len() as u64 != d.length
                    || sha.get("sha256").map(Vec::as_slice) != Some(d.sha256.as_slice())
                {
                    return Err(format!(
                        "{path}: the delta from {} is not its asset",
                        d.from
                    ));
                }
            }
            bytes += d.length;
        }
        bytes += target.length;
        count += 1;
    }
    // A role listing a pack outside its countries, or two roles listing one, is a repository
    // bug the client would quietly skip: refuse it here.
    if count != listed {
        return Err(format!(
            "{listed} targets listed but {count} trusted: a role lists a pack outside its country"
        ));
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
    for name in &names {
        let left = t.delegated(name).ok_or("a role not loaded")?.common.expires - now;
        if left < min_days * DAY {
            return Err(format!(
                "the role {name} expires in {} days, under {min_days}",
                left.div_euclid(DAY)
            ));
        }
        soonest = soonest.min(left);
    }
    Ok(format!(
        "verified: root {roots}, {sn}, {tg}, {} country roles, {count} targets ({bytes} B){}; the soonest expiry is in {} days",
        names.len(),
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
mod policy_tests;
#[cfg(test)]
mod refresh_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tests;
mod vault;
#[cfg(test)]
mod vault_tests;
mod world;
