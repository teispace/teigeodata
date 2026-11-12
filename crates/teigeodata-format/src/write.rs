//! The deterministic writer (feature `write`).
//!
//! The writer never holds section data. It is given each section's length and BLAKE3 hash
//! ([`Writer::section`]), plans the layout ([`Writer::layout`]) and returns the head, the
//! offset of every section and the footer. A caller streams the sections to disk at those
//! offsets, so a world-sized pack is written with bounded memory. [`Layout::assemble`] does the
//! same in memory for small packs and tests.

use alloc::collections::BTreeMap;
use alloc::collections::btree_map::Entry;
use alloc::vec::Vec;

use zerocopy::IntoBytes;
use zerocopy::byteorder::little_endian::{U16, U32, U64};

use crate::layout::{
    ENTRY_LEN, Entry as TableEntry, FOOTER_BYTES, FOOTER_LEN, Footer, HEAD_MAX, HEADER_HASHED,
    HEADER_LEN, Header, MAGIC, SIGNATURE_LEN, align_up, head_digest, pack_uuid,
};
use crate::manifest::{REQUIRED, check};
use crate::{Encoding, Error, FORMAT_MAJOR, FORMAT_MINOR, Kind, Part, Tag};

/// One planned section.
#[derive(Clone, Copy, Debug)]
struct Planned {
    encoding: Encoding,
    item_count: u64,
    decoded_length: u64,
    length: u64,
    hash: [u8; 32],
}

/// Makes the 72-byte signature block for a head digest (see `signer`).
pub type Sign<'s> = dyn Fn(&[u8; 32]) -> [u8; 72] + 's;

/// Collects a pack's manifest and section descriptions.
#[derive(Clone, Debug)]
pub struct Writer {
    manifest: BTreeMap<Tag, Vec<u8>>,
    sections: BTreeMap<(Kind, Part), Planned>,
    build_id: u64,
    created_unix: u64,
}

/// Where a section goes in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    /// The section's kind.
    pub kind: Kind,
    /// Its part of the kind (ADR-0040).
    pub part: Part,
    /// Its offset from the start of the file (64-byte aligned).
    pub offset: u64,
    /// Its length.
    pub length: u64,
    /// Its BLAKE3 hash, as given to the writer.
    pub hash: [u8; 32],
}

/// A planned pack: the head, where each section goes, and the footer.
#[derive(Clone, Debug)]
pub struct Layout {
    head: Vec<u8>,
    signature: Option<core::ops::Range<usize>>,
    signature_set: bool,
    digest: [u8; 32],
    placed: Vec<Placed>,
    footer: [u8; FOOTER_BYTES],
    file_len: u64,
}

impl Writer {
    /// A writer for the pack with this address, component and release (ADR-0010). They name the
    /// pack and derive its UUID.
    ///
    /// # Errors
    /// [`Error::Write`] if a value is not a valid name.
    pub fn new(address: &str, component: &str, release: &str) -> Result<Self, Error> {
        let mut manifest = BTreeMap::new();
        for (tag, v) in REQUIRED.into_iter().zip([address, component, release]) {
            check(tag, v.as_bytes()).map_err(Error::Write)?;
            manifest.insert(tag, v.as_bytes().to_vec());
        }
        Ok(Self {
            manifest,
            sections: BTreeMap::new(),
            build_id: 0,
            created_unix: 0,
        })
    }

    /// Sets the build ID (a hash of the builder version and `sources.lock`).
    pub const fn build_id(&mut self, id: u64) -> &mut Self {
        self.build_id = id;
        self
    }

    /// Sets the creation time: the build's `SOURCE_DATE_EPOCH`. Default 0. Never the clock, so
    /// that builds are reproducible.
    pub const fn created_unix(&mut self, secs: u64) -> &mut Self {
        self.created_unix = secs;
        self
    }

