//! The columnar result blob of teigeodata, format v1
//! ([bindings §3](https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/bindings.md),
//! spike S2, D-275).
//!
//! A result crosses every language boundary as one little-endian buffer of typed columns. Each
//! region starts on an 8-byte boundary from the start of the blob, so a binding can view a column
//! in place (a `Float64Array`, a `memoryview`, a Dart `Float64List`) without parsing rows.
//!
//! | Offset | Field |
//! |---|---|
//! | 0 | magic `TGB1` (a v0 blob, `TGB0`, is read too: it is v1 without the new parts) |
//! | 4 | `u32` row count |
//! | 8 | `u32` column count |
//! | 12 | `u32` total length in bytes |
//! | 16 | column descriptors, [`DESCRIPTOR`] bytes each |
//! | … | column regions, each starting on an 8-byte boundary |
//!
//! A descriptor is a [`NAME_MAX`]-byte NUL-padded UTF-8 name, then six `u32`s: the [`Type`], the
//! data offset and length, the offset and length of an auxiliary region (zero when the type has
//! none), and the offset of the null bitmap (zero when no row is null). Offsets are from the start
//! of the blob.
//!
//! - **Fixed-width types** store one value per row.
//! - **`Utf8`** stores `rows + 1` `u32` offsets into its string bytes, the auxiliary region, as in
//!   Arrow.
//! - **`Dict16` and `Dict32`** store one `u16` or `u32` code per row into a dictionary, the
//!   auxiliary region: a `u32` count, `count + 1` `u32` offsets, then the string bytes. A
//!   repeated string (a timezone, a country) is stored and decoded once.
//! - **The null bitmap** holds one bit per row, least significant bit first, 1 for a value
//!   (Arrow's convention); bits past the last row are 0. A null row's value is zero, its string
//!   empty, its code 0.
//!
//! Readers look columns up by name: a new column never breaks an old reader, and a missing one
//! is absent. Only a change of layout changes the magic.
//!
//! ```
//! use teigeodata_blob::{Blob, Column, Reader, View};
//!
//! let blob = Blob::encode(&[
//!     ("id", Column::U32(&[1, 2])),
//!     ("name", Column::Utf8(&["Kathmandu", "Pokhara"])),
//!     ("tz", Column::Dict(&["Asia/Kathmandu", "Asia/Kathmandu"])),
//!     ("elevation", Column::Nullable(&Column::I16(&[1400, 0]), &[true, false])),
//! ])?;
//! let reader = Reader::new(blob.as_bytes())?;
//! assert_eq!(reader.rows(), 2);
//! let Some(View::Utf8(names)) = reader.column("name") else { unreachable!() };
//! assert_eq!(names.get(1), Some("Pokhara"));
//! let Some(View::Dict(tz)) = reader.column("tz") else { unreachable!() };
//! assert_eq!((tz.get(1), tz.dictionary().len()), (Some("Asia/Kathmandu"), 1));
//! let nulls = reader.nulls("elevation").expect("one row is null");
//! assert!(!nulls.is_null(0) && nulls.is_null(1));
//! # Ok::<(), teigeodata_blob::Error>(())
//! ```
#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// The magic bytes of format v1.
pub const MAGIC: [u8; 4] = *b"TGB1";
/// The magic bytes of format v0, still read.
pub const MAGIC_V0: [u8; 4] = *b"TGB0";
/// Header bytes before the descriptors.
pub const HEADER: usize = 16;
/// Bytes per column descriptor.
pub const DESCRIPTOR: usize = NAME_MAX + 6 * 4;
/// Longest column name in bytes.
pub const NAME_MAX: usize = 16;
/// Alignment of every region, from the start of the blob.
pub const ALIGN: usize = 8;
/// The most distinct strings a `Dict16` column holds; more take `Dict32`.
pub const DICT16_MAX: usize = 1 << 16;

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
    /// `u8` values (v1).
    U8 = 8,
    /// `i16` values (v1).
    I16 = 9,
    /// `u16` values (v1).
    U16 = 10,
    /// `u16` codes into a dictionary of strings (v1).
    Dict16 = 11,
    /// `u32` codes into a dictionary of strings (v1).
    Dict32 = 12,
}

impl Type {
    const ALL: [Self; 12] = [
        Self::I32,
        Self::U32,
        Self::I64,
        Self::U64,
        Self::F32,
        Self::F64,
        Self::Utf8,
        Self::U8,
        Self::I16,
        Self::U16,
        Self::Dict16,
        Self::Dict32,
    ];

