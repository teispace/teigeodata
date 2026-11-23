//! Keys at rest (D-383): the ceremony, encrypted keys and a release signed with them, through
//! the real `age` tool. A plain identity stands in for the passphrase vault, which only a
//! person at a terminal can open; the passphrase path is checked by hand (catalog-keys.md).

use std::path::Path;
use std::process::Command;

/// Whether `age` is installed. CI installs it, so a missing `age` fails there instead of
/// passing by skipping.
fn age() -> bool {
    let ok = Command::new("age").arg("--version").output().is_ok()
        && Command::new("age-keygen").arg("--version").output().is_ok();
    assert!(
        ok || std::env::var_os("CI").is_none(),
        "age is not installed"
    );
    if !ok {
        eprintln!("skipped: age is not installed");
    }
    ok
}

fn run(args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    super::dispatch(&args)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("teigeodata-vault-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn identity(d: &Path) -> String {
    let id = d.join("id.txt");
    let ok = Command::new("age-keygen")
        .args(["-o", &id.to_string_lossy()])
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok);
    id.to_string_lossy().into_owned()
}

#[test]
fn a_ceremony_writes_only_encrypted_keys_and_a_signed_root_and_they_sign_a_release() {
    if !age() {
        return;
    }
    let d = scratch("ceremony");
    let id = identity(&d);
    let dir = d.join("keys-home");
    let p = |s: &str| dir.join(s).to_string_lossy().into_owned();
    let out = run(&["ceremony", &dir.to_string_lossy(), "--identity", &id]).unwrap();
    assert!(out.contains("the vault opens"), "{out}");

    // Every key is an age file; none holds its seed in the clear.
    for name in super::CEREMONY_KEYS {
        let text = std::fs::read_to_string(p(&format!("keys/{name}.key.age"))).unwrap();
        assert!(
            text.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"),
            "{name}"
        );
        let k = super::load_key(&p(&format!("keys/{name}.key.age"))).unwrap();
        assert!(
            !text.contains(&super::hex(k.as_bytes())),
            "{name} in the clear"
        );
        // The public-keys page lists it.
        let page = std::fs::read_to_string(p("public-keys.md")).unwrap();
        assert!(
            page.contains(&super::hex(k.verifying_key().as_bytes())),
            "{name}"
        );
    }
    // Root 1 carries two signatures, by root-1 and root-2.
    let root: serde_json::Value =
        serde_json::from_slice(&std::fs::read(p("metadata/1.root.json")).unwrap()).unwrap();
    assert_eq!(root["signatures"].as_array().unwrap().len(), 2);

    // A second ceremony never writes over the first.
    let again = run(&["ceremony", &dir.to_string_lossy(), "--identity", &id]).unwrap_err();
    assert!(again.contains("not empty"), "{again}");

    // A new key is encrypted to the vault beside it, and pubkey opens it.
    let extra = p("keys/extra.key.age");
    let made = run(&["keygen", &extra]).unwrap();
    let shown = run(&["pubkey", &extra]).unwrap();
    assert!(
        made.contains(shown.lines().next().unwrap()),
        "{made}\n{shown}"
    );

    // The encrypted keys sign a release that verifies from root 1.
    let pack = super::tests::pack(&d, "2026.11.0", 3);
    let repo = p("metadata");
    run(&[
        "release",
        &repo,
        "--assets",
        &d.join("assets").to_string_lossy(),
        "--pack",
        &pack.to_string_lossy(),
        "--targets-key",
        &p("keys/targets.key.age"),
        "--snapshot-key",
        &p("keys/snapshot.key.age"),
        "--timestamp-key",
        &p("keys/timestamp.key.age"),
    ])
    .unwrap();
    run(&["verify", &repo]).unwrap();
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn an_encrypted_key_needs_its_vault_and_its_own_bytes() {
    if !age() {
        return;
    }
    let d = scratch("refused");
    let id = identity(&d);
    let dir = d.join("k");
    std::fs::create_dir_all(&dir).unwrap();
    let recipient = String::from_utf8(
        Command::new("age-keygen")
            .args(["-y", &id])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let key = dir.join("a.key.age").to_string_lossy().into_owned();
    run(&["keygen", &key, "--to", recipient.trim()]).unwrap();

    // No vault beside it: refused, naming what is missing.
    let err = run(&["pubkey", &key]).unwrap_err();
    assert!(err.contains("no vault.age"), "{err}");

    // Another identity's vault does not open it.
    let other = identity(&d.join("o").tap_mkdir());
    std::fs::copy(&other, dir.join("vault.age")).unwrap();
    let err = run(&["pubkey", &key]).unwrap_err();
    assert!(err.contains("age"), "{err}");

    // Its own vault opens it; a changed byte is refused.
    std::fs::remove_file(dir.join("vault.age")).unwrap();
    std::fs::copy(&id, dir.join("vault.age")).unwrap();
    run(&["pubkey", &key]).unwrap();
    let mut text = std::fs::read_to_string(&key).unwrap();
    let at = text.len() / 2;
    let swapped = if &text[at..=at] == "A" { "B" } else { "A" };
    text.replace_range(at..=at, swapped);
    std::fs::write(&key, text).unwrap();
    assert!(run(&["pubkey", &key]).is_err());

    // keygen with no recipient to hand is refused, and never overwrites.
    let lone = d.join("lone").tap_mkdir().join("b.key.age");
    let err = run(&["keygen", &lone.to_string_lossy()]).unwrap_err();
    assert!(err.contains("no vault.pub"), "{err}");
    assert!(run(&["keygen", &key, "--to", recipient.trim()]).is_err());
    std::fs::remove_dir_all(&d).unwrap();
}

trait Mkdir {
    fn tap_mkdir(self) -> Self;
}

impl Mkdir for std::path::PathBuf {
    fn tap_mkdir(self) -> Self {
        std::fs::create_dir_all(&self).unwrap();
        self
    }
}
