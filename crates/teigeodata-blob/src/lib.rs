//! The columnar result blob of teigeodata, format v0
//! ([bindings spike S4](https://github.com/teispace/teigeodata/blob/main/docs/02-research/notes/bindings-spike-s4.md)).
//!
//! A result crosses every language boundary as one little-endian buffer of typed columns. Each
//! column starts on an 8-byte boundary from the start of the blob, so a binding can view it in
//! place (a `Float64Array`, a `memoryview`, a Dart `Float64List`) without parsing rows.
//!
//! | Offset | Field |
//! |---|---|
//! | 0 | magic `TGB0` |
//! | 4 | `u32` row count |
//! | 8 | `u32` column count |
//! | 12 | `u32` total length in bytes |
//! | 16 | column descriptors, [`DESCRIPTOR`] bytes each |
//! | … | column data, each starting on an 8-byte boundary |
//!
//! A descriptor is a [`NAME_MAX`]-byte NUL-padded UTF-8 name, then six `u32`s: the type, the
//! data offset and the data length, for UTF-8 columns the offset and length of the string
//! bytes (zero otherwise), and a reserved field that is zero in v0 (it keeps descriptors a
//! multiple of 8 bytes). A UTF-8 column's data is `rows + 1` `u32` offsets into its string
//! bytes, as in Arrow. Offsets are from the start of the blob.
//!
//! ```
//! use teigeodata_blob::{Blob, Column, Reader, View};
//!
//! let blob = Blob::encode(&[
//!     ("id", Column::U32(&[1, 2])),
//!     ("name", Column::Utf8(&["Kathmandu", "Pokhara"])),
//! ])?;
//! let reader = Reader::new(blob.as_bytes())?;
//! assert_eq!(reader.rows(), 2);
//! let Some(View::Utf8(names)) = reader.column("name") else { unreachable!() };
//! assert_eq!(names.get(1), Some("Pokhara"));
//! # Ok::<(), teigeodata_blob::Error>(())
//! ```
#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// The magic bytes of format v0.
pub const MAGIC: [u8; 4] = *b"TGB0";
/// Header bytes before the descriptors.
pub const HEADER: usize = 16;
/// Bytes per column descriptor.
pub const DESCRIPTOR: usize = NAME_MAX + 6 * 4;
/// Longest column name in bytes.
pub const NAME_MAX: usize = 16;
/// Alignment of every column's data, from the start of the blob.
pub const ALIGN: usize = 8;

/// Column types, as stored in a descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Type {
    /// `i32` values.
    I32 = 1,
    /// `u32` values.
    U32 = 2,
    /// `i64` values.
    I64 = 3,
    /// `u64` values.
    U64 = 4,
    /// `f32` values.
    F32 = 5,
    /// `f64` values.
    F64 = 6,
    /// UTF-8 strings: `rows + 1` `u32` offsets, then the bytes.
    Utf8 = 7,
}

impl Type {
    const ALL: [Self; 7] = [
        Self::I32,
        Self::U32,
        Self::I64,
        Self::U64,
        Self::F32,
        Self::F64,
        Self::Utf8,
    ];

    fn from_u32(v: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|t| *t as u32 == v)
    }

    /// Bytes per value in the data section (a UTF-8 column stores `u32` offsets).
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::I32 | Self::U32 | Self::F32 | Self::Utf8 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }
}