    fn from_u32(v: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|t| *t as u32 == v)
    }

    /// Bytes per value in the data region (a UTF-8 column stores `u32` offsets, a dictionary
    /// column its codes).
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16 | Self::U16 | Self::Dict16 => 2,
            Self::I32 | Self::U32 | Self::F32 | Self::Utf8 | Self::Dict32 => 4,
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
    /// Strings, stored as they are.
    Utf8(&'a [&'a str]),
    /// `u8` values.
    U8(&'a [u8]),
    /// `i16` values.
    I16(&'a [i16]),
    /// `u16` values.
    U16(&'a [u16]),
    /// Strings stored once each, with a code per row: for columns that repeat (timezones,
    /// countries). Encoded as `Dict16` or `Dict32` by the number of distinct strings.
    Dict(&'a [&'a str]),
    /// A column with nulls: `true` where a row has a value. Nesting is an [`Error::Shape`].
    Nullable(&'a Column<'a>, &'a [bool]),
}

impl Column<'_> {
    fn rows(&self) -> usize {
        match self {
            Self::I32(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
            Self::Utf8(v) | Self::Dict(v) => v.len(),
            Self::U8(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::Nullable(c, _) => c.rows(),
        }
    }
}

/// Why a blob could not be built or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No columns, columns with different row counts, a null mask of the wrong length, or a
    /// nested [`Column::Nullable`].
    Shape,
    /// A column name is empty, longer than [`NAME_MAX`] bytes, contains NUL, or is repeated.
    Name,
    /// The blob would be 4 GiB or larger, beyond the `u32` offsets.
    TooLarge,
    /// The bytes are not a valid blob.
    Malformed,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Shape => "columns are missing, have different row counts or a bad null mask",
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

fn to_u32(v: usize) -> Result<u32, Error> {
    u32::try_from(v).map_err(|_| Error::TooLarge)
}

/// One column, planned: its type, region sizes and, for a dictionary, the distinct strings.
struct Plan<'a> {
    ty: Type,
    column: Column<'a>,
    /// `Some` only when at least one row is null.
    present: Option<&'a [bool]>,
    /// Distinct strings in first-seen order and each row's code (dictionary columns).
    dict: Option<(Vec<&'a str>, Vec<u32>)>,
    aux_len: usize,
}

/// A word-at-a-time multiplicative hash (as rustc's `FxHash`): fast on short strings, and only
/// used to find repeats inside one result, so it needs no resistance to chosen input.
fn hash(s: &str) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let (words, tail) = s.as_bytes().as_chunks::<8>();
    let mut h = (s.len() as u64).wrapping_mul(K);
    for w in words {
        h = (h.rotate_left(5) ^ u64::from_le_bytes(*w)).wrapping_mul(K);
    }
    let mut last = [0u8; 8];
    for (d, b) in last.iter_mut().zip(tail) {
        *d = *b;
    }
    (h.rotate_left(5) ^ u64::from_le_bytes(last)).wrapping_mul(K)
}

/// The distinct strings of a column in first-seen order and each row's code (0 for a null
/// row), through an open-addressing table of at least twice the rows.
fn dictionary<'a>(
    v: &[&'a str],
    has: impl Fn(usize) -> bool,
) -> Result<(Vec<&'a str>, Vec<u32>), Error> {
    const EMPTY: u32 = u32::MAX;
    let size = (v.len() * 2).next_power_of_two().max(8);
    let mask = size - 1;
    let mut slots = alloc::vec![EMPTY; size];
    let mut strings: Vec<&'a str> = Vec::new();
    let mut codes = Vec::with_capacity(v.len());
    for (i, s) in v.iter().enumerate() {
        if !has(i) {
            codes.push(0);
            continue;
        }
        // The table's size bounds the probe; `as usize` keeps the hash's low bits.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the low bits pick the slot"
        )]
        let mut at = hash(s) as usize & mask;
        let code = loop {
            let slot = slots.get(at).copied().unwrap_or(EMPTY);
            if slot == EMPTY {
                let code = to_u32(strings.len())?;
                if let Some(x) = slots.get_mut(at) {
                    *x = code;
                }
                strings.push(s);
                break code;
            }
            if strings.get(slot as usize) == Some(s) {
                break slot;
            }
            at = (at + 1) & mask;
        };
        codes.push(code);
    }
    Ok((strings, codes))
}

