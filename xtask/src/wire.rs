//! Wire bytes per component (D-133, D-135): what a client downloads for each component. The
//! pack is written as its transport asset exactly as a release writes it (zstd level 19, frames
//! cut on the pack's structure, D-112), and each frame's compressed bytes are counted for the
//! component of the section it holds. A client that chooses components fetches the head frame,
//! the seek table, its components' frames with everything they need, and `BLOCK_HASHES` when one
//! of its sections is 1 MiB or more (D-078).
//!
//! Gates: the components, the head, the seek table and `BLOCK_HASHES` add up to the asset; and
//! each component a country budgets (`wire_kb` in its `build.yaml`), with what it needs, fits.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use teigeodata_format::component::{Component, Components};
use teigeodata_sync::measure::Measured;
use teigeodata_sync::transport;

/// The zstd level releases compress frames with.
const LEVEL: i32 = 19;

/// The transport asset of `bytes`, as a release writes it.
pub(crate) fn asset(bytes: &[u8]) -> Result<Vec<u8>, String> {
    transport::encode(bytes, |d| zstd::bulk::compress(d, LEVEL).ok()).map_err(|e| e.to_string())
}

/// One pack's asset, measured ([`Measured`], the catalog's own measure).
pub(crate) struct Wire(Measured);

impl Wire {
    /// Writes `bytes` as its transport asset in memory and measures it.
    pub(crate) fn measure(bytes: &[u8]) -> Result<Self, String> {
        Self::measure_asset(bytes).map(|(w, _)| w)
    }

    /// [`Wire::measure`], also giving the asset.
    pub(crate) fn measure_asset(bytes: &[u8]) -> Result<(Self, Vec<u8>), String> {
        let asset = asset(bytes)?;
        let m = Measured::of(bytes, &asset).map_err(|e| e.to_string())?;
        Ok((Self(m), asset))
    }

    /// What a client downloads for `set` and everything it needs.
    pub(crate) fn of(&self, set: Components) -> u64 {
        self.0.wire(set)
    }

    /// The problems: the parts not adding up to the asset, or a budgeted component over its
    /// budget (`kb`, component name → kilobytes of 1,000 bytes).
    pub(crate) fn gate(&self, kb: &BTreeMap<String, u64>) -> Vec<String> {
        let mut out = Vec::new();
        let sum = self.0.fixed + self.0.hashes + self.0.parts.values().map(|p| p.1).sum::<u64>();
        if sum != self.0.asset {
            out.push(format!(
                "components add up to {sum} B, the asset is {} B",
                self.0.asset
            ));
        }
        for (name, &limit) in kb {
            let Some(c) = Component::from_name(name) else {
                out.push(format!("wire_kb: no component `{name}`"));
                continue;
            };
            let got = self.of(Components::of(&[c]));
            if got > limit * 1000 {
                out.push(format!(
                    "{name}: {got} B on the wire with what it needs, over its budget of {limit} kB"
                ));
            }
        }
        out
    }

    /// The transport asset's bytes.
    pub(crate) const fn asset(&self) -> u64 {
        self.0.asset
    }

