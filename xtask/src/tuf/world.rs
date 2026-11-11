//! Sync gate 12 (ADR-0037): the catalog of a whole world, generated, against its budget. The
//! targets metadata GitHub Pages serves (gzip, measured 2026-11-09) must stay within 512 KiB,
//! and the client must verify it and read every entry quickly. Entries are made the way
//! `tuf release` makes them, with every field filled: twelve components, a dependency on the
//! country pack for every part, a box, block hashes, and random hashes that do not compress.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::Instant;

use ed25519_dalek::SigningKey;
use teigeodata_format::Bbox;
use teigeodata_format::component::Component;
use teigeodata_sync::catalog::{BlockHashes, Catalog, Entry, Size, file_name};
use teigeodata_sync::repo::{RoleKeys, TargetFile, root, sign, snapshot, targets, timestamp};
use teigeodata_sync::trust::Trusted;

/// The gzip budget of the targets metadata.
pub(crate) const BUDGET: u64 = 512 * 1024;
/// About the world's targets: every country and the parts of the large ones (the
/// engine-and-catalog note, §3.10).
pub(crate) const WORLD: usize = 5_000;
const NOW: i64 = 1_794_268_800;
const YEAR: i64 = 365 * 86_400;

/// A world catalog, measured.
#[derive(Debug)]
pub(crate) struct Measured {
    pub(crate) targets: usize,
    pub(crate) json: u64,
    pub(crate) gzip: u64,
    /// Verifying the chain from root 1 and reading every entry, best of `runs`.
    pub(crate) read_ms: f64,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0_u8; N];
        for b in &mut out {
            *b = self.next().to_le_bytes()[0];
        }
        out
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn text<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// The `n` addresses: two-letter countries first, then numbered parts of them.
fn addresses(n: usize) -> Vec<String> {
    let letter = |i: usize| char::from(b'a' + u8::try_from(i % 26).unwrap_or(0));
    let countries: Vec<String> = (0..250)
        .map(|i| [letter(i / 26), letter(i)].iter().collect())
        .collect();
    let mut out: Vec<String> = countries.iter().take(n).cloned().collect();
    for (i, country) in countries.iter().cycle().enumerate() {
        if out.len() >= n {
            break;
        }
        out.push(format!("{country}-p{:03}", i / countries.len()));
    }
    out
}