impl<'a> Plan<'a> {
    fn new(column: Column<'a>, rows: usize) -> Result<Self, Error> {
        let (column, mask) = match column {
            Column::Nullable(Column::Nullable(..), _) => return Err(Error::Shape),
            Column::Nullable(c, m) => (*c, Some(m)),
            c => (c, None),
        };
        if column.rows() != rows || mask.is_some_and(|m| m.len() != rows) {
            return Err(Error::Shape);
        }
        let present = mask.filter(|m| m.contains(&false));
        let has = |i: usize| present.is_none_or(|m| m.get(i) == Some(&true));
        let (ty, dict, aux_len) = match column {
            Column::I32(_) => (Type::I32, None, 0),
            Column::U32(_) => (Type::U32, None, 0),
            Column::I64(_) => (Type::I64, None, 0),
            Column::U64(_) => (Type::U64, None, 0),
            Column::F32(_) => (Type::F32, None, 0),
            Column::F64(_) => (Type::F64, None, 0),
            Column::U8(_) => (Type::U8, None, 0),
            Column::I16(_) => (Type::I16, None, 0),
            Column::U16(_) => (Type::U16, None, 0),
            Column::Utf8(v) => {
                let bytes = v.iter().enumerate().filter(|(i, _)| has(*i));
                (Type::Utf8, None, bytes.map(|(_, s)| s.len()).sum())
            }
            Column::Dict(v) => {
                let (strings, codes) = dictionary(v, has)?;
                let bytes: usize = strings.iter().map(|s| s.len()).sum();
                let ty = if strings.len() <= DICT16_MAX {
                    Type::Dict16
                } else {
                    Type::Dict32
                };
                let aux = 4 + (strings.len() + 1) * 4 + bytes;
                (ty, Some((strings, codes)), aux)
            }
            Column::Nullable(..) => return Err(Error::Shape),
        };
        Ok(Self {
            ty,
            column,
            present,
            dict,
            aux_len,
        })
    }

    fn data_len(&self, rows: usize) -> usize {
        let values = if self.ty == Type::Utf8 {
            rows + 1
        } else {
            rows
        };
        values * self.ty.width()
    }

    fn has(&self, i: usize) -> bool {
        self.present.is_none_or(|m| m.get(i) == Some(&true))
    }

    /// Appends the data region; null rows are written as zero.
    fn write_data(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        macro_rules! fixed {
            ($v:expr) => {
                for (i, x) in $v.iter().enumerate() {
                    let x = if self.has(i) { *x } else { Default::default() };
                    out.extend_from_slice(&x.to_le_bytes());
                }
            };
        }
        match self.column {
            Column::I32(v) => fixed!(v),
            Column::U32(v) => fixed!(v),
            Column::I64(v) => fixed!(v),
            Column::U64(v) => fixed!(v),
            Column::F32(v) => fixed!(v),
            Column::F64(v) => fixed!(v),
            Column::U8(v) => fixed!(v),
            Column::I16(v) => fixed!(v),
            Column::U16(v) => fixed!(v),
            Column::Utf8(v) => {
                let mut at = 0u32;
                out.extend_from_slice(&at.to_le_bytes());
                for (i, s) in v.iter().enumerate() {
                    if self.has(i) {
                        at = at.checked_add(to_u32(s.len())?).ok_or(Error::TooLarge)?;
                    }
                    out.extend_from_slice(&at.to_le_bytes());
                }
            }
            Column::Dict(_) => {
                let codes = self.dict.as_ref().map_or(&[][..], |d| &d.1[..]);
                for &c in codes {
                    if self.ty == Type::Dict16 {
                        let c = u16::try_from(c).map_err(|_| Error::TooLarge)?;
                        out.extend_from_slice(&c.to_le_bytes());
                    } else {
                        out.extend_from_slice(&c.to_le_bytes());
                    }
                }
            }
            Column::Nullable(..) => return Err(Error::Shape),
        }
        Ok(())
    }

    /// Appends the auxiliary region: string bytes, or the dictionary.
    fn write_aux(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        if let Column::Utf8(v) = self.column {
            for (i, s) in v.iter().enumerate() {
                if self.has(i) {
                    out.extend_from_slice(s.as_bytes());
                }
            }
        }
        if let Some((strings, _)) = &self.dict {
            out.extend_from_slice(&to_u32(strings.len())?.to_le_bytes());
            let mut at = 0u32;
            out.extend_from_slice(&at.to_le_bytes());
            for s in strings {
                at = at.checked_add(to_u32(s.len())?).ok_or(Error::TooLarge)?;
                out.extend_from_slice(&at.to_le_bytes());
            }
            for s in strings {
                out.extend_from_slice(s.as_bytes());
            }
        }
        Ok(())
    }