    /// [`Wire::report`] with each component's raw and wire bytes per place (`places`).
    #[allow(
        clippy::cast_precision_loss,
        reason = "bytes per place shown with two decimals"
    )]
    pub(crate) fn report_per_place(&self, places: usize) -> String {
        let per = |b: u64| b as f64 / places.max(1) as f64;
        let mut out = String::from(
            "| Component | Disk B | Wire B | Disk B/place | Wire B/place | Download with what it needs |\n|---|--:|--:|--:|--:|--:|\n",
        );
        for (&c, &(raw, wire)) in &self.0.parts {
            let _ = writeln!(
                out,
                "| {} | {raw} | {wire} | {:.2} | {:.2} | {} |",
                c.name(),
                per(raw),
                per(wire),
                self.of(Components::of(&[c]))
            );
        }
        let _ = writeln!(
            out,
            "| head and seek table | | {} | | | |\n| BLOCK_HASHES | | {} | | | |\n| **asset** | | {} | | {:.2} | |",
            self.0.fixed,
            self.0.hashes,
            self.0.asset,
            per(self.0.asset)
        );
        out
    }

    /// The table: each component's raw and wire bytes, and what a client downloads for it with
    /// everything it needs.
    pub(crate) fn report(&self) -> String {
        let mut out = String::from(
            "| Component | Raw | Wire | Download with what it needs |\n|---|--:|--:|--:|\n",
        );
        for (&c, &(raw, wire)) in &self.0.parts {
            let _ = writeln!(
                out,
                "| {} | {raw} | {wire} | {} |",
                c.name(),
                self.of(Components::of(&[c]))
            );
        }
        let _ = writeln!(
            out,
            "| head and seek table | | {} | |\n| BLOCK_HASHES | | {} | |\n| **asset** | | {} | |\n",
            self.0.fixed, self.0.hashes, self.0.asset
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use teigeodata_format::component::{Component, Components};
    use teigeodata_format::{Encoding, Kind, Writer};

    use super::Wire;

    /// A pack of pseudo-random sections (they only need valid hashes): a small `point`, a
    /// small `timezone` and a 2 MiB `timezone-index` that has block hashes.
    fn pack() -> Vec<u8> {
        let mut x = 0x9e37_79b9_u32;
        let mut noise = |n: usize| -> Vec<u8> {
            (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x.to_le_bytes()[0]
                })
                .collect()
        };
        let (ids, tz, region) = (noise(40_000), noise(3_000), noise(2 << 20));
        Writer::new("zz", "places", "2026.11.0")
            .unwrap()
            .pack(
                &[
                    (Kind::IDMAP, Encoding::RAW, 1, &ids),
                    (Kind::PLACE_TZ, Encoding::RAW, 1, &tz),
                    (Kind::REGION, Encoding::RAW, 1, &region),
                ],
                None,
            )
            .unwrap()
    }

    #[test]
    fn parts_add_up_and_budgets_are_enforced() {
        let w = Wire::measure(&pack()).unwrap();
        assert_eq!(w.gate(&BTreeMap::new()), Vec::<String>::new());
        assert!(w.0.hashes > 0, "a 2 MiB section has block hashes");
        let point = w.of(Components::of(&[Component::Point]));
        let tz = w.of(Components::of(&[Component::Timezone]));
        let index = w.of(Components::of(&[Component::TimezoneIndex]));
        // `timezone` needs `point`; only the index needs BLOCK_HASHES.
        assert!(point < tz && tz < index);
        assert_eq!(
            index,
            w.0.fixed
                + w.0.parts[&Component::Point].1
                + w.0.parts[&Component::TimezoneIndex].1
                + w.0.hashes
        );
        assert_eq!(w.of(Components::all()), w.0.asset);
        // The catalog's arithmetic gives G-wire's download bytes for every set of components.
        let c = w.0.catalog_sizes();
        let entry = teigeodata_sync::catalog::Entry {
            address: "zz".into(),
            component: "places".into(),
            release: "2026.11.0".into(),
            file: String::new(),
            pack_length: 0,
            head: [0; 16],
            seek_table: c.seek_table,
            head_frame: c.head_frame,
            block_hashes: c.block_hashes,
            sizes: c.sizes,
            parts: c.parts,
            deps: vec![],
            min_reader: 0,
            bbox: None,
            licence: "CC0-1.0".into(),
        };
        for bits in 0_u16..1 << 12 {
            let set = Components::of(
                &Component::ALL
                    .iter()
                    .copied()
                    .filter(|c| bits >> (*c as u16) & 1 == 1)
                    .collect::<Vec<_>>(),
            );
            assert_eq!(entry.size_of(set).wire, w.of(set), "{bits:#b}");
        }
        let budget = |name: &str, kb: u64| BTreeMap::from([(name.to_owned(), kb)]);
        assert_eq!(
            w.gate(&budget("point", point.div_ceil(1000))),
            Vec::<String>::new()
        );
        assert_eq!(w.gate(&budget("point", point / 1000 - 1)).len(), 1, "over");
        assert_eq!(
            w.gate(&budget("points", 9_999)).len(),
            1,
            "no such component"
        );
    }
}
