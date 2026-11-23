//! Keys at rest (D-383): every private key is a file encrypted with [age](https://age-encryption.org)
//! to one vault, so the files may be copied anywhere (a cloud drive, USB sticks) and a plain
//! key never touches a disk.
//!
//! A key directory holds `vault.age` (the age identity that opens the keys, itself encrypted
//! with a passphrase, or a hardware key's identity stub), `vault.pub` (its recipient) and
//! `keys/<role>.key.age`. A key file ending in `.age` is opened with the vault found beside it
//! or one directory up, or the one `TEIGEODATA_VAULT` names. A passphrase vault is unlocked
//! once per command, so a release asks for the passphrase once. The `age` tool does the
//! cryptography and the passphrase prompt (on the terminal, never in an argument or a variable).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// The suffix of an encrypted key file.
pub(crate) const SUFFIX: &str = ".age";
/// The vault's file names in a key directory.
pub(crate) const VAULT: &str = "vault.age";
pub(crate) const RECIPIENT: &str = "vault.pub";
/// Names a vault outside the key's directory.
const ENV: &str = "TEIGEODATA_VAULT";

/// Passphrase vaults already opened by this command: (vault, identity text).
static UNLOCKED: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// Runs `bin` with `args` and `input` on its standard input; its standard output. Prompts go
/// to the terminal, which age opens itself.
fn run(bin: &str, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!("{bin}: {e} (install age: `brew install age` or `apt install age`)")
        })?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(bytes).map_err(|e| format!("{bin}: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{bin}: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "{bin} {}: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Whether `bytes` are an age file (binary or armored), not a plain identity.
fn encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(b"age-encryption.org/")
        || bytes.starts_with(b"-----BEGIN AGE ENCRYPTED FILE-----")
}

/// Writes a new file readable by its owner only; an existing file is never replaced.
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// `plain` encrypted (armored) to the age `recipient`.
pub(crate) fn encrypt(recipient: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    run(
        "age",
        &["--encrypt", "--armor", "-r", recipient],
        Some(plain),
    )
}

/// The file `name` beside `file` or one directory up.
fn near(file: &Path, name: &str) -> Option<PathBuf> {
    let dir = file.parent()?;
    [Some(dir), dir.parent()]
        .into_iter()
        .flatten()
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The recipient new keys beside `file` are encrypted to: `vault.pub` beside it or one up.
pub(crate) fn recipient_near(file: &Path) -> Result<String, String> {
    let p = near(file, RECIPIENT).ok_or_else(|| {
        format!(
            "no {RECIPIENT} beside {} or one directory up: pass --to <age recipient>",
            file.display()
        )
    })?;
    Ok(fs::read_to_string(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?
        .trim()
        .to_string())
}

/// The key text in the encrypted `file`, opened with its vault.
pub(crate) fn decrypt(file: &Path) -> Result<String, String> {
    let vault = std::env::var_os(ENV)
        .map(PathBuf::from)
        .or_else(|| near(file, VAULT))
        .ok_or_else(|| {
            format!(
                "{}: no {VAULT} beside it or one directory up; set {ENV} to the vault",
                file.display()
            )
        })?;
    let bytes = fs::read(&vault).map_err(|e| format!("{}: {e}", vault.display()))?;
    let name = file.to_string_lossy();
    let out = if encrypted(&bytes) {
        let identity = unlock(&vault)?;
        run(
            "age",
            &["--decrypt", "-i", "-", &name],
            Some(identity.as_bytes()),
        )?
    } else {
        // A plain identity: a hardware key's stub (or a test's key).
        let v = vault.to_string_lossy();
        run("age", &["--decrypt", "-i", &v, &name], None)?
    };
    String::from_utf8(out).map_err(|_| format!("{name}: not a key"))
}

/// The identity in a passphrase vault, asking for the passphrase once per command.
fn unlock(vault: &Path) -> Result<String, String> {
    let mut open = UNLOCKED.lock().map_err(|_| "the vault lock is poisoned")?;
    if let Some((_, id)) = open.iter().find(|(p, _)| p == vault) {
        return Ok(id.clone());
    }
    eprintln!("opening {}", vault.display());
    let id = String::from_utf8(run("age", &["--decrypt", &vault.to_string_lossy()], None)?)
        .map_err(|_| format!("{}: not an identity", vault.display()))?;
    open.push((vault.to_path_buf(), id.clone()));
    Ok(id)
}

/// A new passphrase vault in `dir`: a fresh age identity, encrypted with a passphrase age asks
/// for (an empty answer makes age generate and print a strong one). Its recipient.
pub(crate) fn create(dir: &Path) -> Result<String, String> {
    let identity = run("age-keygen", &[], None)?;
    let recipient = recipient_of(&identity)?;
    eprintln!(
        "choose the vault's passphrase (press Enter twice for a generated one, then save it)"
    );
    let sealed = run(
        "age",
        &["--encrypt", "--passphrase", "--armor"],
        Some(&identity),
    )?;
    write_new(&dir.join(VAULT), &sealed)?;
    write_new(&dir.join(RECIPIENT), format!("{recipient}\n").as_bytes())?;
    Ok(recipient)
}

/// A vault in `dir` from an identity the maintainer already holds (a hardware key's stub, a
/// passphrase-encrypted identity, a test's key), copied in. Its recipient: `to`, or read from
/// the identity.
pub(crate) fn adopt(dir: &Path, identity: &Path, to: Option<&str>) -> Result<String, String> {
    let bytes = fs::read(identity).map_err(|e| format!("{}: {e}", identity.display()))?;
    let recipient = match to {
        Some(r) => r.to_string(),
        None if encrypted(&bytes) => {
            return Err(format!(
                "{}: an encrypted identity; pass --to <its recipient>",
                identity.display()
            ));
        }
        None => recipient_of(&bytes)?,
    };
    write_new(&dir.join(VAULT), &bytes)?;
    write_new(&dir.join(RECIPIENT), format!("{recipient}\n").as_bytes())?;
    Ok(recipient)
}

/// The recipient of an identity file's text: its `# public key:` or `# Recipient:` comment, or
/// `age-keygen -y` for a bare key.
fn recipient_of(identity: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(identity);
    let comment = text.lines().find_map(|l| {
        let l = l.trim_start_matches('#').trim();
        l.strip_prefix("public key:")
            .or_else(|| l.strip_prefix("Recipient:"))
            .map(|r| r.trim().to_string())
    });
    match comment {
        Some(r) => Ok(r),
        None => Ok(
            String::from_utf8_lossy(&run("age-keygen", &["-y"], Some(identity))?)
                .trim()
                .to_string(),
        ),
    }
}
