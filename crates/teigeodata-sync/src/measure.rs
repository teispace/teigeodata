//! A pack's transport asset measured per component (D-133, D-135, D-162), and its catalog
//! entry: the one description `xtask tuf release`, `check-pack`'s G-wire and every test that
//! publishes a pack share.
//!
//! Each frame's compressed bytes count for the component of the section whose span holds it;
//! the head frame and the seek table go to every client; `BLOCK_HASHES` to a client that holds
//! a section of 1 MiB or more (D-078). On disk a section takes its span: its bytes and the
//! padding to the next section, or to the footer for the last (D-172), so the parts add up to
//! the file, which is checked.

use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec::Vec;

use teigeodata_format::arcs::Arcs;
use teigeodata_format::component::{Component, Components};
use teigeodata_format::{FOOTER_SIZE, Kind, Pack, Part, Tag};

use crate::Error;
use crate::catalog::{BlockHashes, Entry, PartSize, Size, file_name};
use crate::transport::{Layout, SeekTable};

/// The catalog entry's size fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSizes {
    /// The seek table's bytes, at the asset's end.
    pub seek_table: u64,
    /// The head frame: its bytes in the asset and the head's bytes in the pack.
    pub head_frame: Size,
    /// `BLOCK_HASHES` and the components that need it, when the pack has it.
    pub block_hashes: Option<BlockHashes>,
    /// Each component's download and disk bytes.
    pub sizes: BTreeMap<Component, Size>,
    /// Each part of a split kind but its base, its tolerance not yet filled (D-253).
    pub parts: Vec<PartSize>,
}

/// One pack's asset, measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measured {
    /// The whole asset.
    pub asset: u64,
    /// The head frame and the seek table: every client fetches both.
    pub fixed: u64,
    /// The seek table alone.
    pub table: u64,
    /// The head's bytes in the pack (what the head frame decodes to).
    pub head_raw: u64,
    /// `BLOCK_HASHES` in the asset.
    pub hashes: u64,
    /// `BLOCK_HASHES` on disk.
    pub hashes_raw: u64,
    /// Per component of its own sections: disk bytes (spans) and asset bytes.
    pub parts: BTreeMap<Component, (u64, u64)>,
    /// The same for each part of a split kind but its base (ADR-0040): disk and asset bytes.
    pub split: BTreeMap<(Component, Part), (u64, u64)>,
    /// The components with a section of 1 MiB or more, which need `BLOCK_HASHES`.
    pub blocked: Components,
}

impl Measured {
    /// Measures `asset`, the transport asset of `pack`.
    ///
    /// # Errors
    /// [`Error::SeekTable`] when the asset is not the pack's, [`Error::Pack`] for a refused
    /// pack, [`Error::Layout`] for a section with no component or parts that do not add up.
    pub fn of(pack: &[u8], asset: &[u8]) -> Result<Self, Error> {
        let p = Pack::open(pack).map_err(Error::Pack)?;
        let tail = SeekTable::tail_len(asset)?;
        let table = asset
            .get(asset.len().saturating_sub(tail)..)
            .ok_or(Error::SeekTable("longer than the asset"))?;
        let asset_len = asset.len() as u64;
        let frames = SeekTable::parse(table)?.check(asset_len, Layout::of(&p)?)?;
        let spans = frames.layout().sections().to_vec();
        let mut m = Self {
            asset: asset_len,
            fixed: frames.table_len(),
            table: frames.table_len(),
            head_raw: 0,
            hashes: 0,
            hashes_raw: 0,
            parts: BTreeMap::new(),
            split: BTreeMap::new(),
            blocked: Components::NONE,
        };
        for f in frames.iter() {
            let len = f.compressed.end - f.compressed.start;
            // The section whose span (to the next section) holds the frame; none: the head.
            let Some(s) = spans.iter().rev().find(|s| s.offset <= f.decoded.start) else {
                m.fixed += len;
                m.head_raw += f.decoded.end - f.decoded.start;
                continue;
            };
            match Component::of(s.kind) {
                Some(k) => {
                    m.parts.entry(k).or_default().1 += len;
                    if !s.part.is_base() {
                        m.split.entry((k, s.part)).or_default().1 += len;
                    }
                }
                None if s.kind == Kind::BLOCK_HASHES => m.hashes += len,
                None => return Err(Error::Layout("a section has no component")),
            }
        }
        let body_end = (pack.len() - FOOTER_SIZE) as u64;
        for (i, s) in spans.iter().enumerate() {
            let end = spans.get(i + 1).map_or(body_end, |n| n.offset);
            let disk = end
                .checked_sub(s.offset)
                .ok_or(Error::Layout("sections out of order"))?;
            if s.kind == Kind::BLOCK_HASHES {
                m.hashes_raw += disk;
            }
            if let Some(k) = Component::of(s.kind) {
                m.parts.entry(k).or_default().0 += disk;
                if !s.part.is_base() {
                    m.split.entry((k, s.part)).or_default().0 += disk;
                }
            }
        }
        for s in p.sections() {
            if let Some(k) = Component::of(s.kind())
                && p.part_blocks(s.kind(), s.part())
                    .map_err(Error::Pack)?
                    .is_some()
            {
                m.blocked = Components::of(&[k]).union(m.blocked);
            }
        }
        let disk = m.head_raw
            + m.hashes_raw
            + m.parts.values().map(|p| p.0).sum::<u64>()
            + FOOTER_SIZE as u64;
        if disk != pack.len() as u64 {
            return Err(Error::Layout("the parts do not add up to the pack"));
        }
        Ok(m)
    }