    /// Adds a manifest entry. Entries are written in tag order whatever the order of calls.
    ///
    /// # Errors
    /// [`Error::Write`] for a repeated tag, a bad value for a known tag, or an unknown critical
    /// tag (this writer could not read the pack back).
    pub fn manifest(&mut self, tag: Tag, value: &[u8]) -> Result<&mut Self, Error> {
        check(tag, value).map_err(Error::Write)?;
        if u32::try_from(value.len()).is_err() {
            return Err(Error::Write("a manifest value is longer than 4 GiB"));
        }
        // Refuse before inserting: a failed call must leave the writer unchanged.
        match self.manifest.entry(tag) {
            Entry::Occupied(_) => Err(Error::Write("a manifest tag was given twice")),
            Entry::Vacant(slot) => {
                slot.insert(value.to_vec());
                Ok(self)
            }
        }
    }

    /// Adds a section by its length and BLAKE3 hash, for data streamed elsewhere: the base
    /// part of its kind.
    ///
    /// # Errors
    /// [`Error::Write`] for kind 0, a repeated kind, or a raw section whose decoded length
    /// differs from its length.
    pub fn section(
        &mut self,
        kind: Kind,
        encoding: Encoding,
        item_count: u64,
        decoded_length: u64,
        length: u64,
        hash: [u8; 32],
    ) -> Result<&mut Self, Error> {
        self.section_part(
            kind,
            Part::BASE,
            encoding,
            item_count,
            decoded_length,
            length,
            hash,
        )
    }

