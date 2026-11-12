//! Sync gate 12 (ADR-0037, D-175): the catalog of a whole world, generated, against its budget.
//! The catalog is delegated by country, so the gate is per file GitHub Pages serves (gzip,
//! measured 2026-11-09): the top-level targets, the snapshot and the largest country's role
//! each within 512 KiB, and the client must verify and read them quickly. Entries are made the
//! way `tuf release` makes them, with every field filled: twelve components, a dependency on
//! the country pack for every part, a box, block hashes, and random hashes that do not
//! compress. Parts are shared out unevenly (Zipf: the first country has the most), as real
//! countries are, so the largest role is a large country's.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::Instant;

use ed25519_dalek::SigningKey;
use teigeodata_format::Bbox;
use teigeodata_format::component::Component;
use teigeodata_sync::catalog::{BlockHashes, Catalog, Entry, Size, file_name};
use teigeodata_sync::repo::{
    Files, RoleKeys, TargetFile, by_country, country_role, root, sign, snapshot_listing,
    targets_delegating, timestamp,
};
use teigeodata_sync::trust::Trusted;

/// The gzip budget of each metadata file: top-level targets, snapshot, a country's role.
pub(crate) const BUDGET: u64 = 512 * 1024;
/// About the world's targets: every country and the parts of the large ones (the
/// engine-and-catalog note, §3.10).
pub(crate) const WORLD: usize = 5_000;
const NOW: i64 = 1_794_268_800;
const YEAR: i64 = 365 * 86_400;

/// One metadata file's size.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Bytes {
    pub(crate) json: u64,
    pub(crate) gzip: u64,
}

/// A world catalog, measured.
#[derive(Debug)]
pub(crate) struct Measured {
    pub(crate) targets: usize,
    pub(crate) roles: usize,
    pub(crate) top: Bytes,
    pub(crate) snapshot: Bytes,
    /// The largest country's role, and its name and targets.
    pub(crate) largest: (String, usize, Bytes),
    /// Verifying the chain from root 1 and reading every entry of every role, best of `runs`.
    pub(crate) read_all_ms: f64,
    /// The same for the largest country alone: what an app that wants it does.
    pub(crate) read_one_ms: f64,
}

impl Measured {
    /// Whether every file is within [`BUDGET`].
    pub(crate) fn within(&self) -> bool {
        [self.top, self.snapshot, self.largest.2]
            .iter()
            .all(|b| b.gzip <= BUDGET)
    }
}

/// A generated repository: root 1, top-level targets, snapshot, timestamp, and each country's
/// role by name.
pub(crate) struct World {
    root1: Vec<u8>,
    targets: Vec<u8>,
    snapshot: Vec<u8>,
    timestamp: Vec<u8>,
    roles: BTreeMap<String, Vec<u8>>,
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

/// Packs of no country, kept by the top-level targets.
const GLOBAL: [&str; 2] = ["earth", "oceans"];

/// The `n` addresses: two packs of no country, 250 two-letter countries, then parts of them,
/// country `i` getting a share of the parts in proportion to `1 / (i + 1)`.
fn addresses(n: usize) -> Vec<String> {
    let letter = |i: usize| char::from(b'a' + u8::try_from(i % 26).unwrap_or(0));
    let countries: Vec<String> = (0..250)
        .map(|i| [letter(i / 26), letter(i)].iter().collect())
        .collect();
    let mut out: Vec<String> = GLOBAL.iter().map(ToString::to_string).collect();
    out.extend(countries.iter().cloned());
    out.truncate(n);
    let parts = n.saturating_sub(out.len());
    #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
    let harmonic: f64 = (1..=countries.len()).map(|k| 1.0 / k as f64).sum();
    let mut given = 0;
    for (i, country) in countries.iter().enumerate() {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share of a count far below 2^52, never negative"
        )]
        let share = if i + 1 == countries.len() {
            parts - given
        } else {
            ((parts as f64 / (i + 1) as f64 / harmonic).round() as usize).min(parts - given)
        };
        for k in 0..share {
            out.push(format!("{country}-p{k:04}"));
        }
        given += share;
    }
    out
}

