//! The TUF differential (D-161): every case python-tuf built and judged
//! (`tools/tuf-oracle/generate.py`) is replayed with our client, which must reach python-tuf's
//! verdict at every step, or the verdict the case records for us with its reason.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};

use serde_json::Value;
use teigeodata_sync::trust::{Error, Trusted};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tuf")
}

fn read_json(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

/// Our result in python-tuf's terms.
fn verdict(r: &Result<(), Error>) -> &'static str {
    match r {
        Ok(()) => "ok",
        Err(Error::Expired(_)) => "expired",
        Err(Error::Unchanged) => "equal",
        Err(_) => "reject",
    }
}

fn replay(dir: &Path, now: i64) -> Vec<String> {
    let case = read_json(&dir.join("case.json"));
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let mut t = Trusted::new(&std::fs::read(dir.join("root.json")).unwrap(), now)
        .unwrap_or_else(|e| panic!("{name}: the trusted root: {e}"));
    let mut wrong = Vec::new();
    for step in case["steps"].as_array().unwrap() {
        let op = step["op"].as_str().unwrap();
        let file = step["file"].as_str().unwrap();
        let data = std::fs::read(dir.join(file)).unwrap();
        let r = match op {
            "root" => t.update_root(&data),
            "timestamp" => t.update_timestamp(&data),
            "snapshot" => t.update_snapshot(&data, false),
            "snapshot_stored" => t.update_snapshot(&data, true),
            "targets" => t.update_targets(&data),
            "target" => t.verify_target(step["path"].as_str().unwrap(), &data),
            _ => panic!("{name}: unknown step {op}"),
        };
        let want = step
            .get("ours")
            .unwrap_or(&step["python"])
            .as_str()
            .unwrap();
        let got = verdict(&r);
        if got != want {
            wrong.push(format!(
                "{name}/{file}: {op} gave {got} ({r:?}), want {want}"
            ));
        }
    }
    wrong
}

#[test]
fn our_client_agrees_with_python_tuf() {
    let meta = read_json(&corpus().join("corpus.json"));
    let now = meta["now"].as_i64().unwrap();
    let cases = meta["cases"].as_array().unwrap();
    assert!(cases.len() >= 25, "the corpus lost cases: {}", cases.len());
    let mut wrong = Vec::new();
    let mut steps = 0;
    for c in cases {
        let dir = corpus().join(c.as_str().unwrap());
        steps += read_json(&dir.join("case.json"))["steps"]
            .as_array()
            .unwrap()
            .len();
        wrong.extend(replay(&dir, now));
    }
    assert!(
        wrong.is_empty(),
        "{} of {steps} steps disagree:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Every difference from python-tuf is written down with its reason.
#[test]
fn every_difference_says_why() {
    let meta = read_json(&corpus().join("corpus.json"));
    for c in meta["cases"].as_array().unwrap() {
        let case = read_json(&corpus().join(c.as_str().unwrap()).join("case.json"));
        for s in case["steps"].as_array().unwrap() {
            if s.get("ours").is_some() {
                assert!(
                    s["ours_why"].as_str().is_some_and(|w| w.len() > 10),
                    "{c}: {s}"
                );
            }
        }
    }
}
