//! The validating reader. [`Pack::open`] checks the whole structure before returning; after it,
//! every accessor is a cheap view and cannot fail.

use zerocopy::FromBytes;

use crate::layout::{
    ENTRY_LEN, Entry, FOOTER_LEN, Footer, HEAD_MAX, HEADER_HASHED, HEADER_LEN, Header, MAGIC,
    SIGNATURE_LEN, align_up, head_digest, pack_uuid,
};
use crate::{Encoding, Error, Kind, Manifest, READER_MAJOR};

/// An open, structurally valid pack, borrowing its bytes.
#[derive(Debug)]
pub struct Pack<'a> {
    bytes: &'a [u8],
    header: &'a Header,
    table: &'a [Entry],
    manifest: Manifest<'a>,
    digest: [u8; 32],
    signature: Option<&'a [u8]>,
}

/// One section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Section<'a> {
    kind: Kind,
    encoding: Encoding,
    item_count: u64,
    decoded_length: u64,
    hash: &'a [u8; 32],
    bytes: &'a [u8],
}

impl<'a> Section<'a> {
    /// A column-table section that is not in a pack, for a writer that reads back what it
    /// encoded (feature `write`). Its hash is all zero, so [`Section::verify`] fails.
    #[cfg(feature = "write")]
    #[must_use]
    pub const fn unpacked(kind: Kind, item_count: u64, bytes: &'a [u8]) -> Self {
        Self {
            kind,
            encoding: Encoding::TABLE,
            item_count,
            decoded_length: bytes.len() as u64,
            hash: &[0; 32],
            bytes,
        }
    }

    /// The section's kind.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// How the section's bytes are encoded.
    #[must_use]
    pub const fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// The number of items (rows, entries) the section holds.
    #[must_use]
    pub const fn item_count(&self) -> u64 {
        self.item_count
    }

    /// The length after decoding; equal to the stored length for [`Encoding::RAW`].
    #[must_use]
    pub const fn decoded_length(&self) -> u64 {
        self.decoded_length
    }

    /// The BLAKE3 hash of the stored bytes, from the section table.
    #[must_use]
    pub const fn hash(&self) -> &'a [u8; 32] {
        self.hash
    }

    /// The stored bytes. Not yet checked against [`Section::hash`]: see [`Section::verify`].
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Checks the bytes against the hash.
    ///
    /// # Errors
    /// [`Error::SectionHash`] if they differ.
    pub fn verify(&self) -> Result<(), Error> {
        if blake3::hash(self.bytes).as_bytes() == self.hash {
            Ok(())
        } else {
            Err(Error::SectionHash { kind: self.kind.0 })
        }
    }
}

fn usize_of(v: u64) -> Option<usize> {
    usize::try_from(v).ok()
}

fn slice(bytes: &[u8], from: u64, to: u64) -> Result<&[u8], Error> {
    bytes
        .get(usize_of(from).ok_or(Error::Length)?..usize_of(to).ok_or(Error::Length)?)
        .ok_or(Error::Length)
}

/// Where the head's parts are, once the header is checked.
struct Head {
    manifest_off: u64,
    manifest_end: u64,
    signature_off: Option<u64>,
    /// The end of the head: the manifest's end, or the signature's.
    end: u64,
}

/// The length of a pack's footer.
pub const FOOTER_SIZE: usize = crate::layout::FOOTER_BYTES;

/// The footer of the pack whose header starts `head`: the magic, the file length and the head
/// hash, all copied from the header. A pack installed with only some sections (D-136) lacks
/// the last frame that carries the footer, and writes this one instead; [`Pack::open`] then
/// checks it against the head digest as for any pack, so a forged header gains nothing.
///
/// # Errors
/// [`Error::Truncated`] if `head` is shorter than a header; [`Error::Magic`] if it is not one.
pub fn footer_of(head: &[u8]) -> Result<[u8; FOOTER_SIZE], Error> {
    use zerocopy::IntoBytes;
    let (header, _) = Header::ref_from_prefix(head).map_err(|_| Error::Truncated)?;
    if header.magic != MAGIC {
        return Err(Error::Magic);
    }
    let footer = Footer {
        magic: MAGIC,
        file_len: header.file_len,
        head_hash: header.head_hash,
    };
    let mut out = [0; FOOTER_SIZE];
    out.copy_from_slice(footer.as_bytes());
    Ok(out)
}