    /// Adds part `part` of a kind by its length and BLAKE3 hash (ADR-0040). The layout
    /// refuses a part other than the base without its kind's base part.
    ///
    /// # Errors
    /// As [`Writer::section`], and for a part other than the base of a kind that takes none.
    #[allow(
        clippy::too_many_arguments,
        reason = "a section's table entry, field by field"
    )]
    pub fn section_part(
        &mut self,
        kind: Kind,
        part: Part,
        encoding: Encoding,
        item_count: u64,
        decoded_length: u64,
        length: u64,
        hash: [u8; 32],
    ) -> Result<&mut Self, Error> {
        if kind.0 == 0 {
            return Err(Error::Write("section kind 0 is reserved"));
        }
        if !part.is_base() && !kind.takes_parts() {
            return Err(Error::Write("a part of a kind that takes none"));
        }
        if encoding == Encoding::RAW && decoded_length != length {
            return Err(Error::Write(
                "a raw section's decoded length must equal its length",
            ));
        }
        let planned = Planned {
            encoding,
            item_count,
            decoded_length,
            length,
            hash,
        };
        match self.sections.entry((kind, part)) {
            Entry::Occupied(_) => Err(Error::Write("a section kind was given twice")),
            Entry::Vacant(slot) => {
                slot.insert(planned);
                Ok(self)
            }
        }
    }

    /// Adds a raw section from its bytes: hashes them and calls [`Writer::section`].
    ///
    /// # Errors
    /// As [`Writer::section`].
    pub fn section_bytes(
        &mut self,
        kind: Kind,
        encoding: Encoding,
        item_count: u64,
        bytes: &[u8],
    ) -> Result<&mut Self, Error> {
        self.part_bytes(kind, Part::BASE, encoding, item_count, bytes)
    }

    /// Adds part `part` of a kind from its bytes: hashes them and calls
    /// [`Writer::section_part`].
    ///
    /// # Errors
    /// As [`Writer::section_part`].
    pub fn part_bytes(
        &mut self,
        kind: Kind,
        part: Part,
        encoding: Encoding,
        item_count: u64,
        bytes: &[u8],
    ) -> Result<&mut Self, Error> {
        let len = u64::try_from(bytes.len()).map_err(|_| Error::Write("section too long"))?;
        let hash = *blake3::hash(bytes).as_bytes();
        self.section_part(kind, part, encoding, item_count, len, len, hash)
    }

    /// The coverage rule of `BLOCK_HASHES` (B12), from the planned lengths: a list exactly when
    /// a section is 1 MiB or more, of the length and item count those sections need.
    fn check_block_hashes(&self) -> Result<(), Error> {
        use crate::blocks::{blocks_of, covered};
        let big: Vec<u64> = self
            .sections
            .iter()
            .filter(|((k, _), p)| covered(*k, p.length))
            .map(|(_, p)| p.length)
            .collect();
        let cvs: u64 = big.iter().map(|&l| blocks_of(l)).sum();
        let want = 16 + 16 * big.len() as u64 + 32 * cvs;
        match self.sections.get(&(Kind::BLOCK_HASHES, Part::BASE)) {
            None if big.is_empty() => Ok(()),
            Some(p)
                if !big.is_empty()
                    && p.encoding == Encoding::RAW
                    && p.length == want
                    && p.item_count == cvs =>
            {
                Ok(())
            }
            _ => Err(Error::Write(
                "BLOCK_HASHES must cover exactly the sections of 1 MiB or more (blocks::encode)",
            )),
        }
    }

    /// The planned sections' rules: parts, then block hashes.
    fn check_planned(&self) -> Result<(), Error> {
        self.check_parts()?;
        self.check_block_hashes()
    }

    /// Every part other than the base comes with its kind's base part (ADR-0040).
    fn check_parts(&self) -> Result<(), Error> {
        if self
            .sections
            .keys()
            .any(|&(k, p)| !p.is_base() && !self.sections.contains_key(&(k, Part::BASE)))
        {
            return Err(Error::Write("a part comes without its kind's base part"));
        }
        Ok(())
    }

    /// Writes a whole pack in memory: adds `sections` (kind, encoding, item count, bytes), the
    /// `BLOCK_HASHES` list they need, plans the layout, signs it with `sign` when given (see
    /// `signer`), and assembles the file. For packs that fit in memory; a larger pack streams
    /// its sections at [`Layout::placed`] offsets instead.
    ///
    /// # Errors
    /// As [`Writer::section_bytes`], [`Writer::layout`] and [`Layout::assemble`].
    pub fn pack(
        self,
        sections: &[(Kind, Encoding, u64, &[u8])],
        sign: Option<&Sign<'_>>,
    ) -> Result<Vec<u8>, Error> {
        let parts: Vec<(Kind, Part, Encoding, u64, &[u8])> = sections
            .iter()
            .map(|&(k, e, n, b)| (k, Part::BASE, e, n, b))
            .collect();
        self.pack_parts(&parts, sign)
    }

    /// [`Writer::pack`] with each section's part of its kind (ADR-0040).
    ///
    /// # Errors
    /// As [`Writer::pack`] and [`Writer::part_bytes`].
    pub fn pack_parts(
        mut self,
        sections: &[(Kind, Part, Encoding, u64, &[u8])],
        sign: Option<&Sign<'_>>,
    ) -> Result<Vec<u8>, Error> {
        let mut bodies: Vec<(Kind, Part, &[u8])> =
            sections.iter().map(|s| (s.0, s.1, s.4)).collect();
        for &(kind, part, encoding, items, body) in sections {
            self.part_bytes(kind, part, encoding, items, body)?;
        }
        let list = crate::blocks::encode_parts(&bodies)?;
        if let Some(l) = &list {
            let items = crate::blocks::item_count(l);
            self.section_bytes(Kind::BLOCK_HASHES, Encoding::RAW, items, l)?;
            bodies.push((Kind::BLOCK_HASHES, Part::BASE, l));
        }
        let mut layout = self.layout(sign.is_some())?;
        if let Some(sign) = sign {
            let block = sign(layout.digest());
            layout.set_signature(&block)?;
        }
        let ordered: Vec<&[u8]> = layout
            .placed()
            .iter()
            .filter_map(|p| {
                bodies
                    .iter()
                    .find(|b| (b.0, b.1) == (p.kind, p.part))
                    .map(|b| b.2)
            })
            .collect();
        layout.assemble(&ordered)
    }

    /// Plans the file. With `signed`, a 72-byte signature slot follows the manifest, to be
    /// filled by [`Layout::set_signature`] from [`Layout::digest`].
    ///
    /// # Errors
    /// [`Error::Write`] if the head exceeds 16 KiB or the file exceeds `u64`.
    pub fn layout(&self, signed: bool) -> Result<Layout, Error> {
        self.check_planned()?;
        let too_big = Error::Write("the head is longer than 16 KiB");
        let mut manifest = Vec::new();
        for (tag, v) in &self.manifest {
            manifest.extend_from_slice(&tag.0.to_le_bytes());
            let len = u32::try_from(v.len()).map_err(|_| too_big)?;
            manifest.extend_from_slice(&len.to_le_bytes());
            manifest.extend_from_slice(v);
        }
        let count = u32::try_from(self.sections.len()).map_err(|_| too_big)?;
        let manifest_off = HEADER_LEN + u64::from(count) * ENTRY_LEN;
        let manifest_len = u32::try_from(manifest.len()).map_err(|_| too_big)?;
        let sig_off = manifest_off + u64::from(manifest_len);
        let head_end = sig_off + if signed { u64::from(SIGNATURE_LEN) } else { 0 };
        if head_end > HEAD_MAX {
            return Err(too_big);
        }

        let overflow = Error::Write("the pack is larger than 2^64 bytes");
        let mut pos = head_end;
        let mut placed = Vec::with_capacity(self.sections.len());
        let mut table = Vec::with_capacity(self.sections.len());
        for (&(kind, part), p) in &self.sections {
            let offset = align_up(pos).ok_or(overflow)?;
            pos = offset.checked_add(p.length).ok_or(overflow)?;
            placed.push(Placed {
                kind,
                part,
                offset,
                length: p.length,
                hash: p.hash,
            });
            table.push(TableEntry {
                kind: U32::new(kind.0),
                encoding: U16::new(p.encoding.0),
                flags: U16::ZERO,
                offset: U64::new(offset),
                length: U64::new(p.length),
                decoded_length: U64::new(p.decoded_length),
                item_count: U64::new(p.item_count),
                blake3: p.hash,
                part: U32::new(part.0),
                reserved: [0; 4],
            });
        }
        let file_len = pos.checked_add(FOOTER_LEN).ok_or(overflow)?;

        let text = |t| self.manifest.get(&t).map_or(&[][..], Vec::as_slice);
        let utf8 = |t| core::str::from_utf8(text(t)).unwrap_or_default();
        let mut header = Header {
            magic: MAGIC,
            format_major: U16::new(FORMAT_MAJOR),
            format_minor: U16::new(FORMAT_MINOR),
            min_reader_major: U16::new(FORMAT_MAJOR),
            flags: U16::ZERO,
            pack_uuid: pack_uuid(utf8(Tag::ADDRESS), utf8(Tag::COMPONENT), utf8(Tag::RELEASE)),
            build_id: U64::new(self.build_id),
            created_unix: U64::new(self.created_unix),
            table_off: U64::new(HEADER_LEN),
            section_count: U32::new(count),
            manifest_len: U32::new(manifest_len),
            manifest_off: U64::new(manifest_off),
            signature_off: U64::new(if signed { sig_off } else { 0 }),
            signature_len: U32::new(if signed { SIGNATURE_LEN } else { 0 }),
            file_len: U64::new(file_len),
            reserved: [0; 20],
            head_hash: [0; 16],
        };
        let digest = head_digest(
            header.as_bytes().get(..HEADER_HASHED).unwrap_or_default(),
            table.as_bytes(),
            &manifest,
        );
        header
            .head_hash
            .copy_from_slice(digest.get(..16).unwrap_or(&[0; 16]));

        // The head runs up to the first section, so its zero padding is included.
        let head_len = placed.first().map_or(head_end, |p| p.offset);
        let mut head = Vec::with_capacity(usize::try_from(head_len).unwrap_or(0));
        head.extend_from_slice(header.as_bytes());
        head.extend_from_slice(table.as_bytes());
        head.extend_from_slice(&manifest);
        let signature = signed.then(|| {
            let start = head.len();
            start..start + SIGNATURE_LEN as usize
        });
        head.resize(usize::try_from(head_len).map_err(|_| overflow)?, 0);

        let footer = Footer {
            magic: MAGIC,
            file_len: U64::new(file_len),
            head_hash: header.head_hash,
        };
        let mut footer_bytes = [0; FOOTER_BYTES];
        footer_bytes.copy_from_slice(footer.as_bytes());
        Ok(Layout {
            head,
            signature,
            signature_set: false,
            digest,
            placed,
            footer: footer_bytes,
            file_len,
        })
    }
}

