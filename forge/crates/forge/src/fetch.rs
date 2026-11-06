//! The `fetch` stage (ADR-0035): downloads each pinned source into the content store
//! `data/cache/store/sha256/<hex>`, streaming through SHA-256, and keeps only bytes that match
//! the pin. A `sparql` source posts its query template with the parameters from the lock; its
//! snapshot is pinned like a file. `--pin` records new bytes in the lock instead (first fetch,
//! or a deliberate update of a moving source). A `tileset` source is pinned by its manifest,
//! which pins every tile ([`crate::tiles`]).

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::lock::{self, Lock, Source};

pub(crate) const USER_AGENT: &str =
    "teigeodata-forge/0.0 (+https://github.com/teispace/teigeodata)";
const ATTEMPTS: u32 = 3;

/// The content-store path of a source's pinned bytes.
pub(crate) fn store_path(root: &Path, s: &Source) -> PathBuf {
    root.join("data/cache/store/sha256").join(&s.sha256)
}

/// Fetches the given sources (all when `ids` is empty). With `pin`, writes the new SHA-256,
/// size and date into the lock.
pub(crate) fn run(root: &Path, ids: &[String], pin: bool, verify: bool) -> Result<(), String> {
    let lock_path = root.join("data/sources.lock");
    let lock = Lock::read(&lock_path)?;
    let chosen: Vec<&Source> = if ids.is_empty() {
        lock.sources.iter().collect()
    } else {
        ids.iter()
            .map(|id| lock.get(id))
            .collect::<Result<_, _>>()?
    };
    let store = root.join("data/cache/store/sha256");
    fs::create_dir_all(&store).map_err(|e| format!("{}: {e}", store.display()))?;
    let mut lock_text = None;
    for s in chosen {
        if s.kind == "tileset" {
            if let Some((hash, size)) = crate::tiles::fetch(root, s, pin, verify)? {
                let text = match lock_text.take() {
                    Some(t) => t,
                    None => fs::read_to_string(&lock_path).map_err(|e| e.to_string())?,
                };
                let updated = lock::repin(&text, &s.id, &hash, size, &today())
                    .ok_or_else(|| format!("{}: source block not found in the lock", s.id))?;
                lock_text = Some(updated);
                say(&format!(
                    "{}: manifest pinned sha256 {hash}, {size} bytes",
                    s.id
                ));
            }
            continue;
        }
        let target = store_path(root, s);
        if !pin && target.is_file() {
            if verify {
                let (hash, size) = hash_file(&target)?;
                if hash != s.sha256 || size != s.size {
                    return Err(format!(
                        "{}: the stored file is corrupt (sha256 {hash}, {size} bytes); delete it and fetch again",
                        s.id
                    ));
                }
            }
            say(&format!("{}: ok (stored)", s.id));
            continue;
        }
        let partial = store.join(format!(".partial-{}", s.id));
        let (hash, size) = download(root, s, pin, &partial)?;
        if pin {
            let fin = store.join(&hash);
            fs::rename(&partial, &fin).map_err(|e| format!("{}: {e}", fin.display()))?;
            let text = match lock_text.take() {
                Some(t) => t,
                None => fs::read_to_string(&lock_path).map_err(|e| e.to_string())?,
            };
            let updated = lock::repin(&text, &s.id, &hash, size, &today())
                .ok_or_else(|| format!("{}: source block not found in the lock", s.id))?;
            lock_text = Some(updated);
            say(&format!("{}: pinned sha256 {hash}, {size} bytes", s.id));
        } else if hash == s.sha256 && size == s.size {
            fs::rename(&partial, &target).map_err(|e| format!("{}: {e}", target.display()))?;
            say(&format!("{}: ok ({size} bytes)", s.id));
        } else {
            let _ = fs::remove_file(&partial);
            let hint = if s.moving || s.kind == "sparql" {
                "the upstream moves; fetch from the mirror, or re-pin deliberately with --pin"
            } else {
                "the upstream changed a pinned file; investigate before re-pinning"
            };
            return Err(format!(
                "{}: got sha256 {hash} ({size} bytes), pinned {} ({} bytes): {hint}",
                s.id, s.sha256, s.size
            ));
        }
    }
    if let Some(t) = lock_text {
        fs::write(&lock_path, t).map_err(|e| format!("{}: {e}", lock_path.display()))?;
    }
    Ok(())
}