/// The signed repository of a generated world catalog of `n` targets: root 1, targets,
/// snapshot, timestamp.
pub(crate) fn generate(n: usize) -> Result<[Vec<u8>; 4], String> {
    let e = text;
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut files = BTreeMap::new();
    for address in addresses(n) {
        let sha: [u8; 32] = rng.bytes();
        let part = address.contains('-');
        let len = 1_000_000 + rng.below(200_000_000);
        // The components share the pack after its head, block hashes and footer.
        let list = len / 512;
        let body = len - 1_400 - list - 32;
        let weights: Vec<u64> = Component::ALL.iter().map(|_| 1 + rng.below(100)).collect();
        let total: u64 = weights.iter().sum();
        let mut sizes = BTreeMap::new();
        for (c, w) in Component::ALL.into_iter().zip(&weights) {
            let disk = body / total * w;
            sizes.insert(
                c,
                Size {
                    wire: disk * 7 / 10,
                    disk,
                },
            );
        }
        let lon = i32::try_from(rng.below(3_500_000_000)).unwrap_or(0) - 1_750_000_000;
        let lat = i32::try_from(rng.below(1_700_000_000)).unwrap_or(0) - 850_000_000;
        let bbox = Bbox {
            min: teigeodata_core::Point::from_units(lat, lon).ok_or("a point off the globe")?,
            max: teigeodata_core::Point::from_units(lat + 30_000_000, lon + 50_000_000)
                .ok_or("a point off the globe")?,
        };
        let entry = Entry {
            address: address.clone(),
            component: "places".into(),
            release: "2027.1.0".into(),
            file: file_name(&address, "places", "2027.1.0", &sha),
            pack_length: len,
            head: rng.bytes(),
            seek_table: 1_000 + rng.below(20_000),
            head_frame: Size {
                wire: 900,
                disk: 1_400,
            },
            block_hashes: Some(BlockHashes {
                size: Size {
                    wire: list,
                    disk: list,
                },
                needed_by: teigeodata_format::component::Components::of(&[
                    Component::Names,
                    Component::Search,
                ]),
            }),
            sizes,
            deps: if part {
                vec![format!("{}.places.tgd", &address[..2])]
            } else {
                vec![]
            },
            min_reader: 1,
            bbox: Some(bbox),
            licence: "ODbL-1.0".into(),
        };
        files.insert(
            entry.path(),
            TargetFile {
                length: len * 7 / 10,
                hashes: BTreeMap::from([("sha256".to_string(), sha.to_vec())]),
                custom: Some(entry.to_custom()),
            },
        );
    }
    let public = |s: u8| key(s).verifying_key().to_bytes();
    let one = |s: u8| RoleKeys {
        keys: vec![public(s)],
        threshold: 1,
    };
    let roles = [
        RoleKeys {
            keys: vec![public(1), public(2), public(3)],
            threshold: 2,
        },
        one(10),
        one(20),
        one(30),
    ];
    let root1 = sign(
        &root(1, NOW + YEAR, &roles).map_err(e)?,
        &[&key(1), &key(2)],
    )
    .map_err(e)?;
    let tg = sign(&targets(1, NOW + YEAR, &files).map_err(e)?, &[&key(30)]).map_err(e)?;
    let sn = sign(&snapshot(1, NOW + YEAR, &tg, 1).map_err(e)?, &[&key(20)]).map_err(e)?;
    let ts = sign(&timestamp(1, NOW + 86_400, &sn, 1).map_err(e)?, &[&key(10)]).map_err(e)?;
    Ok([root1, tg, sn, ts])
}

/// Verifies the chain and reads every entry; the number of entries read.
pub(crate) fn read(repo: &[Vec<u8>; 4]) -> Result<usize, String> {
    let [root1, tg, sn, ts] = repo;
    let e = text;
    let mut t = Trusted::new(root1, NOW).map_err(e)?;
    t.update_timestamp(ts).map_err(e)?;
    t.update_snapshot(sn, false).map_err(e)?;
    t.update_targets(tg).map_err(e)?;
    let (catalog, refused) = Catalog::new(t.targets().ok_or("no targets")?);
    if let Some((path, err)) = refused.first() {
        return Err(format!("{path}: {err}"));
    }
    Ok(catalog.entries().len())
}

/// Generates a world of `n` targets and measures it: sizes, and the read time, best of `runs`.
pub(crate) fn measure(n: usize, runs: usize) -> Result<Measured, String> {
    let repo = generate(n)?;
    let json = repo[1].len() as u64;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&repo[1]).map_err(|e| e.to_string())?;
    let gzip = gz.finish().map_err(|e| e.to_string())?.len() as u64;
    let mut best = f64::INFINITY;
    for _ in 0..runs.max(1) {
        let start = Instant::now();
        let got = read(&repo)?;
        best = best.min(start.elapsed().as_secs_f64() * 1e3);
        if got != n {
            return Err(format!("read {got} entries of {n}"));
        }
    }
    Ok(Measured {
        targets: n,
        json,
        gzip,
        read_ms: best,
    })
}

#[cfg(test)]
mod tests {
    use super::{BUDGET, WORLD, generate, measure, read};

    /// The generator's entries are ones the client accepts, every one.
    #[test]
    fn a_generated_catalog_reads_back_whole() {
        assert_eq!(read(&generate(300).unwrap()).unwrap(), 300);
    }

    /// Gate 12: the world's targets within the gzip budget. It fails today: one targets file
    /// for the world is about 1.4 MB gzip (D-175); the catalog is to be delegated by country.
    #[test]
    #[ignore = "gate 12 fails with one targets file for the world; delegation by country (D-175)"]
    fn the_world_catalog_fits_its_budget() {
        let m = measure(WORLD, 1).unwrap();
        assert!(m.gzip <= BUDGET, "{m:?}");
    }
}