/// Checks the header against the footer and the real length, and the head's layout: table,
/// manifest and signature contiguous, all within 16 KiB.
fn check_header(header: &Header, footer: &Footer, len: u64) -> Result<Head, Error> {
    if header.magic != MAGIC || footer.magic != MAGIC {
        return Err(Error::Magic);
    }
    let min_reader = header.min_reader_major.get();
    if min_reader > READER_MAJOR {
        return Err(Error::Version {
            min_reader_major: min_reader,
        });
    }
    if min_reader > header.format_major.get() {
        return Err(Error::Header(
            "the minimum reader version is above the format version",
        ));
    }
    if header.flags.get() != 0 || header.reserved != [0; 20] {
        return Err(Error::Header("reserved flags or bytes are not zero"));
    }
    if header.file_len.get() != len || footer.file_len.get() != len {
        return Err(Error::Length);
    }
    if header.table_off.get() != HEADER_LEN {
        return Err(Error::Header(
            "the section table does not follow the header",
        ));
    }
    // At most 2^32 entries of 80 bytes: no overflow.
    let manifest_off = HEADER_LEN + u64::from(header.section_count.get()) * ENTRY_LEN;
    if header.manifest_off.get() != manifest_off {
        return Err(Error::Header(
            "the manifest does not follow the section table",
        ));
    }
    let manifest_end = manifest_off + u64::from(header.manifest_len.get());
    let (signature_off, end) = match (header.signature_off.get(), header.signature_len.get()) {
        (0, 0) => (None, manifest_end),
        (off, SIGNATURE_LEN) if off == manifest_end => {
            (Some(off), manifest_end + u64::from(SIGNATURE_LEN))
        }
        _ => {
            return Err(Error::Header(
                "the signature is not 72 bytes right after the manifest",
            ));
        }
    };
    if end > HEAD_MAX || end > len - FOOTER_LEN {
        return Err(Error::Header(
            "the head is longer than 16 KiB or than the file",
        ));
    }
    Ok(Head {
        manifest_off,
        manifest_end,
        signature_off,
        end,
    })
}

/// Checks every table entry: kinds strictly increasing, each section at the next 64-byte
/// boundary after the previous one with zero padding, and the footer right after the last.
fn check_sections(
    bytes: &[u8],
    table: &[Entry],
    head_end: u64,
    footer_start: u64,
) -> Result<(), Error> {
    let mut pos = head_end;
    let mut prev_kind = 0;
    for (i, e) in table.iter().enumerate() {
        let err = |problem| Error::Section {
            index: u32::try_from(i).unwrap_or(u32::MAX),
            problem,
        };
        let kind = e.kind.get();
        if kind == 0 || kind <= prev_kind {
            return Err(err("kinds are not strictly increasing from 1"));
        }
        if e.flags.get() != 0 || e.reserved != [0; 8] {
            return Err(err("reserved flags or bytes are not zero"));
        }
        let off = e.offset.get();
        if Some(off) != align_up(pos) {
            return Err(err("does not start at the next 64-byte boundary"));
        }
        let end = off
            .checked_add(e.length.get())
            .filter(|&end| end <= footer_start)
            .ok_or_else(|| err("runs past the footer"))?;
        if slice(bytes, pos, off)?.iter().any(|&b| b != 0) {
            return Err(err("the padding before it is not zero"));
        }
        if e.encoding.get() == Encoding::RAW.0 && e.decoded_length.get() != e.length.get() {
            return Err(err(
                "a raw section's decoded length differs from its length",
            ));
        }
        prev_kind = kind;
        pos = end;
    }
    if pos == footer_start {
        Ok(())
    } else {
        Err(Error::Length)
    }
}