/// The signed repository of a generated world catalog of `n` targets, delegated by country
/// with the targets key (D-175).
pub(crate) fn generate(n: usize) -> Result<World, String> {
    let e = text;
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut files = BTreeMap::new();
    for address in addresses(n) {
        let (path, file) = target_file(&mut rng, &address)?;
        files.insert(path, file);
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
    let (top, countries) = by_country(files);
    let delegated = countries
        .keys()
        .map(|&cc| country_role(cc, one(30)))
        .collect::<Vec<_>>();
    let mut roles = BTreeMap::new();
    for (cc, files) in &countries {
        let name: String = cc.iter().map(|&b| char::from(b)).collect();
        roles.insert(name, signed_targets(files, &[])?);
    }
    let tg = signed_targets(&top, &delegated)?;
    let mut listed: Vec<(&str, &[u8], u64)> = vec![("targets", &tg, 1)];
    listed.extend(roles.iter().map(|(n, b)| (n.as_str(), b.as_slice(), 1)));
    let sn = sign(
        &snapshot_listing(1, NOW + YEAR, &listed).map_err(e)?,
        &[&key(20)],
    )
    .map_err(e)?;
    let ts = sign(&timestamp(1, NOW + 86_400, &sn, 1).map_err(e)?, &[&key(10)]).map_err(e)?;
    Ok(World {
        root1,
        targets: tg,
        snapshot: sn,
        timestamp: ts,
        roles,
    })
}

/// A generated target file for `address`, at its path.
fn target_file(rng: &mut Rng, address: &str) -> Result<(String, TargetFile), String> {
    let sha: [u8; 32] = rng.bytes();
    let part = address.contains('-');
    let country = address.len() == 2 || part;
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
        address: address.to_string(),
        component: "places".into(),
        release: "2027.1.0".into(),
        file: file_name(address, "places", "2027.1.0", &sha),
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
        parts: Vec::new(),
        deps: if part && country {
            vec![format!("{}.places.tgd", &address[..2])]
        } else {
            vec![]
        },
        min_reader: 1,
        bbox: Some(bbox),
        licence: "ODbL-1.0".into(),
    };
    Ok((
        entry.path(),
        TargetFile {
            length: len * 7 / 10,
            hashes: BTreeMap::from([("sha256".to_string(), sha.to_vec())]),
            custom: Some(entry.to_custom()),
        },
    ))
}

fn signed_targets(
    files: &Files,
    roles: &[teigeodata_sync::repo::DelegatedRole],
) -> Result<Vec<u8>, String> {
    sign(
        &targets_delegating(1, NOW + YEAR, files, roles).map_err(text)?,
        &[&key(30)],
    )
    .map_err(text)
}

/// Verifies the chain and the roles `only` names (every role when `None`), and reads every
/// entry they hold; the number of entries read.
pub(crate) fn read(w: &World, only: Option<&str>) -> Result<usize, String> {
    let e = text;
    let mut t = Trusted::new(&w.root1, NOW).map_err(e)?;
    t.update_timestamp(&w.timestamp).map_err(e)?;
    t.update_snapshot(&w.snapshot, false).map_err(e)?;
    t.update_targets(&w.targets).map_err(e)?;
    for (name, data) in &w.roles {
        if only.is_none_or(|o| o == name) {
            t.update_delegated(name, data)
                .map_err(|err| format!("{name}: {err}"))?;
        }
    }
    let (catalog, refused) = Catalog::trusted(&t);
    if let Some((path, err)) = refused.first() {
        return Err(format!("{path}: {err}"));
    }
    Ok(catalog.entries().len())
}

fn bytes_of(data: &[u8]) -> Result<Bytes, String> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(data).map_err(|e| e.to_string())?;
    Ok(Bytes {
        json: data.len() as u64,
        gzip: gz.finish().map_err(|e| e.to_string())?.len() as u64,
    })
}

/// The best time of `runs` reads, checking each read `want` entries.
fn timed(w: &World, only: Option<&str>, want: usize, runs: usize) -> Result<f64, String> {
    let mut best = f64::INFINITY;
    for _ in 0..runs.max(1) {
        let start = Instant::now();
        let got = read(w, only)?;
        best = best.min(start.elapsed().as_secs_f64() * 1e3);
        if got != want {
            return Err(format!("read {got} entries of {want}"));
        }
    }
    Ok(best)
}

/// Generates a world of `n` targets and measures it: each file's size, and the read times,
/// best of `runs`.
pub(crate) fn measure(n: usize, runs: usize) -> Result<Measured, String> {
    let w = generate(n)?;
    let (name, role) = w
        .roles
        .iter()
        .max_by_key(|(_, b)| b.len())
        .ok_or("no country")?;
    let global = GLOBAL.len().min(n);
    let in_largest = read(&w, Some(name))? - global;
    Ok(Measured {
        targets: n,
        roles: w.roles.len(),
        top: bytes_of(&w.targets)?,
        snapshot: bytes_of(&w.snapshot)?,
        largest: (name.clone(), in_largest, bytes_of(role)?),
        read_all_ms: timed(&w, None, n, runs)?,
        read_one_ms: timed(&w, Some(name), in_largest + global, runs)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{WORLD, generate, measure, read};

    /// The generator's entries are ones the client accepts, every one, through the roles; one
    /// country's role gives that country's packs and the packs of no country.
    #[test]
    fn a_generated_catalog_reads_back_whole() {
        let w = generate(600).unwrap();
        assert_eq!(read(&w, None).unwrap(), 600);
        let first = read(&w, Some("aa")).unwrap();
        let last = read(&w, Some("jp")).unwrap();
        assert!(first > last && last >= 3, "Zipf: {first} > {last}");
        assert_eq!(
            read(&w, Some("none")).unwrap(),
            2,
            "the packs of no country"
        );
    }

    /// Gate 12: each file of the world's catalog within the gzip budget (D-175).
    #[test]
    fn the_world_catalog_fits_its_budget() {
        let m = measure(WORLD, 1).unwrap();
        assert!(m.within(), "{m:?}");
        assert_eq!(m.roles, 250);
    }
}