pub(crate) fn say(msg: &str) {
    let _ = writeln!(io::stderr(), "{msg}");
}

/// Downloads into `partial`, returning the SHA-256 (lower-case hex) and the byte count.
fn download(root: &Path, s: &Source, pin: bool, partial: &Path) -> Result<(String, u64), String> {
    let mut last = String::new();
    for attempt in 1..=ATTEMPTS {
        match download_once(root, s, pin, partial) {
            Ok(r) => return Ok(r),
            Err(e) => {
                say(&format!("{}: attempt {attempt} failed: {e}", s.id));
                last = e;
                std::thread::sleep(Duration::from_secs(u64::from(attempt) * 5));
            }
        }
    }
    let _ = fs::remove_file(partial);
    Err(format!("{}: {last}", s.id))
}

fn download_once(
    root: &Path,
    s: &Source,
    pin: bool,
    partial: &Path,
) -> Result<(String, u64), String> {
    // A pinned fetch prefers our mirror; pinning reads the upstream.
    let url = match (&s.mirror_url, pin) {
        (Some(m), false) if !m.is_empty() => m.clone(),
        _ => s.url.clone(),
    };
    let mut resp = match s.kind.as_str() {
        "http" => ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| e.to_string())?,
        "sparql" => {
            let template = s.query.as_ref().ok_or("a sparql source needs a query")?;
            let q = render(root, template, s.query_params.iter())?;
            ureq::post(&url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "text/tab-separated-values")
                .send_form([("query", q.as_str())])
                .map_err(|e| e.to_string())?
        }
        k => return Err(format!("unknown kind `{k}`")),
    };
    stream_to(resp.body_mut().as_reader(), partial)
}

/// Reads a query template and fills its `{{KEY}}` parameters; every one must be set.
pub(crate) fn render<'a>(
    root: &Path,
    template: &str,
    params: impl Iterator<Item = (&'a String, &'a String)>,
) -> Result<String, String> {
    let mut q = fs::read_to_string(root.join(template)).map_err(|e| format!("{template}: {e}"))?;
    for (k, v) in params {
        q = q.replace(&format!("{{{{{k}}}}}"), v);
    }
    if q.contains("{{") {
        return Err(format!("{template}: a template parameter is not set"));
    }
    Ok(q)
}

/// Downloads one URL into `partial` (GET, or POST of a JSON `body`, with retries); `Ok(None)`
/// when the server has no such file (404), which a tileset records as an absent tile.
pub(crate) fn get_to(
    url: &str,
    body: Option<&str>,
    partial: &Path,
) -> Result<Option<(String, u64)>, String> {
    let mut last = String::new();
    for attempt in 1..=ATTEMPTS {
        let sent = match body {
            None => ureq::get(url).header("User-Agent", USER_AGENT).call(),
            Some(b) => ureq::post(url)
                .header("User-Agent", USER_AGENT)
                .header("Content-Type", "application/json")
                .send(b),
        };
        match sent {
            Ok(mut resp) => match stream_to(resp.body_mut().as_reader(), partial) {
                Ok(r) => return Ok(Some(r)),
                Err(e) => last = e,
            },
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(e) => last = e.to_string(),
        }
        say(&format!("{url}: attempt {attempt} failed: {last}"));
        std::thread::sleep(Duration::from_secs(u64::from(attempt) * 5));
    }
    let _ = fs::remove_file(partial);
    Err(format!("{url}: {last}"))
}

/// Streams a body into `partial` through SHA-256.
fn stream_to(mut reader: impl Read, partial: &Path) -> Result<(String, u64), String> {
    let file = File::create(partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        hasher.update(chunk);
        out.write_all(chunk).map_err(|e| e.to_string())?;
        size += n as u64;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok((hex(&hasher.finalize()), size))
}

pub(crate) fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(buf.get(..n).unwrap_or_default());
        size += n as u64;
    }
    Ok((hex(&hasher.finalize()), size))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Today's UTC date, `YYYY-MM-DD` (Howard Hinnant's civil-from-days).
pub(crate) fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, hex};

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_398), (2025, 11, 6));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn hex_is_lower_case() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }
}