impl<'a> Pack<'a> {
    /// Opens a pack: checks the magic, versions, lengths, the head hash, the layout of every
    /// section and the manifest. Section bytes are hashed by [`Pack::verify_sections`] or
    /// [`Section::verify`], not here, so opening a large pack costs only its head.
    ///
    /// The checks make the encoding canonical: a pack is accepted only if it is exactly what
    /// the writer produces for its content.
    ///
    /// # Errors
    /// The first problem found, as an [`Error`].
    pub fn open(bytes: &'a [u8]) -> Result<Self, Error> {
        let pack = Self::open_head(bytes)?;
        crate::blocks::check(pack.section(Kind::BLOCK_HASHES), pack.sections())?;
        Ok(pack)
    }

    /// Opens a pack installed with only some of its sections (D-136): the checks of
    /// [`Pack::open`] that need only the head and the footer. It reads no section's bytes, so
    /// the sections not installed may be holes; the `BLOCK_HASHES` list is checked when a
    /// section asks for its blocks ([`Pack::blocks`], which proves them against the signed
    /// section hash). The footer may be rebuilt with [`footer_of`].
    ///
    /// # Errors
    /// As [`Pack::open`], but for the block-hash list's structure.
    pub fn open_head(bytes: &'a [u8]) -> Result<Self, Error> {
        let len = u64::try_from(bytes.len()).map_err(|_| Error::Length)?;
        if len < HEADER_LEN + FOOTER_LEN {
            return Err(Error::Truncated);
        }
        let (header, _) = Header::ref_from_prefix(bytes).map_err(|_| Error::Truncated)?;
        let (_, footer) = Footer::ref_from_suffix(bytes).map_err(|_| Error::Truncated)?;
        let head = check_header(header, footer, len)?;

        let rest = slice(bytes, HEADER_LEN, len)?;
        let count = usize_of(u64::from(header.section_count.get())).ok_or(Error::Length)?;
        let (table, _) = <[Entry]>::ref_from_prefix_with_elems(rest, count)
            .map_err(|_| Error::Header("the section table is truncated"))?;
        let manifest_bytes = slice(bytes, head.manifest_off, head.manifest_end)?;
        let digest = head_digest(
            slice(bytes, 0, HEADER_HASHED as u64)?,
            slice(bytes, HEADER_LEN, head.manifest_off)?,
            manifest_bytes,
        );
        if digest.get(..16) != Some(&header.head_hash[..]) || footer.head_hash != header.head_hash {
            return Err(Error::HeadHash);
        }
        let manifest = Manifest::parse(manifest_bytes)?;
        if pack_uuid(manifest.address(), manifest.component(), manifest.release())
            != header.pack_uuid
        {
            return Err(Error::Header("the pack UUID does not match its identity"));
        }
        let signature = match head.signature_off {
            Some(off) => Some(slice(bytes, off, off + u64::from(SIGNATURE_LEN))?),
            None => None,
        };
        check_sections(bytes, table, head.end, len - FOOTER_LEN)?;
        Ok(Self {
            bytes,
            header,
            table,
            manifest,
            digest,
            signature,
        })
    }

    /// Section `kind`'s block hashes, proven against its hash (B12): O(blocks) merges, no
    /// hashing of the section; then [`Blocks::verify`](crate::blocks::Blocks::verify) checks
    /// a block on first read. `None` if there is no such section or it is under 1 MiB (verify
    /// it whole with [`Section::verify`]).
    ///
    /// # Errors
    /// [`Error::SectionHash`] if the block hashes do not give the section hash.
    pub fn blocks(&self, kind: Kind) -> Result<Option<crate::blocks::Blocks<'a, 'static>>, Error> {
        let blocks = self.locate_blocks(kind)?;
        if let Some(b) = &blocks {
            b.prove()?;
        }
        Ok(blocks)
    }