    /// Appends the null bitmap.
    fn write_nulls(&self, out: &mut Vec<u8>) {
        if let Some(m) = self.present {
            for chunk in m.chunks(8) {
                let byte = chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |b, (k, &v)| b | (u8::from(v) << k));
                out.push(byte);
            }
        }
    }
}

impl Blob {
    /// Encodes named columns. Every column must have the same number of rows. The output is
    /// allocated once at its exact size. A null mask with no null in it is dropped.
    ///
    /// # Errors
    ///
    /// [`Error::Shape`], [`Error::Name`] or [`Error::TooLarge`].
    pub fn encode(columns: &[(&str, Column<'_>)]) -> Result<Self, Error> {
        let rows = columns.first().ok_or(Error::Shape)?.1.rows();
        let mut plans = Vec::with_capacity(columns.len());
        for (i, (name, col)) in columns.iter().enumerate() {
            let bad = name.is_empty() || name.len() > NAME_MAX || name.contains('\0');
            if bad || columns.iter().take(i).any(|(n, _)| n == name) {
                return Err(Error::Name);
            }
            plans.push(Plan::new(*col, rows)?);
        }
        // Layout first, so the buffer is allocated once and every offset is known.
        let mut at = pad(HEADER + columns.len() * DESCRIPTOR);
        let mut layout = Vec::with_capacity(columns.len());
        for p in &plans {
            let data = at;
            at = pad(at + p.data_len(rows));
            let aux = if p.aux_len > 0 || p.ty == Type::Utf8 || p.dict.is_some() {
                at
            } else {
                0
            };
            at = pad(at + p.aux_len);
            let nulls = if p.present.is_some() { at } else { 0 };
            if p.present.is_some() {
                at = pad(at + rows.div_ceil(8));
            }
            layout.push((data, aux, nulls));
        }
        let total = at;
        to_u32(total)?;
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&MAGIC);
        for v in [rows, columns.len(), total] {
            out.extend_from_slice(&to_u32(v)?.to_le_bytes());
        }
        for (((name, _), p), (data, aux, nulls)) in columns.iter().zip(&plans).zip(&layout) {
            let mut padded = [0u8; NAME_MAX];
            for (slot, b) in padded.iter_mut().zip(name.bytes()) {
                *slot = b;
            }
            out.extend_from_slice(&padded);
            let fields = [
                p.ty as usize,
                *data,
                p.data_len(rows),
                *aux,
                p.aux_len,
                *nulls,
            ];
            for v in fields {
                out.extend_from_slice(&to_u32(v)?.to_le_bytes());
            }
        }
        for (p, (data, aux, nulls)) in plans.iter().zip(&layout) {
            out.resize(*data, 0);
            p.write_data(&mut out)?;
            if *aux > 0 {
                out.resize(*aux, 0);
                p.write_aux(&mut out)?;
            }
            if *nulls > 0 {
                out.resize(*nulls, 0);
                p.write_nulls(&mut out);
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
    /// `u8` values.
    U8(Fixed<'a, 1>),
    /// `i16` values.
    I16(Fixed<'a, 2>),
    /// `u16` values.
    U16(Fixed<'a, 2>),
    /// Strings through a dictionary (`Dict16` or `Dict32`).
    Dict(Dict<'a>),
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
values!(1, u8s: u8);
values!(2, i16s: i16, u16s: u16);
values!(4, i32s: i32, u32s: u32, f32s: f32);
values!(8, i64s: i64, u64s: u64, f64s: f64);

/// Strings by offsets into UTF-8 bytes, validated when the reader was built.
#[derive(Debug, Clone, Copy)]
pub struct Strings<'a> {
    offsets: &'a [[u8; 4]],
    bytes: &'a str,
}

impl<'a> Strings<'a> {
    /// The string at `i`.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&'a str> {
        let at = |k: usize| self.offsets.get(k).map(|b| u32::from_le_bytes(*b) as usize);
        self.bytes.get(at(i)?..at(i + 1)?)
    }

    /// Number of strings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every string in order.
    pub fn iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        (0..self.len()).filter_map(|i| self.get(i))
    }

    /// Checks `offsets` (little-endian `u32`s) against `bytes`: UTF-8, starting at 0, monotonic,
    /// on char boundaries, ending at the end.
    fn parse(offsets: &'a [u8], bytes: &'a [u8]) -> Option<Self> {
        let bytes = core::str::from_utf8(bytes).ok()?;
        let offsets: &[[u8; 4]] = offsets.as_chunks().0;
        let mut prev = 0;
        for (k, o) in offsets.iter().enumerate() {
            let o = u32::from_le_bytes(*o) as usize;
            let ok = (k > 0 || o == 0) && o >= prev && bytes.is_char_boundary(o);
            ok.then_some(())?;
            prev = o;
        }
        (!offsets.is_empty() && prev == bytes.len()).then_some(Self { offsets, bytes })
    }
}

/// A dictionary column: a code per row into distinct strings.
#[derive(Debug, Clone, Copy)]
pub struct Dict<'a> {
    codes: Codes<'a>,
    dictionary: Strings<'a>,
}

#[derive(Debug, Clone, Copy)]
enum Codes<'a> {
    U16(&'a [[u8; 2]]),
    U32(&'a [[u8; 4]]),
}

impl<'a> Dict<'a> {
    /// The code of row `i`.
    #[must_use]
    pub fn code(&self, i: usize) -> Option<u32> {
        match self.codes {
            Codes::U16(c) => c.get(i).map(|b| u32::from(u16::from_le_bytes(*b))),
            Codes::U32(c) => c.get(i).map(|b| u32::from_le_bytes(*b)),
        }
    }

    /// The string of row `i` (empty for a null row of an empty dictionary).
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&'a str> {
        let code = self.code(i)? as usize;
        if self.dictionary.is_empty() && i < self.len() {
            return Some("");
        }
        self.dictionary.get(code)
    }

    /// Number of rows.
    #[must_use]
    pub fn len(&self) -> usize {
        match self.codes {
            Codes::U16(c) => c.len(),
            Codes::U32(c) => c.len(),
        }
    }

    /// Whether there are no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The distinct strings, in the order the codes number them.
    #[must_use]
    pub fn dictionary(&self) -> Strings<'a> {
        self.dictionary
    }
}

/// The null bitmap of a column: one bit per row, set where the row has a value.
#[derive(Debug, Clone, Copy)]
pub struct Nulls<'a> {
    bits: &'a [u8],
    rows: usize,
}

impl Nulls<'_> {
    /// Whether row `i` is null (rows past the end are not).
    #[must_use]
    pub fn is_null(&self, i: usize) -> bool {
        i < self.rows
            && self
                .bits
                .get(i / 8)
                .is_some_and(|b| b & (1 << (i % 8)) == 0)
    }

    /// Number of null rows.
    #[must_use]
    pub fn count(&self) -> usize {
        let set: usize = self.bits.iter().map(|b| b.count_ones() as usize).sum();
        self.rows.saturating_sub(set)
    }

    /// The raw bitmap, least significant bit first.
    #[must_use]
    pub fn raw(&self) -> &[u8] {
        self.bits
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    let b: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(b) as usize)
}

/// A checked byte range of the blob, 8-byte aligned and after the descriptors.
fn region(bytes: &[u8], at: usize, len: usize, floor: usize) -> Option<&[u8]> {
    (at.is_multiple_of(ALIGN) && at >= floor).then_some(())?;
    bytes.get(at..at.checked_add(len)?)
}

/// A descriptor's six fields.
struct Desc {
    ty: Type,
    at: usize,
    len: usize,
    aux_at: usize,
    aux_len: usize,
    nulls_at: usize,
}

impl<'a> Reader<'a> {
    /// Validates `bytes` as a v1 (or v0) blob: header, every descriptor, every range, every
    /// string, every dictionary code and every null bitmap.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] if anything is out of bounds, misaligned, of the wrong length, not
    /// UTF-8, if the offsets of strings are not monotonic char boundaries, if a code is outside
    /// its dictionary, or if a bitmap sets a bit past the last row.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        let check = || -> Option<Self> {
            let magic = bytes.get(..4)?;
            let v0 = magic == MAGIC_V0;
            (v0 || magic == MAGIC).then_some(())?;
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
                let d = reader.desc(i)?;
                let new = d.nulls_at != 0 || (d.ty as u32) > (Type::Utf8 as u32);
                (!(v0 && new)).then_some(())?;
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
        self.view(self.index(name)?, self.floor())
    }

    /// The null bitmap of the column called `name`; `None` when it has no null (or is missing).
    #[must_use]
    pub fn nulls(&self, name: &str) -> Option<Nulls<'a>> {
        self.bitmap(&self.desc(self.index(name)?)?, self.floor())
    }

