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
use teigeodata_format::{Kind, Pack};
use teigeodata_sync::catalog::{BlockHashes, Size};
use teigeodata_sync::transport::{self, Layout, SeekTable};

/// The zstd level releases compress frames with.
const LEVEL: i32 = 19;

/// The catalog entry's size fields, from the measured asset.
pub(crate) struct CatalogSizes {
    pub(crate) seek_table: u64,
    pub(crate) head_frame: Size,
    pub(crate) block_hashes: Option<BlockHashes>,
    pub(crate) sizes: BTreeMap<Component, Size>,
}

/// One pack's asset, measured.
pub(crate) struct Wire {
    /// The whole asset.
    asset: u64,
    /// The head frame and the seek table: every client fetches both.
    fixed: u64,
    /// The seek table alone.
    table: u64,
    /// The head's bytes in the pack (what the head frame decodes to).
    head_raw: u64,
    /// `BLOCK_HASHES`.
    hashes: u64,
    /// `BLOCK_HASHES` in the pack.
    hashes_raw: u64,
    /// Raw and compressed bytes of each component's own sections.
    parts: BTreeMap<Component, (u64, u64)>,
    /// The components with a section of 1 MiB or more, which needs `BLOCK_HASHES`.
    blocked: Components,
}

impl Wire {
    /// Writes `bytes` as its transport asset in memory and measures it.
    pub(crate) fn measure(bytes: &[u8]) -> Result<Self, String> {
        Self::measure_asset(bytes).map(|(w, _)| w)
    }

    /// [`Wire::measure`], also giving the asset.
    pub(crate) fn measure_asset(bytes: &[u8]) -> Result<(Self, Vec<u8>), String> {
        let err = |e: teigeodata_sync::Error| e.to_string();
        let asset =
            transport::encode(bytes, |d| zstd::bulk::compress(d, LEVEL).ok()).map_err(err)?;
        let pack = Pack::open(bytes).map_err(|e| e.to_string())?;
        let tail = SeekTable::tail_len(&asset).map_err(err)?;
        let table = asset
            .get(asset.len().saturating_sub(tail)..)
            .ok_or("the seek table is longer than the asset")?;
        let asset_len = asset.len() as u64;
        let frames = SeekTable::parse(table)
            .and_then(|t| t.check(asset_len, Layout::of(&pack)?))
            .map_err(err)?;
        let spans = frames.layout().sections().to_vec();
        let mut wire = Self {
            asset: asset_len,
            fixed: frames.table_len(),
            table: frames.table_len(),
            head_raw: 0,
            hashes: 0,
            hashes_raw: 0,
            parts: BTreeMap::new(),
            blocked: Components::NONE,
        };
        for f in frames.iter() {
            let len = f.compressed.end - f.compressed.start;
            // The section whose span (to the next section) holds the frame; none: the head.
            let Some(s) = spans.iter().rev().find(|s| s.offset <= f.decoded.start) else {
                wire.fixed += len;
                wire.head_raw += f.decoded.end - f.decoded.start;
                continue;
            };
            match Component::of(s.kind) {
                Some(k) => wire.parts.entry(k).or_default().1 += len,
                None if s.kind == Kind::BLOCK_HASHES => wire.hashes += len,
                None => return Err(format!("section {} has no component", s.kind)),
            }
        }
        for s in pack.sections() {
            if s.kind() == Kind::BLOCK_HASHES {
                wire.hashes_raw += s.bytes().len() as u64;
            }
            if let Some(k) = Component::of(s.kind()) {
                wire.parts.entry(k).or_default().0 += s.bytes().len() as u64;
                if pack.blocks(s.kind()).map_err(|e| e.to_string())?.is_some() {
                    wire.blocked = Components::of(&[k]).union(wire.blocked);
                }
            }
        }
        Ok((wire, asset))
    }

    /// The catalog's sizes for this pack (D-162): the head frame, the seek table, each
    /// component, and `BLOCK_HASHES` with the components that need it.
    pub(crate) fn catalog_sizes(&self) -> CatalogSizes {
        let size = |wire: u64, disk: u64| Size { wire, disk };
        CatalogSizes {
            seek_table: self.table,
            head_frame: size(self.fixed - self.table, self.head_raw),
            block_hashes: (self.hashes > 0).then(|| BlockHashes {
                size: size(self.hashes, self.hashes_raw),
                needed_by: self.blocked,
            }),
            sizes: self
                .parts
                .iter()
                .map(|(&c, &(raw, wire))| (c, size(wire, raw)))
                .collect(),
        }
    }

    /// What a client downloads for `set` and everything it needs: the head, the seek table,
    /// those components' frames, and `BLOCK_HASHES` if one of them needs it.
    pub(crate) fn of(&self, set: Components) -> u64 {
        let set = set.closed();
        let own: u64 = set
            .iter()
            .filter_map(|c| self.parts.get(&c))
            .map(|p| p.1)
            .sum();
        let hashes = if set.iter().any(|c| self.blocked.contains(c)) {
            self.hashes
        } else {
            0
        };
        self.fixed + own + hashes
    }

    /// The problems: the parts not adding up to the asset, or a budgeted component over its
    /// budget (`kb`, component name → kilobytes of 1,000 bytes).
    pub(crate) fn gate(&self, kb: &BTreeMap<String, u64>) -> Vec<String> {
        let mut out = Vec::new();
        let sum = self.fixed + self.hashes + self.parts.values().map(|p| p.1).sum::<u64>();
        if sum != self.asset {
            out.push(format!(
                "components add up to {sum} B, the asset is {} B",
                self.asset
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

    /// The table: each component's raw and wire bytes, and what a client downloads for it with
    /// everything it needs.
    pub(crate) fn report(&self) -> String {
        let mut out = String::from(
            "| Component | Raw | Wire | Download with what it needs |\n|---|--:|--:|--:|\n",
        );
        for (&c, &(raw, wire)) in &self.parts {
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
            self.fixed, self.hashes, self.asset
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
        assert!(w.hashes > 0, "a 2 MiB section has block hashes");
        let point = w.of(Components::of(&[Component::Point]));
        let tz = w.of(Components::of(&[Component::Timezone]));
        let index = w.of(Components::of(&[Component::TimezoneIndex]));
        // `timezone` needs `point`; only the index needs BLOCK_HASHES.
        assert!(point < tz && tz < index);
        assert_eq!(
            index,
            w.fixed
                + w.parts[&Component::Point].1
                + w.parts[&Component::TimezoneIndex].1
                + w.hashes
        );
        assert_eq!(w.of(Components::all()), w.asset);
        // The catalog's arithmetic gives G-wire's download bytes for every set of components.
        let c = w.catalog_sizes();
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