    /// Section `kind`'s block hashes, found but **not proven**: O(entries). For a reader that
    /// defers the proof to the section's first use; call
    /// [`Blocks::prove`](crate::blocks::Blocks::prove) before trusting a block.
    ///
    /// # Errors
    /// [`Error::Payload`] if `BLOCK_HASHES` is missing or has no entry for a covered section.
    pub fn locate_blocks(
        &self,
        kind: Kind,
    ) -> Result<Option<crate::blocks::Blocks<'a, 'static>>, Error> {
        match self.section(kind) {
            Some(s) => crate::blocks::Blocks::locate(s, self.section(Kind::BLOCK_HASHES)),
            None => Ok(None),
        }
    }

    /// Opens a pack and hashes every section: [`Pack::open`] then [`Pack::verify_sections`].
    ///
    /// # Errors
    /// As [`Pack::open`], or [`Error::SectionHash`].
    pub fn open_verified(bytes: &'a [u8]) -> Result<Self, Error> {
        let pack = Self::open(bytes)?;
        pack.verify_sections()?;
        Ok(pack)
    }

    /// Hashes every section and compares it with the table.
    ///
    /// # Errors
    /// [`Error::SectionHash`] for the first section that differs.
    pub fn verify_sections(&self) -> Result<(), Error> {
        self.sections().try_for_each(|s| s.verify())
    }

    fn section_at(&self, e: &'a Entry) -> Section<'a> {
        // `open` checked that every section lies inside the file.
        let start = usize_of(e.offset.get()).unwrap_or(usize::MAX);
        let end = usize_of(e.offset.get().saturating_add(e.length.get())).unwrap_or(usize::MAX);
        Section {
            kind: Kind(e.kind.get()),
            encoding: Encoding(e.encoding.get()),
            item_count: e.item_count.get(),
            decoded_length: e.decoded_length.get(),
            hash: &e.blake3,
            bytes: self.bytes.get(start..end).unwrap_or_default(),
        }
    }

    /// Every section, in kind order.
    pub fn sections(&self) -> impl ExactSizeIterator<Item = Section<'a>> + '_ {
        self.table.iter().map(|e| self.section_at(e))
    }

    /// The section of this kind, if the pack has one.
    #[must_use]
    pub fn section(&self, kind: Kind) -> Option<Section<'a>> {
        self.table
            .binary_search_by_key(&kind.0, |e| e.kind.get())
            .ok()
            .and_then(|i| self.table.get(i))
            .map(|e| self.section_at(e))
    }

    /// The manifest.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest<'a> {
        &self.manifest
    }

    /// `(major, minor)` of the format the pack was written in.
    #[must_use]
    pub fn format_version(&self) -> (u16, u16) {
        (
            self.header.format_major.get(),
            self.header.format_minor.get(),
        )
    }

    /// The oldest reader major version that can read the pack.
    #[must_use]
    pub fn min_reader_major(&self) -> u16 {
        self.header.min_reader_major.get()
    }

    /// The pack UUID, derived from its address, component and release.
    #[must_use]
    pub fn uuid(&self) -> [u8; 16] {
        self.header.pack_uuid
    }

    /// The build ID: a hash of the builder version and the pinned sources.
    #[must_use]
    pub fn build_id(&self) -> u64 {
        self.header.build_id.get()
    }

    /// When the pack says it was built (`SOURCE_DATE_EPOCH`), in Unix seconds.
    #[must_use]
    pub fn created_unix(&self) -> u64 {
        self.header.created_unix.get()
    }

    /// The head digest: BLAKE3 over the header, the section table and the manifest. Covers every
    /// section through the table's hashes; this is what a signature signs.
    #[must_use]
    pub const fn head_digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// The signature block (key ID and signature), if the pack is signed.
    #[must_use]
    pub fn signature(&self) -> Option<([u8; 8], [u8; 64])> {
        let s = self.signature?;
        Some((s.get(..8)?.try_into().ok()?, s.get(8..72)?.try_into().ok()?))
    }

    /// The pack's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}