    fn floor(&self) -> usize {
        HEADER + self.columns * DESCRIPTOR
    }

    fn index(&self, name: &str) -> Option<usize> {
        (0..self.columns).find(|&i| self.name(i) == Some(name))
    }

    fn descriptor(&self, i: usize) -> Option<&'a [u8]> {
        let at = HEADER + i * DESCRIPTOR;
        self.bytes.get(at..at + DESCRIPTOR)
    }

    fn desc(&self, i: usize) -> Option<Desc> {
        let d = self.descriptor(i)?;
        let field = |k: usize| u32_at(d, NAME_MAX + 4 * k);
        Some(Desc {
            ty: Type::from_u32(u32::try_from(field(0)?).ok()?)?,
            at: field(1)?,
            len: field(2)?,
            aux_at: field(3)?,
            aux_len: field(4)?,
            nulls_at: field(5)?,
        })
    }

    fn name(&self, i: usize) -> Option<&'a str> {
        let raw = self.descriptor(i)?.get(..NAME_MAX)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(NAME_MAX);
        let (name, rest) = raw.split_at(end);
        let ok = end > 0 && rest.iter().all(|&b| b == 0);
        ok.then(|| core::str::from_utf8(name).ok()).flatten()
    }

    fn bitmap(&self, d: &Desc, floor: usize) -> Option<Nulls<'a>> {
        if d.nulls_at == 0 {
            return None;
        }
        let bits = region(self.bytes, d.nulls_at, self.rows.div_ceil(8), floor)?;
        Some(Nulls {
            bits,
            rows: self.rows,
        })
    }

    fn view(&self, i: usize, floor: usize) -> Option<View<'a>> {
        let d = self.desc(i)?;
        let nulls = self.bitmap(&d, floor);
        if d.nulls_at != 0 {
            // Present, with no bit set past the last row.
            let n = nulls?;
            let tail = self.rows % 8;
            let last = n.bits.last().copied().unwrap_or(0);
            (tail == 0 || last >> tail == 0).then_some(())?;
        }
        let values = if d.ty == Type::Utf8 {
            self.rows.checked_add(1)?
        } else {
            self.rows
        };
        (d.len == values.checked_mul(d.ty.width())?).then_some(())?;
        let data = region(self.bytes, d.at, d.len, floor)?;
        let aux = matches!(d.ty, Type::Utf8 | Type::Dict16 | Type::Dict32);
        (aux || (d.aux_at == 0 && d.aux_len == 0)).then_some(())?;
        Some(match d.ty {
            Type::I32 => View::I32(Fixed(data.as_chunks().0)),
            Type::U32 => View::U32(Fixed(data.as_chunks().0)),
            Type::I64 => View::I64(Fixed(data.as_chunks().0)),
            Type::U64 => View::U64(Fixed(data.as_chunks().0)),
            Type::F32 => View::F32(Fixed(data.as_chunks().0)),
            Type::F64 => View::F64(Fixed(data.as_chunks().0)),
            Type::U8 => View::U8(Fixed(data.as_chunks().0)),
            Type::I16 => View::I16(Fixed(data.as_chunks().0)),
            Type::U16 => View::U16(Fixed(data.as_chunks().0)),
            Type::Utf8 => {
                let bytes = region(self.bytes, d.aux_at, d.aux_len, floor)?;
                View::Utf8(Strings::parse(data, bytes)?)
            }
            Type::Dict16 | Type::Dict32 => {
                let aux = region(self.bytes, d.aux_at, d.aux_len, floor)?;
                let count = u32_at(aux, 0)?;
                let split = count.checked_add(1)?.checked_mul(4)?.checked_add(4)?;
                let dictionary = Strings::parse(aux.get(4..split)?, aux.get(split..)?)?;
                let codes = if d.ty == Type::Dict16 {
                    Codes::U16(data.as_chunks().0)
                } else {
                    Codes::U32(data.as_chunks().0)
                };
                let dict = Dict { codes, dictionary };
                for r in 0..self.rows {
                    let null = nulls.is_some_and(|n| n.is_null(r));
                    let code = dict.code(r)? as usize;
                    (code < count || (null && code == 0)).then_some(())?;
                }
                View::Dict(dict)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ALIGN, Blob, Column, DESCRIPTOR, Error, HEADER, NAME_MAX, Reader, Type, View};
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    const ELEV: Column<'static> = Column::I16(&[1400, 9, -3]);

    fn sample() -> Blob {
        Blob::encode(&[
            ("id", Column::U32(&[7, 8, 9])),
            ("lat", Column::F64(&[27.7, -33.9, 0.0])),
            ("elev", Column::F32(&[1400.5, 3.0, -1.0])),
            ("pop", Column::I64(&[i64::MIN, 0, i64::MAX])),
            ("name", Column::Utf8(&["काठमाडौं", "", "Zürich"])),
            ("d", Column::I32(&[-1, 0, 1])),
            ("u", Column::U64(&[u64::MAX, 1, 2])),
            ("kind", Column::U8(&[1, 2, 255])),
            ("e16", Column::Nullable(&ELEV, &[true, false, true])),
            ("level", Column::U16(&[0, 1, u16::MAX])),
            (
                "tz",
                Column::Dict(&["Asia/Kathmandu", "Europe/Zurich", "Asia/Kathmandu"]),
            ),
            (
                "iso",
                Column::Nullable(
                    &Column::Dict(&["NP-P3", "x", "CH-ZH"]),
                    &[true, false, true],
                ),
            ),
            (
                "parent",
                Column::Nullable(&Column::Utf8(&["a", "dropped", "c"]), &[true, false, true]),
            ),
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

    fn dict<'a>(r: &Reader<'a>, name: &str) -> Vec<&'a str> {
        let Some(View::Dict(d)) = r.column(name) else {
            panic!("{name}")
        };
        (0..d.len()).filter_map(|i| d.get(i)).collect()
    }

    #[test]
    fn round_trips_every_type() {
        let blob = sample();
        let reader = Reader::new(blob.as_bytes()).unwrap();
        assert_eq!(reader.rows(), 3);
        assert_eq!(
            reader.names().collect::<Vec<_>>(),
            [
                "id", "lat", "elev", "pop", "name", "d", "u", "kind", "e16", "level", "tz", "iso",
                "parent"
            ]
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
        assert_eq!(values!(reader, "kind", U8, u8s), Some(vec![1, 2, 255]));
        assert_eq!(
            values!(reader, "level", U16, u16s),
            Some(vec![0, 1, u16::MAX])
        );
        // A null row's value is zero.
        assert_eq!(values!(reader, "e16", I16, i16s), Some(vec![1400, 0, -3]));
        assert_eq!(values!(reader, "id", F32, f32s), None);
        let Some(View::Utf8(names)) = reader.column("name") else {
            panic!("name")
        };
        assert_eq!(names.iter().collect::<Vec<_>>(), ["काठमाडौं", "", "Zürich"]);
        assert_eq!(names.get(3), None);
        assert!(reader.column("missing").is_none());
        assert_eq!(
            dict(&reader, "tz"),
            ["Asia/Kathmandu", "Europe/Zurich", "Asia/Kathmandu"]
        );
        let Some(View::Dict(tz)) = reader.column("tz") else {
            panic!("tz")
        };
        assert_eq!(tz.dictionary().len(), 2);
        assert_eq!(dict(&reader, "iso"), ["NP-P3", "NP-P3", "CH-ZH"]);
        let Some(View::Utf8(parent)) = reader.column("parent") else {
            panic!("parent")
        };
        assert_eq!(parent.iter().collect::<Vec<_>>(), ["a", "", "c"]);
        for name in ["e16", "iso", "parent"] {
            let n = reader.nulls(name).unwrap();
            assert_eq!(
                (n.is_null(0), n.is_null(1), n.is_null(2)),
                (false, true, false)
            );
            assert_eq!(n.count(), 1);
            assert!(!n.is_null(3));
        }
        assert!(reader.nulls("name").is_none());
        assert!(reader.nulls("missing").is_none());
    }

    #[test]
    fn a_mask_with_no_null_is_dropped() {
        let all = [true; 3];
        let with = Blob::encode(&[("e", Column::Nullable(&ELEV, &all))]).unwrap();
        let without = Blob::encode(&[("e", ELEV)]).unwrap();
        assert_eq!(with, without);
    }

    #[test]
    fn dictionaries_widen_past_u16() {
        let owned: Vec<String> = (0..70_000).map(|i| format!("{i}")).collect();
        let strings: Vec<&str> = owned.iter().map(String::as_str).collect();
        let blob = Blob::encode(&[("s", Column::Dict(&strings))]).unwrap();
        let b = blob.as_bytes();
        let ty = u32::from_le_bytes(
            b[HEADER + NAME_MAX..HEADER + NAME_MAX + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(ty, Type::Dict32 as u32);
        let r = Reader::new(b).unwrap();
        assert_eq!(dict(&r, "s")[69_999], "69999");
        let small = Blob::encode(&[("s", Column::Dict(&strings[..10]))]).unwrap();
        let ty = &small.as_bytes()[HEADER + NAME_MAX..HEADER + NAME_MAX + 4];
        assert_eq!(ty, (Type::Dict16 as u32).to_le_bytes());
    }

    #[test]
    fn an_all_null_dictionary_is_empty() {
        let blob = Blob::encode(&[(
            "s",
            Column::Nullable(&Column::Dict(&["a", "b"]), &[false, false]),
        )])
        .unwrap();
        let r = Reader::new(blob.as_bytes()).unwrap();
        assert_eq!(dict(&r, "s"), ["", ""]);
        assert_eq!(r.nulls("s").map(|n| n.count()), Some(2));
    }

    #[test]
    fn reads_a_v0_blob() {
        // v0: `sample_rows` of teigeodata-api before v1, as checked into the parity tests.
        let mut b = Blob::encode(&[("lat", Column::F64(&[1.5])), ("n", Column::Utf8(&["x"]))])
            .unwrap()
            .into_vec();
        b[..4].copy_from_slice(b"TGB0");
        let r = Reader::new(&b).unwrap();
        assert_eq!(values!(r, "lat", F64, f64s), Some(vec![1.5]));
        // A v0 blob cannot hold v1 types or bitmaps.
        let mut v1 = Blob::encode(&[("k", Column::U8(&[1]))]).unwrap().into_vec();
        v1[..4].copy_from_slice(b"TGB0");
        assert!(Reader::new(&v1).is_err());
    }

    #[test]
    fn regions_are_aligned_and_length_is_exact() {
        let blob = sample();
        let b = blob.as_bytes();
        assert_eq!(b.len() % ALIGN, 0);
        for i in 0..13 {
            let d = HEADER + i * DESCRIPTOR + NAME_MAX;
            for k in [4, 12, 20] {
                let at = u32::from_le_bytes(b[d + k..d + k + 4].try_into().unwrap());
                assert_eq!(at % 8, 0, "column {i} field {k}");
            }
        }
        assert_eq!(blob.clone().into_vec().capacity(), b.len());
    }

    #[test]
    fn empty_columns_encode() {
        let blob = Blob::encode(&[
            ("a", Column::F64(&[])),
            ("s", Column::Utf8(&[])),
            ("d", Column::Dict(&[])),
        ]);
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
        let short_mask = [("a", Column::Nullable(&ELEV, &[true]))];
        assert_eq!(Blob::encode(&short_mask), Err(Error::Shape));
        let inner = Column::Nullable(&ELEV, &[true, false, true]);
        let nested = [("a", Column::Nullable(&inner, &[true, true, true]))];
        assert_eq!(Blob::encode(&nested), Err(Error::Shape));
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
                        match r.column(name) {
                            Some(View::Utf8(s)) => assert_eq!(s.iter().count(), r.rows()),
                            Some(View::Dict(d)) => {
                                assert!((0..r.rows()).all(|k| d.get(k).is_some()));
                            }
                            _ => {}
                        }
                        if let Some(n) = r.nulls(name) {
                            assert!(n.count() <= r.rows());
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
        let field = |col: usize, k: usize| HEADER + col * DESCRIPTOR + NAME_MAX + 4 * k;
        let at = |col: usize, k: usize| {
            u32::from_le_bytes(good[field(col, k)..field(col, k) + 4].try_into().unwrap()) as usize
        };
        // Column 10 is `tz` (a 2-string dictionary), 8 is `e16` (row 1 null).
        let codes = at(10, 1);
        let bitmap = at(8, 5);
        let mut code_out = good.clone();
        code_out[codes..codes + 2].copy_from_slice(&2u16.to_le_bytes());
        let mut stray_bit = good.clone();
        stray_bit[bitmap] |= 0b1000;
        let cases = [
            set(0, 0),            // magic
            set(4, 4),            // row count
            set(8, 1000),         // column count
            set(12, 8),           // total length
            set(field(0, 0), 99), // unknown type
            set(field(0, 1), 3),  // misaligned data
            set(field(4, 3), 1),  // misaligned string bytes
            set(field(0, 3), 8),  // aux on a fixed column
            set(field(8, 5), 4),  // misaligned bitmap
            set(at(10, 3), 7),    // dictionary count past its offsets
            code_out,             // a code outside the dictionary
            stray_bit,            // a bit past the last row
        ];
        for (k, bad) in cases.iter().enumerate() {
            assert!(Reader::new(bad).is_err(), "case {k}");
        }
        assert!(Reader::new(&good).is_ok());
    }
}