impl Layout {
    /// The head digest, which a signature signs.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Fills the signature slot: an 8-byte key ID, then the 64-byte Ed25519 signature of
    /// [`crate::SIGNATURE_DOMAIN`] followed by the digest. With the `signature` feature,
    /// [`crate::signer`] makes it.
    ///
    /// # Errors
    /// [`Error::Write`] if the layout was planned unsigned.
    pub fn set_signature(&mut self, block: &[u8; 72]) -> Result<(), Error> {
        let slot = self
            .signature
            .clone()
            .and_then(|r| self.head.get_mut(r))
            .ok_or(Error::Write("the layout was planned without a signature"))?;
        slot.copy_from_slice(block);
        self.signature_set = true;
        Ok(())
    }

    /// The bytes from offset 0 to the first section (or to the footer if there is none),
    /// including zero padding.
    ///
    /// # Errors
    /// [`Error::Write`] if the signature slot is still empty.
    pub fn head(&self) -> Result<&[u8], Error> {
        if self.signature.is_some() && !self.signature_set {
            return Err(Error::Write("the signature has not been set"));
        }
        Ok(&self.head)
    }

    /// Where each section goes, in file order. Bytes between sections are zero.
    #[must_use]
    pub fn placed(&self) -> &[Placed] {
        &self.placed
    }