    /// The catalog's sizes (D-162).
    #[must_use]
    pub fn catalog_sizes(&self) -> CatalogSizes {
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
            parts: self
                .split
                .iter()
                .map(|(&(component, part), &(raw, wire))| PartSize {
                    component,
                    part,
                    size: size(wire, raw),
                    tolerance_m: 0,
                })
                .collect(),
        }
    }

    /// What a client downloads for `set` and everything it needs: the head, the seek table,
    /// those components' frames, and `BLOCK_HASHES` if one of them needs it.
    #[must_use]
    pub fn wire(&self, set: Components) -> u64 {
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
}

/// The SPDX identifier of a manifest's licence class (ADR-0002).
///
/// # Errors
/// [`Error::Write`] for a class with no mapping.
pub fn spdx(class: &str) -> Result<&'static str, Error> {
    match class {
        "odbl" => Ok("ODbL-1.0"),
        "cc-by-4.0" => Ok("CC-BY-4.0"),
        _ => Err(Error::Write("the pack's licence has no SPDX mapping")),
    }
}

/// The catalog entry of `pack` published as `asset`, with no dependencies.
///
/// # Errors
/// As [`Measured::of`], and [`Error::Write`] when the pack states no licence or one with no
/// SPDX mapping.
pub fn entry(pack: &[u8], asset: &[u8]) -> Result<Entry, Error> {
    let mut c = Measured::of(pack, asset)?.catalog_sizes();
    let p = Pack::open(pack).map_err(Error::Pack)?;
    // A coarser outline part's tolerance, from the base ARCS's levels (D-250).
    let coarse = c
        .parts
        .iter()
        .any(|q| q.part.outline_levels().is_some_and(|(_, lod)| lod > 0));
    let arcs = p
        .section(Kind::ARCS)
        .filter(|_| coarse)
        .map(|s| Arcs::open(&s))
        .transpose()
        .map_err(Error::Pack)?;
    for part in &mut c.parts {
        if let (Some((_, lod @ 1..)), Some(a)) = (part.part.outline_levels(), &arcs)
            && part.component.kinds().iter().any(|k| k.is_outline())
        {
            part.tolerance_m = a.tolerance_cm(lod).map_or(0, |cm| cm / 100);
        }
    }
    let m = p.manifest();
    let licence = spdx(
        m.text(Tag::LICENCE)
            .ok_or(Error::Write("the pack states no licence"))?,
    )?;
    let sha: Vec<u8> = {
        use sha2::Digest as _;
        sha2::Sha256::digest(asset).to_vec()
    };
    let mut head = [0_u8; 16];
    head.copy_from_slice(p.head_digest().get(..16).unwrap_or(&[0; 16]));
    Ok(Entry {
        address: m.address().to_string(),
        component: m.component().to_string(),
        release: m.release().to_string(),
        file: file_name(m.address(), m.component(), m.release(), &sha),
        pack_length: pack.len() as u64,
        head,
        seek_table: c.seek_table,
        head_frame: c.head_frame,
        block_hashes: c.block_hashes,
        sizes: c.sizes,
        parts: c.parts,
        deps: Vec::new(),
        min_reader: p.min_reader_major(),
        bbox: m.bbox(),
        licence: licence.to_string(),
    })
}