/// A column handed to the encoder.
#[derive(Debug, Clone, Copy)]
pub enum Column<'a> {
    /// `i32` values.
    I32(&'a [i32]),
    /// `u32` values.
    U32(&'a [u32]),
    /// `i64` values.
    I64(&'a [i64]),
    /// `u64` values.
    U64(&'a [u64]),
    /// `f32` values.
    F32(&'a [f32]),
    /// `f64` values.
    F64(&'a [f64]),
    /// Strings.
    Utf8(&'a [&'a str]),
}

impl Column<'_> {
    fn ty(&self) -> Type {
        match self {
            Self::I32(_) => Type::I32,
            Self::U32(_) => Type::U32,
            Self::I64(_) => Type::I64,
            Self::U64(_) => Type::U64,
            Self::F32(_) => Type::F32,
            Self::F64(_) => Type::F64,
            Self::Utf8(_) => Type::Utf8,
        }
    }

    fn rows(&self) -> usize {
        match self {
            Self::I32(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
            Self::Utf8(v) => v.len(),
        }
    }

    /// Length of the string bytes (zero for fixed-width columns).
    fn string_bytes(&self) -> usize {
        match self {
            Self::Utf8(v) => v.iter().map(|s| s.len()).sum(),
            _ => 0,
        }
    }

    /// Length of the data section in bytes.
    fn data_len(&self) -> usize {
        let rows = self.rows();
        match self {
            Self::Utf8(_) => (rows + 1) * 4,
            _ => rows * self.ty().width(),
        }
    }

    /// Appends the data section (and the string bytes for UTF-8) at the end of `out`.
    fn write_data(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            Self::I32(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Self::U32(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Self::I64(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Self::U64(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Self::F32(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Self::F64(v) => v
                .iter()
                .for_each(|x| out.extend_from_slice(&x.to_le_bytes())),
            Self::Utf8(v) => {
                let mut at = 0u32;
                out.extend_from_slice(&at.to_le_bytes());
                for s in *v {
                    at = at
                        .checked_add(u32::try_from(s.len()).map_err(|_| Error::TooLarge)?)
                        .ok_or(Error::TooLarge)?;
                    out.extend_from_slice(&at.to_le_bytes());
                }
            }
        }
        Ok(())
    }
}

/// Why a blob could not be built or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No columns, or columns with different row counts.
    Shape,
    /// A column name is empty, longer than [`NAME_MAX`] bytes, contains NUL, or is repeated.
    Name,
    /// The blob would be 4 GiB or larger, beyond the `u32` offsets.
    TooLarge,
    /// The bytes are not a valid v0 blob.
    Malformed,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Shape => "columns are missing or have different row counts",
            Self::Name => "a column name is empty, too long, has NUL or is repeated",
            Self::TooLarge => "the blob would exceed 4 GiB",
            Self::Malformed => "the bytes are not a valid teigeodata blob",
        })
    }
}

impl core::error::Error for Error {}

/// An encoded blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob(Vec<u8>);

fn pad(n: usize) -> usize {
    n.next_multiple_of(ALIGN)
}

fn put_u32(out: &mut Vec<u8>, v: usize) -> Result<(), Error> {
    out.extend_from_slice(&u32::try_from(v).map_err(|_| Error::TooLarge)?.to_le_bytes());
    Ok(())
}

impl Blob {
    /// Encodes named columns. Every column must have the same number of rows. The output is
    /// allocated once at its exact size.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`], [`Error::Name`] or [`Error::TooLarge`].
    pub fn encode(columns: &[(&str, Column<'_>)]) -> Result<Self, Error> {
        let rows = columns.first().ok_or(Error::Shape)?.1.rows();
        for (i, (name, col)) in columns.iter().enumerate() {
            if col.rows() != rows {
                return Err(Error::Shape);
            }
            let bad = name.is_empty() || name.len() > NAME_MAX || name.contains('\0');
            if bad || columns.iter().take(i).any(|(n, _)| n == name) {
                return Err(Error::Name);
            }
        }
        // Layout first, so the buffer is allocated once and every offset is known.
        let mut at = pad(HEADER + columns.len() * DESCRIPTOR);
        let mut layout = Vec::with_capacity(columns.len());
        for (_, col) in columns {
            let data = at;
            at = pad(at + col.data_len());
            let strings = at;
            at = pad(at + col.string_bytes());
            layout.push((data, strings));
        }
        let total = at;
        if u32::try_from(total).is_err() {
            return Err(Error::TooLarge);
        }
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&MAGIC);
        put_u32(&mut out, rows)?;
        put_u32(&mut out, columns.len())?;
        put_u32(&mut out, total)?;
        for ((name, col), (data, strings)) in columns.iter().zip(&layout) {
            let mut padded = [0u8; NAME_MAX];
            for (slot, b) in padded.iter_mut().zip(name.bytes()) {
                *slot = b;
            }
            out.extend_from_slice(&padded);
            out.extend_from_slice(&(col.ty() as u32).to_le_bytes());
            put_u32(&mut out, *data)?;
            put_u32(&mut out, col.data_len())?;
            let utf8 = matches!(col, Column::Utf8(_));
            put_u32(&mut out, if utf8 { *strings } else { 0 })?;
            put_u32(&mut out, col.string_bytes())?;
            put_u32(&mut out, 0)?;
        }
        for (col, (data, strings)) in columns.iter().map(|(_, c)| c).zip(&layout) {
            out.resize(*data, 0);
            col.write_data(&mut out)?;
            out.resize(*strings, 0);
            if let Column::Utf8(v) = col {
                v.iter().for_each(|s| out.extend_from_slice(s.as_bytes()));
            }
        }
        out.resize(total, 0);
        Ok(Self(out))
    }

    /// The encoded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The encoded bytes, owned, for a binding that hands them to its runtime.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// A validated view of a blob. Construction checks every descriptor, so reads never fail.
#[derive(Debug, Clone, Copy)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    rows: usize,
    columns: usize,
}

/// One column of a [`Reader`].
#[derive(Debug, Clone, Copy)]
pub enum View<'a> {
    /// `i32` values.
    I32(Fixed<'a, 4>),
    /// `u32` values.
    U32(Fixed<'a, 4>),
    /// `i64` values.
    I64(Fixed<'a, 8>),
    /// `u64` values.
    U64(Fixed<'a, 8>),
    /// `f32` values.
    F32(Fixed<'a, 4>),
    /// `f64` values.
    F64(Fixed<'a, 8>),
    /// Strings.
    Utf8(Strings<'a>),
}

/// The little-endian values of a fixed-width column, `N` bytes each.
#[derive(Debug, Clone, Copy)]
pub struct Fixed<'a, const N: usize>(&'a [[u8; N]]);

impl<'a, const N: usize> Fixed<'a, N> {
    /// The raw little-endian values.
    #[must_use]
    pub fn raw(&self) -> &'a [[u8; N]] {
        self.0
    }
}

macro_rules! values {
    ($n:literal, $($name:ident: $t:ty),+) => {
        impl<'a> Fixed<'a, $n> {
            $(
                #[doc = concat!("The values as `", stringify!($t), "`.")]
                pub fn $name(&self) -> impl ExactSizeIterator<Item = $t> + 'a {
                    self.0.iter().map(|b| <$t>::from_le_bytes(*b))
                }
            )+
        }
    };
}
values!(4, i32s: i32, u32s: u32, f32s: f32);
values!(8, i64s: i64, u64s: u64, f64s: f64);

/// The strings of a UTF-8 column, validated when the reader was built.
#[derive(Debug, Clone, Copy)]
pub struct Strings<'a> {
    offsets: &'a [[u8; 4]],
    bytes: &'a str,
}

impl<'a> Strings<'a> {
    /// The string in row `i`.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&'a str> {
        let at = |k: usize| self.offsets.get(k).map(|b| u32::from_le_bytes(*b) as usize);
        self.bytes.get(at(i)?..at(i + 1)?)
    }

    /// Every string in row order.
    pub fn iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        (0..self.offsets.len().saturating_sub(1)).filter_map(|i| self.get(i))
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(b) as usize)
}

/// A checked byte range of the blob, 8-byte aligned and after the descriptors.
fn region(bytes: &[u8], at: usize, len: usize, floor: usize) -> Option<&[u8]> {
    (at.is_multiple_of(ALIGN) && at >= floor).then_some(())?;
    bytes.get(at..at.checked_add(len)?)
}

impl<'a> Reader<'a> {
    /// Validates `bytes` as a v0 blob: header, every descriptor, every range and every string.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] if anything is out of bounds, misaligned, of the wrong length, not
    /// UTF-8, or if the offsets of a string column are not monotonic char boundaries.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        let check = || -> Option<Self> {
            (bytes.get(..4)? == MAGIC).then_some(())?;
            let rows = u32_at(bytes, 4)?;
            let columns = u32_at(bytes, 8)?;
            (u32_at(bytes, 12)? == bytes.len()).then_some(())?;
            let floor = HEADER.checked_add(columns.checked_mul(DESCRIPTOR)?)?;
            (floor <= bytes.len()).then_some(())?;
            let reader = Self {
                bytes,
                rows,
                columns,
            };
            for i in 0..columns {
                reader.view(i, floor)?;
                let name = reader.name(i)?;
                (0..i).all(|j| reader.name(j) != Some(name)).then_some(())?;
            }
            Some(reader)
        };
        check().ok_or(Error::Malformed)
    }

    /// Number of rows.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Column names in order.
    pub fn names(&self) -> impl Iterator<Item = &'a str> + '_ {
        (0..self.columns).filter_map(|i| self.name(i))
    }

    /// The column called `name`.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<View<'a>> {
        let i = (0..self.columns).find(|&i| self.name(i) == Some(name))?;
        self.view(i, HEADER + self.columns * DESCRIPTOR)
    }

    fn descriptor(&self, i: usize) -> Option<&'a [u8]> {
        let at = HEADER + i * DESCRIPTOR;
        self.bytes.get(at..at + DESCRIPTOR)
    }

    fn name(&self, i: usize) -> Option<&'a str> {
        let raw = self.descriptor(i)?.get(..NAME_MAX)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(NAME_MAX);
        let (name, rest) = raw.split_at(end);
        let ok = end > 0 && rest.iter().all(|&b| b == 0);
        ok.then(|| core::str::from_utf8(name).ok()).flatten()
    }

    fn view(&self, i: usize, floor: usize) -> Option<View<'a>> {
        let d = self.descriptor(i)?;
        let field = |k: usize| u32_at(d, NAME_MAX + 4 * k);
        let ty = Type::from_u32(u32::try_from(field(0)?).ok()?)?;
        let (at, len, s_at, s_len) = (field(1)?, field(2)?, field(3)?, field(4)?);
        (field(5)? == 0).then_some(())?;
        let values = if ty == Type::Utf8 {
            self.rows.checked_add(1)?
        } else {
            (s_at == 0 && s_len == 0).then_some(())?;
            self.rows
        };
        (len == values.checked_mul(ty.width())?).then_some(())?;
        let data = region(self.bytes, at, len, floor)?;
        Some(match ty {
            Type::I32 => View::I32(Fixed(data.as_chunks().0)),
            Type::U32 => View::U32(Fixed(data.as_chunks().0)),
            Type::I64 => View::I64(Fixed(data.as_chunks().0)),
            Type::U64 => View::U64(Fixed(data.as_chunks().0)),
            Type::F32 => View::F32(Fixed(data.as_chunks().0)),
            Type::F64 => View::F64(Fixed(data.as_chunks().0)),
            Type::Utf8 => {
                let bytes = core::str::from_utf8(region(self.bytes, s_at, s_len, floor)?).ok()?;
                let offsets: &[[u8; 4]] = data.as_chunks().0;
                let mut prev = 0;
                for (k, o) in offsets.iter().enumerate() {
                    let o = u32::from_le_bytes(*o) as usize;
                    let ok = (k > 0 || o == 0) && o >= prev && bytes.is_char_boundary(o);
                    ok.then_some(())?;
                    prev = o;
                }
                (prev == bytes.len()).then_some(())?;
                View::Utf8(Strings { offsets, bytes })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ALIGN, Blob, Column, DESCRIPTOR, Error, HEADER, NAME_MAX, Reader, View};
    use alloc::vec;
    use alloc::vec::Vec;

    fn sample() -> Blob {
        Blob::encode(&[
            ("id", Column::U32(&[7, 8, 9])),
            ("lat", Column::F64(&[27.7, -33.9, 0.0])),
            ("elev", Column::F32(&[1400.5, 3.0, -1.0])),
            ("pop", Column::I64(&[i64::MIN, 0, i64::MAX])),
            ("name", Column::Utf8(&["काठमाडौं", "", "Zürich"])),
            ("d", Column::I32(&[-1, 0, 1])),
            ("u", Column::U64(&[u64::MAX, 1, 2])),
        ])
        .unwrap_or_else(|_| Blob(Vec::new()))
    }

    /// The values of a fixed-width column, or `None` if it is missing or of another type.
    macro_rules! values {
        ($reader:expr, $name:literal, $variant:ident, $get:ident) => {
            match $reader.column($name) {
                Some(View::$variant(c)) => Some(c.$get().collect::<Vec<_>>()),
                _ => None,
            }
        };
    }

    #[test]
    fn round_trips_every_type() {
        let blob = sample();
        let reader = Reader::new(blob.as_bytes()).unwrap();
        assert_eq!(reader.rows(), 3);
        assert_eq!(
            reader.names().collect::<Vec<_>>(),
            ["id", "lat", "elev", "pop", "name", "d", "u"]
        );
        assert_eq!(values!(reader, "id", U32, u32s), Some(vec![7, 8, 9]));
        assert_eq!(
            values!(reader, "lat", F64, f64s),
            Some(vec![27.7, -33.9, 0.0])
        );
        assert_eq!(
            values!(reader, "elev", F32, f32s),
            Some(vec![1400.5, 3.0, -1.0])
        );
        let pop = values!(reader, "pop", I64, i64s);
        assert_eq!(pop, Some(vec![i64::MIN, 0, i64::MAX]));
        assert_eq!(values!(reader, "d", I32, i32s), Some(vec![-1, 0, 1]));
        assert_eq!(values!(reader, "u", U64, u64s), Some(vec![u64::MAX, 1, 2]));
        assert_eq!(values!(reader, "id", F32, f32s), None);
        let Some(View::Utf8(names)) = reader.column("name") else {
            panic!("name")
        };
        assert_eq!(names.iter().collect::<Vec<_>>(), ["काठमाडौं", "", "Zürich"]);
        assert_eq!(names.get(3), None);
        assert!(reader.column("missing").is_none());
    }

    #[test]
    fn columns_are_aligned_and_length_is_exact() {
        let blob = sample();
        let b = blob.as_bytes();
        assert_eq!(b.len() % ALIGN, 0);
        for i in 0..7 {
            let d = HEADER + i * DESCRIPTOR + NAME_MAX;
            let at = u32::from_le_bytes(b[d + 4..d + 8].try_into().unwrap());
            assert_eq!(at % 8, 0);
        }
        assert_eq!(blob.clone().into_vec().capacity(), b.len());
    }

    #[test]
    fn empty_columns_encode() {
        let blob = Blob::encode(&[("a", Column::F64(&[])), ("s", Column::Utf8(&[]))]);
        let r = blob
            .as_ref()
            .map(|b| Reader::new(b.as_bytes()).map(|r| r.rows()));
        assert_eq!(r, Ok(Ok(0)));
    }

    #[test]
    fn rejects_bad_input_to_the_encoder() {
        assert_eq!(Blob::encode(&[]), Err(Error::Shape));
        let ragged = [("a", Column::U32(&[1])), ("b", Column::U32(&[1, 2]))];
        assert_eq!(Blob::encode(&ragged), Err(Error::Shape));
        for name in ["", "seventeen-bytes!!", "a\0b"] {
            assert_eq!(Blob::encode(&[(name, Column::U32(&[]))]), Err(Error::Name));
        }
        let twice = [("a", Column::U32(&[])), ("a", Column::F32(&[]))];
        assert_eq!(Blob::encode(&twice), Err(Error::Name));
    }

    /// Every single-byte corruption and every truncation is either rejected or still reads
    /// back as a valid blob: the reader never panics or reads out of bounds.
    #[test]
    fn reader_survives_every_corruption() {
        let good = sample().into_vec();
        for i in 0..good.len() {
            for flip in [0x01u8, 0x80, 0xff] {
                let mut bad = good.clone();
                if let Some(b) = bad.get_mut(i) {
                    *b ^= flip;
                }
                if let Ok(r) = Reader::new(&bad) {
                    for name in r.names() {
                        if let Some(View::Utf8(s)) = r.column(name) {
                            assert_eq!(s.iter().count(), r.rows());
                        }
                    }
                }
            }
            assert!(Reader::new(good.get(..i).unwrap_or_default()).is_err());
        }
    }

    #[test]
    fn rejects_hand_made_faults() {
        let good = sample().into_vec();
        let set = |at: usize, v: u32| {
            let mut b = good.clone();
            if let Some(s) = b.get_mut(at..at + 4) {
                s.copy_from_slice(&v.to_le_bytes());
            }
            b
        };
        let name_desc = HEADER + 4 * DESCRIPTOR + NAME_MAX;
        let cases = [
            set(0, 0),                     // magic
            set(4, 4),                     // row count
            set(8, 1000),                  // column count
            set(12, 8),                    // total length
            set(HEADER + NAME_MAX, 99),    // unknown type
            set(HEADER + NAME_MAX + 4, 3), // misaligned data
            set(name_desc + 12, 1),        // misaligned string bytes
        ];
        for (k, bad) in cases.iter().enumerate() {
            assert!(Reader::new(bad).is_err(), "case {k}");
        }
        assert!(Reader::new(&good).is_ok());
    }
}