    /// The footer: the last 32 bytes of the file.
    #[must_use]
    pub const fn footer(&self) -> &[u8; 32] {
        &self.footer
    }

    /// The length of the whole file.
    #[must_use]
    pub const fn file_len(&self) -> u64 {
        self.file_len
    }

    /// Writes the whole pack into memory. `sections` are the bytes of each section in
    /// [`Layout::placed`] order.
    ///
    /// # Errors
    /// [`Error::Write`] if the signature is unset, or a section's length or hash differs from
    /// what the writer was given.
    pub fn assemble(&self, sections: &[&[u8]]) -> Result<Vec<u8>, Error> {
        if sections.len() != self.placed.len() {
            return Err(Error::Write(
                "the number of sections differs from the layout",
            ));
        }
        let len = usize::try_from(self.file_len).map_err(|_| Error::Write("pack too large"))?;
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(self.head()?);
        for (p, bytes) in self.placed.iter().zip(sections) {
            if u64::try_from(bytes.len()).ok() != Some(p.length)
                || blake3::hash(bytes).as_bytes() != &p.hash
            {
                return Err(Error::Write(
                    "a section's bytes differ from its length or hash",
                ));
            }
            if p.kind == Kind::BLOCK_HASHES {
                let all: Vec<(Kind, Part, u64, [u8; 32])> = self
                    .placed
                    .iter()
                    .map(|q| (q.kind, q.part, q.length, q.hash))
                    .collect();
                crate::blocks::prove_all(bytes, &all)?;
            }
            out.resize(usize::try_from(p.offset).unwrap_or(usize::MAX), 0);
            out.extend_from_slice(bytes);
        }
        out.extend_from_slice(&self.footer);
        Ok(out)
    }
}
