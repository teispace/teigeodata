//! DEM (kind 13): elevation on regular grids, at a bounded error, in tiles a point query decodes
//! on its own (D-089, B11).
//!
//! Heights are quantised to `q = round(h / step)`, so every stored height is within `step / 2`
//! of the source; everything after that is lossless on `q`. A grid is cut into square tiles
//! (`2^log2` samples a side) on a global tile grid anchored at 90° N, 180° W, so neighbouring
//! packs share identical tiles. Each tile is coded alone:
//!
//! - **Prediction:** `p = a + ((Σ cₖ·(nₖ − a) + 32) >> 6)` over the left `a`, above `b`,
//!   above-left `c`, above-right `d`, left-left `aa` and above-above `bb` samples, with five i8
//!   coefficients fitted per tile by least squares (in 1/64). A neighbour outside the tile is
//!   replaced by the nearest one inside: `a` by `b` in column 0, `b`, `c`, `d` and `bb` by `a` in
//!   row 0, `c` by `b` in column 0, `d` by `b` in the last column, `aa` by `a` in columns 0 and 1,
//!   `bb` by `b` in rows 0 and 1. The first sample is the tile's base.
//! - **Contexts:** the activity `|a − c| + |b − c| + |d − b|` against seven thresholds picks one
//!   of 8 contexts; each context of each tile names one model of the section's family.
//! - **Coding:** the residual's zigzag `z` is coded with static rANS (32-bit state, byte
//!   renormalisation, 12-bit frequencies) over 64 symbols; symbol 63 escapes, followed by
//!   `z − 63` as three uniform bytes (high first). Model 0 of the family codes only zeros and
//!   costs no bits, so a flat tile is its header and a 4-byte state.
//!
//! A [column table](teigeodata_core::table); `item_count` is the number of tiles over all grids.
//! Rows of columns 1–10 are grids, usually one; a grid's tiles are row-major over its box of
//! tiles, absent tiles included.
//!
//! | ID | Column | Values |
//! |---:|---|---|
//! | 1 | [`TILE_ROW0`] | per grid: its first tile row on the global grid (rows of tiles from 90° N) |
//! | 2 | [`TILE_COL0`] | per grid: its first tile column (columns of tiles from 180° W) |
//! | 3 | [`TILE_ROWS`] | per grid: rows of tiles |
//! | 4 | [`TILE_COLS`] | per grid: columns of tiles |
//! | 5 | [`LAT_STEP`] | per grid: sample spacing in latitude, in 0.1″ (10 for 1″) |
//! | 6 | [`LON_STEP`] | per grid: sample spacing in longitude, in 0.1″ |
//! | 7 | [`STEP_MM`] | per grid: the height step in millimetres (2,000 for ±1 m) |
//! | 8 | [`DATUM`] | per grid: [`DATUM_EGM2008`] (others reserved) |
//! | 9 | [`TILE_LOG2`] | per grid: log2 of the tile side, 4 to 6 |
//! | 10 | [`TILE_START`] | per grid and one more: its first tile; the last is `item_count` |
//! | 11 | [`OFFSETS`] | per tile and one more: its code's byte offset in [`PAYLOAD`]; an absent tile has none (equal neighbours) |
//! | 12 | [`MODELS`] | bytes: 64 models × 64 symbols, u16 little-endian frequencies summing to 4,096 |
//! | 13 | [`CONTEXTS`] | the 7 activity thresholds, non-decreasing |
//! | 14 | [`PAYLOAD`] | bytes: the tile codes back to back, zero-padded |
//!
//! A tile code: the base `q` (zigzag LEB128), the five coefficients (i8, in the order `b`, `c`,
//! `d`, `aa`, `bb`), 8 model indices (6 bits each, little-endian, 6 bytes), the rANS state
//! (u32 little-endian), then the renormalisation bytes. Decoding runs in raster order, so a
//! query stops after the rows it needs, and a [`DemScratch`] keeps a tile part-decoded for the
//! next query.
//!
//! Sample row `r` of a grid lies at latitude 90° − r·`LAT_STEP`, column `c` at longitude
//! −180° + c·`LON_STEP` (`PixelIsPoint`). [`Dem::elevation`] interpolates bilinearly between the
//! four samples around a point; a convex combination of values within `step / 2` stays within it.

#![allow(
    clippy::many_single_char_names,
    reason = "the predictor's neighbours a, b, c, d are the literature's names (LOCO-I); r, c, s, t are rows, columns, scratch and tiles"
)]

use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;
use teigeodata_core::{E7, Point};

use crate::{Error, Kind, Section, columns};

/// Column: each grid's first tile row on the global tile grid.
pub const TILE_ROW0: u16 = 1;
/// Column: each grid's first tile column.
pub const TILE_COL0: u16 = 2;
/// Column: each grid's rows of tiles.
pub const TILE_ROWS: u16 = 3;
/// Column: each grid's columns of tiles.
pub const TILE_COLS: u16 = 4;
/// Column: each grid's latitude spacing, 0.1″.
pub const LAT_STEP: u16 = 5;
/// Column: each grid's longitude spacing, 0.1″.
pub const LON_STEP: u16 = 6;
/// Column: each grid's height step, millimetres.
pub const STEP_MM: u16 = 7;
/// Column: each grid's vertical datum.
pub const DATUM: u16 = 8;
/// Column: each grid's log2 tile side.
pub const TILE_LOG2: u16 = 9;
/// Column: each grid's first tile, and one more.
pub const TILE_START: u16 = 10;
/// Column: each tile's code offset in [`PAYLOAD`], and one more.
pub const OFFSETS: u16 = 11;
/// Column: the model family (bytes).
pub const MODELS: u16 = 12;
/// Column: the context thresholds.
pub const CONTEXTS: u16 = 13;
/// Column: the tile codes (bytes).
pub const PAYLOAD: u16 = 14;

/// Heights above the EGM2008 geoid (orthometric), as Copernicus GLO-30 ships them.
pub const DATUM_EGM2008: u32 = 1;
/// Symbols of a model: zigzag residuals 0 to 62, and the escape.
pub const SYMBOLS: usize = 64;
/// Models in a family.
pub const FAMILY: usize = 64;
/// Activity contexts per tile.
pub const CONTEXT_COUNT: usize = 8;
/// The escape symbol.
const ESCAPE: u32 = 63;
/// Frequency bits: a model's frequencies sum to 2^12.
const SCALE_BITS: u32 = 12;
const SCALE: u32 = 1 << SCALE_BITS;
/// The rANS state's lower bound.
const RANS_L: u32 = 1 << 23;
/// The smallest and largest log2 tile side.
const MIN_LOG2: u32 = 4;
/// The largest log2 tile side; a scratch holds one tile of this side.
pub const MAX_LOG2: u32 = 6;
const MAX_TILE: usize = 1 << (2 * MAX_LOG2);
/// Units of 0.1″ in 360°.
const TURN: u64 = 12_960_000;
/// Units of 0.1″ in 180°.
const HALF_TURN: u64 = 6_480_000;
/// The largest escaped value, `z − 63` in three bytes.
const MAX_ESCAPE: u64 = 1 << 24;

fn fail(column: u16, problem: &'static str) -> Error {
    Error::Payload {
        kind: Kind::DEM.0,
        column,
        problem,
    }
}

/// One grid of a DEM section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    /// First tile row on the global tile grid.
    pub tile_row0: u32,
    /// First tile column.
    pub tile_col0: u32,
    /// Rows of tiles.
    pub tile_rows: u32,
    /// Columns of tiles.
    pub tile_cols: u32,
    /// Latitude spacing, 0.1″.
    pub lat_step: u32,
    /// Longitude spacing, 0.1″.
    pub lon_step: u32,
    /// Height step, millimetres.
    pub step_mm: u32,
    /// Vertical datum.
    pub datum: u32,
    /// log2 of the tile side.
    pub tile_log2: u32,
    /// Its first tile in the section.
    pub first_tile: u32,
}

impl Grid {
    /// The tile side in samples.
    #[must_use]
    pub const fn side(&self) -> usize {
        1 << self.tile_log2
    }

    /// The section tile at tile row `tr` and column `tc` of the global tile grid, if inside.
    fn tile(&self, tr: u64, tc: u64) -> Option<usize> {
        let r = tr.checked_sub(u64::from(self.tile_row0))?;
        let c = tc.checked_sub(u64::from(self.tile_col0))?;
        if r >= u64::from(self.tile_rows) || c >= u64::from(self.tile_cols) {
            return None;
        }
        usize::try_from(u64::from(self.first_tile) + r * u64::from(self.tile_cols) + c).ok()
    }
}

/// A caller-owned buffer for decoding: one tile of up to 64 × 64 samples (16 KiB) and where
/// its decoding stopped, so the next query in the same tile goes on from there. Reused across
/// queries; no query allocates.
#[derive(Clone, Debug)]
pub struct DemScratch {
    q: [i32; MAX_TILE],
    /// The tile in `q` (section tile number), if any.
    tile: Option<usize>,
    log2: u32,
    /// Samples decoded so far.
    done: usize,
    state: u32,
    /// The next renormalisation byte, in the section's payload.
    pos: usize,
    end: usize,
    coef: [i64; 5],
    models: [u8; CONTEXT_COUNT],
    /// Each context's cumulative frequencies for the tile being decoded, ending at 4,096.
    cum: [[u32; SYMBOLS + 1]; CONTEXT_COUNT],
    /// Each context's first symbol in each of 64 slot buckets, where the symbol search starts.
    first: [[u8; BUCKETS]; CONTEXT_COUNT],
}

/// Slot buckets of the symbol search: 4,096 slots in buckets of 64.
const BUCKETS: usize = 64;
/// log2 of a bucket's width in slots.
const BUCKET_BITS: u32 = SCALE_BITS - 6;

impl Default for DemScratch {
    fn default() -> Self {
        Self {
            q: [0; MAX_TILE],
            tile: None,
            log2: 0,
            done: 0,
            state: 0,
            pos: 0,
            end: 0,
            coef: [0; 5],
            models: [0; CONTEXT_COUNT],
            cum: [[0; SYMBOLS + 1]; CONTEXT_COUNT],
            first: [[0; BUCKETS]; CONTEXT_COUNT],
        }
    }
}

impl DemScratch {
    /// An empty scratch.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// The DEM section of an open pack.
#[derive(Clone, Copy, Debug)]
pub struct Dem<'a> {
    cols: [Packed<'a>; 10],
    grids: usize,
    tiles: usize,
    offsets: Packed<'a>,
    coder: Coder<'a>,
}

/// What decoding a tile code needs: the model family, the thresholds and the codes' bytes.
#[derive(Clone, Copy, Debug)]
struct Coder<'a> {
    models: &'a [u8],
    thresholds: [u32; 7],
    /// The context of every activity below [`SMALL_ACTIVITY`], from the thresholds.
    small: [u8; SMALL_ACTIVITY],
    payload: &'a [u8],
}

/// Activities with a precomputed context.
const SMALL_ACTIVITY: usize = 64;

/// The context of each activity below [`SMALL_ACTIVITY`].
fn small_contexts(thresholds: &[u32; 7]) -> [u8; SMALL_ACTIVITY] {
    let mut small = [0; SMALL_ACTIVITY];
    for (act, c) in (0_u64..).zip(small.iter_mut()) {
        *c = u8::try_from(context(act, thresholds)).unwrap_or(u8::MAX);
    }
    small
}

/// The context of activity `act`: how many thresholds it exceeds.
#[inline]
fn context(act: u64, thresholds: &[u32; 7]) -> usize {
    thresholds.iter().filter(|&&t| act > u64::from(t)).count()
}

/// The six neighbours of sample `i` in a tile of side `1 << log2`, with the edge rules of the
/// module comment: `[a, b, c, d, aa, bb]`.
#[inline]
fn neighbours(q: &[i32], log2: u32, i: usize) -> [i64; 6] {
    let n = 1_usize << log2;
    let (y, x) = (i >> log2, i & (n - 1));
    let at = |j: usize| i64::from(q.get(j).copied().unwrap_or(0));
    let above = if y > 0 { Some(at(i - n)) } else { None };
    let a = if x > 0 { at(i - 1) } else { above.unwrap_or(0) };
    let b = above.unwrap_or(a);
    let c = if x > 0 && y > 0 { at(i - n - 1) } else { b };
    let d = if y > 0 && x + 1 < n { at(i - n + 1) } else { b };
    let aa = if x > 1 { at(i - 2) } else { a };
    let bb = if y > 1 { at(i - 2 * n) } else { b };
    [a, b, c, d, aa, bb]
}

/// The prediction and context of a sample from its neighbours.
#[inline]
fn predict(nb: &[i64; 6], coef: &[i64; 5], thresholds: &[u32; 7]) -> (i64, usize) {
    let [a, b, c, d, aa, bb] = *nb;
    let sum = coef[0] * (b - a)
        + coef[1] * (c - a)
        + coef[2] * (d - a)
        + coef[3] * (aa - a)
        + coef[4] * (bb - a);
    let act = (a - c).unsigned_abs() + (b - c).unsigned_abs() + (d - b).unsigned_abs();
    (a + ((sum + 32) >> 6), context(act, thresholds))
}

const fn zigzag(r: i64) -> u64 {
    ((r << 1) ^ (r >> 63)).cast_unsigned()
}

const fn unzigzag(z: u64) -> i64 {
    (z >> 1).cast_signed() ^ -(z & 1).cast_signed()
}

impl Coder<'_> {
    /// Frequency and cumulative frequency of symbol `s` in model `m`, or the symbol holding
    /// `slot` (by a linear scan: residuals are small, so the scan is short).
    fn freq(&self, m: usize, s: usize) -> u32 {
        let at = (m * SYMBOLS + s) * 2;
        match self.models.get(at..at + 2) {
            Some(&[lo, hi]) => u32::from(u16::from_le_bytes([lo, hi])),
            _ => 0,
        }
    }

    /// Starts decoding the code at `range` of the payload, tile `t` of side `1 << log2`, into
    /// `s`.
    fn start(&self, range: (usize, usize), t: usize, log2: u32, s: &mut DemScratch) -> Option<()> {
        s.tile = None;
        let (mut pos, end) = range;
        let mut base: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = *self.payload.get(pos)?;
            pos += 1;
            base |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 35 {
                return None;
            }
        }
        let head = self.payload.get(pos..pos + 15)?;
        for (c, &b) in s.coef.iter_mut().zip(head) {
            *c = i64::from(b.cast_signed());
        }
        let bits = head
            .get(5..11)?
            .iter()
            .rev()
            .fold(0_u64, |acc, &b| (acc << 8) | u64::from(b));
        for (k, (m, cum)) in s.models.iter_mut().zip(s.cum.iter_mut()).enumerate() {
            *m = ((bits >> (6 * k)) & 63) as u8;
            let mut total = 0;
            for (sym, c) in cum.iter_mut().enumerate() {
                *c = total;
                if sym < SYMBOLS {
                    total += self.freq(usize::from(*m), sym);
                }
            }
            // A model not summing to 4,096 is refused here, so decoding needs no checks.
            if total != SCALE {
                return None;
            }
        }
        for (cum, first) in s.cum.iter().zip(s.first.iter_mut()) {
            let mut sym = 0;
            for (bucket, f) in (0_u32..).zip(first.iter_mut()) {
                let slot = bucket << BUCKET_BITS;
                while sym + 1 < SYMBOLS && cum.get(sym + 1).is_some_and(|&c| c <= slot) {
                    sym += 1;
                }
                *f = u8::try_from(sym).ok()?;
            }
        }
        s.state = u32::from_le_bytes(head.get(11..15)?.try_into().ok()?);
        s.q[0] = i32::try_from(unzigzag(base)).ok()?;
        s.pos = pos + 15;
        s.end = end;
        s.log2 = log2;
        s.done = 1;
        s.tile = Some(t);
        Some(())
    }

    /// Decodes the started tile in `s` up to sample `upto` (exclusive). The hot loop keeps the
    /// coder's state in locals and writes it back once.
    fn run(&self, s: &mut DemScratch, upto: usize) -> Option<()> {
        let upto = upto.min(1_usize << (2 * s.log2));
        let mut rans = Stream {
            state: s.state,
            pos: s.pos,
            bytes: self.payload.get(..s.end)?,
        };
        let mut i = s.done;
        let ok = self.steps(s, &mut rans, &mut i, upto);
        s.state = rans.state;
        s.pos = rans.pos;
        s.done = i;
        ok
    }

    /// The samples from `*i` to `upto` of the tile in `s`. Inside a row (from column 2 to the
    /// last but one, from row 2) the neighbours roll in registers: only `d` and `bb` are loaded,
    /// from rows already decoded, so the chain from one sample to the next never waits on memory.
    #[inline]
    fn steps(
        &self,
        s: &mut DemScratch,
        rans: &mut Stream<'_>,
        i: &mut usize,
        upto: usize,
    ) -> Option<()> {
        let log2 = s.log2;
        let side = 1_usize << log2;
        let k = s.coef;
        let (q, cum, first) = (&mut s.q, &s.cum, &s.first);
        let one = |nb: [i64; 6], rans: &mut Stream<'_>| -> Option<i32> {
            let [a, b, c, d, aa, bb] = nb;
            let sum = k[0] * (b - a)
                + k[1] * (c - a)
                + k[2] * (d - a)
                + k[3] * (aa - a)
                + k[4] * (bb - a);
            let p = a + ((sum + 32) >> 6);
            let act = (a - c).unsigned_abs() + (b - c).unsigned_abs() + (d - b).unsigned_abs();
            let ctx = usize::try_from(act)
                .ok()
                .and_then(|v| self.small.get(v))
                .map_or_else(|| context(act, &self.thresholds), |&c| usize::from(c));
            let mut z = u64::from(rans.symbol(cum.get(ctx)?, first.get(ctx)?)?);
            if z == u64::from(ESCAPE) {
                z += (rans.byte()? << 16) | (rans.byte()? << 8) | rans.byte()?;
            }
            i32::try_from(p + unzigzag(z)).ok()
        };
        while *i < upto {
            let n = *i;
            let (x, y) = (n & (side - 1), n >> log2);
            if y >= 2 && x >= 2 && x + 1 < side {
                // A run inside the row, up to the last column but one.
                let stop = upto.min(n - x + side - 1);
                let at = |q: &[i32; MAX_TILE], j: usize| q.get(j).map(|&v| i64::from(v));
                let mut nb = [
                    at(q, n - 1)?,
                    at(q, n - side)?,
                    at(q, n - side - 1)?,
                    at(q, n - side + 1)?,
                    at(q, n - 2)?,
                    at(q, n - 2 * side)?,
                ];
                let mut m = n;
                loop {
                    let v = one(nb, rans)?;
                    *q.get_mut(m)? = v;
                    m += 1;
                    if m == stop {
                        break;
                    }
                    let [a, b, _, d, _, _] = nb;
                    nb = [
                        i64::from(v),
                        d,
                        b,
                        at(q, m - side + 1)?,
                        a,
                        at(q, m - 2 * side)?,
                    ];
                }
                *i = m;
            } else {
                let v = one(neighbours(q, log2, n), rans)?;
                *q.get_mut(n)? = v;
                *i += 1;
            }
        }
        Some(())
    }
}

/// The rANS decoder over one tile's renormalisation bytes, held in locals by [`Coder::run`].
struct Stream<'p> {
    state: u32,
    /// The next byte in `bytes`.
    pos: usize,
    /// The section's payload up to the tile's end.
    bytes: &'p [u8],
}

impl Stream<'_> {
    /// Decodes one symbol with cumulative frequencies `cum`, or `None` on a corrupt stream.
    /// A linear scan from the slot's bucket finds it (B11: faster than a branch-free count).
    #[inline]
    fn symbol(&mut self, cum: &[u32; SYMBOLS + 1], first: &[u8; BUCKETS]) -> Option<u32> {
        let slot = self.state & (SCALE - 1);
        // `cum[0] = 0 <= slot < 4,096 = cum[64]`, checked when the tile started; the search
        // starts at the bucket's first symbol, so it rarely moves.
        let mut sym = usize::from(*first.get((slot >> BUCKET_BITS) as usize)?);
        while sym + 1 < SYMBOLS && cum.get(sym + 1).is_some_and(|&c| c <= slot) {
            sym += 1;
        }
        let (lo, hi) = (*cum.get(sym)?, *cum.get(sym + 1)?);
        self.state = (hi - lo) * (self.state >> SCALE_BITS) + slot - lo;
        self.renorm()?;
        u32::try_from(sym).ok()
    }

    /// A uniform byte from the stream (frequency 16 each).
    #[inline]
    fn byte(&mut self) -> Option<u64> {
        let slot = self.state & (SCALE - 1);
        self.state = 16 * (self.state >> SCALE_BITS) + (slot & 15);
        self.renorm()?;
        Some(u64::from(slot >> 4))
    }

    #[inline]
    fn renorm(&mut self) -> Option<()> {
        while self.state < RANS_L {
            self.state = (self.state << 8) | u32::from(*self.bytes.get(self.pos)?);
            self.pos += 1;
        }
        Some(())
    }
}

impl<'a> Dem<'a> {
    /// Opens the section: frame, columns and lengths, in O(columns + grids).
    ///
    /// # Errors
    /// [`Error::Payload`] if it is not a DEM column table, or a column is missing, unknown,
    /// malformed or of the wrong length.
    pub fn open(section: &Section<'a>) -> Result<Self, Error> {
        let table: Table<'a> = columns::open(section, Kind::DEM)?;
        let tiles = usize::try_from(section.item_count())
            .map_err(|_| fail(0, "the tile count does not fit this platform"))?;
        if table.ids().any(|id| id == 0 || id > PAYLOAD) {
            return Err(fail(0, "an unknown column"));
        }
        let grids = columns::required(&table, Kind::DEM, TILE_ROW0, None)?.len();
        let col = |id| columns::required(&table, Kind::DEM, id, Some(grids));
        let cols = [
            col(TILE_ROW0)?,
            col(TILE_COL0)?,
            col(TILE_ROWS)?,
            col(TILE_COLS)?,
            col(LAT_STEP)?,
            col(LON_STEP)?,
            col(STEP_MM)?,
            col(DATUM)?,
            col(TILE_LOG2)?,
            columns::required(&table, Kind::DEM, TILE_START, Some(grids + 1))?,
        ];
        let offsets = columns::required(&table, Kind::DEM, OFFSETS, Some(tiles + 1))?;
        let models = table
            .bytes(MODELS)
            .map_err(|e| columns::map(Kind::DEM, e))?;
        if models.len() != FAMILY * SYMBOLS * 2 {
            return Err(fail(MODELS, "the model family is not 64 × 64 frequencies"));
        }
        let ctx = columns::required(&table, Kind::DEM, CONTEXTS, Some(7))?;
        let mut thresholds = [0; 7];
        ctx.decode_all(&mut thresholds);
        let payload = table
            .bytes(PAYLOAD)
            .map_err(|e| columns::map(Kind::DEM, e))?;
        Ok(Self {
            cols,
            grids,
            tiles,
            offsets,
            coder: Coder {
                models,
                thresholds,
                small: small_contexts(&thresholds),
                payload,
            },
        })
    }

    /// The number of grids.
    #[must_use]
    pub const fn grids(&self) -> usize {
        self.grids
    }

    /// The number of tiles over all grids, absent ones included.
    #[must_use]
    pub const fn tiles(&self) -> usize {
        self.tiles
    }

    /// Grid `g`, or `None` past the end.
    #[must_use]
    pub fn grid(&self, g: usize) -> Option<Grid> {
        let v = |k: usize| self.cols.get(k).and_then(|c| c.get(g));
        Some(Grid {
            tile_row0: v(0)?,
            tile_col0: v(1)?,
            tile_rows: v(2)?,
            tile_cols: v(3)?,
            lat_step: v(4)?,
            lon_step: v(5)?,
            step_mm: v(6)?,
            datum: v(7)?,
            tile_log2: v(8)?,
            first_tile: v(9)?,
        })
    }

    /// Tile `t`'s code, or `None` if it is absent or past the end.
    fn code(&self, t: usize) -> Option<(usize, usize)> {
        let start = self.offsets.get(t)? as usize;
        let end = self.offsets.get(t + 1)? as usize;
        (end > start && end <= self.coder.payload.len()).then_some((start, end))
    }

    /// The quantised sample at global sample row `r`, column `c` of grid `g`, decoding into `s`
    /// as needed; `None` outside the grid or in an absent tile.
    fn sample(&self, g: &Grid, r: u64, c: u64, s: &mut DemScratch) -> Option<i32> {
        let log2 = g.tile_log2;
        let t = g.tile(r >> log2, c >> log2)?;
        let mask = (1_u64 << log2) - 1;
        let i = usize::try_from(((r & mask) << log2) | (c & mask)).ok()?;
        if s.tile != Some(t) {
            self.coder.start(self.code(t)?, t, log2, s)?;
        }
        if s.done <= i && self.coder.run(s, i + 1).is_none() {
            s.tile = None;
            return None;
        }
        s.q.get(i).copied()
    }

    /// The height at `at` in metres, interpolated bilinearly between the four samples around
    /// it, from the first grid that has any of them. Samples in absent tiles are left out and
    /// the weights of the rest renormalised; `None` where no grid has a sample with weight.
    #[must_use]
    pub fn elevation(&self, at: Point<E7>, s: &mut DemScratch) -> Option<f64> {
        (0..self.grids).find_map(|g| self.grid(g).and_then(|grid| self.bilinear(&grid, at, s)))
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "remainders below 2^40 and heights below 2^31: exact in f64 to far below a millimetre"
    )]
    fn bilinear(&self, g: &Grid, at: Point<E7>, s: &mut DemScratch) -> Option<f64> {
        // Rows from 90° N and columns from 180° W, as exact fractions: 1e-7° × 36 / 1e4 = 0.1″.
        let num_r = (900_000_000 - i64::from(at.lat())).unsigned_abs() * 36;
        let num_c = (1_800_000_000 + i64::from(at.lon())).unsigned_abs() * 36;
        let (den_r, den_c) = (
            10_000 * u64::from(g.lat_step),
            10_000 * u64::from(g.lon_step),
        );
        if den_r == 0 || den_c == 0 {
            return None;
        }
        let (r, fr) = (num_r / den_r, (num_r % den_r) as f64 / den_r as f64);
        let (c, fc) = (num_c / den_c, (num_c % den_c) as f64 / den_c as f64);
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (dr, wr) in [(0, 1.0 - fr), (1, fr)] {
            for (dc, wc) in [(0, 1.0 - fc), (1, fc)] {
                let w = wr * wc;
                if w > 0.0
                    && let Some(q) = self.sample(g, r + dr, c + dc, s)
                {
                    sum += w * f64::from(q);
                    weight += w;
                }
            }
        }
        (weight > 0.0).then(|| sum / weight * f64::from(g.step_mm) / 1000.0)
    }

    /// Decodes tile `t` (a section tile number) whole into `s`, or `None` if it is absent or
    /// corrupt. The samples are row-major, `side × side`.
    #[must_use]
    pub fn decode_tile<'s>(&self, t: usize, s: &'s mut DemScratch) -> Option<&'s [i32]> {
        let log2 = (0..self.grids)
            .filter_map(|g| self.grid(g))
            .find(|g| {
                let first = g.first_tile as usize;
                t >= first && t < first + (g.tile_rows as usize) * (g.tile_cols as usize)
            })?
            .tile_log2;
        if !(MIN_LOG2..=MAX_LOG2).contains(&log2) {
            return None;
        }
        self.coder.start(self.code(t)?, t, log2, s)?;
        let n = 1 << (2 * log2);
        if self.coder.run(s, n).is_none() {
            s.tile = None;
            return None;
        }
        s.q.get(..n)
    }

    /// Checks everything a reader relies on: the grids (tile sides 16 to 64, positive steps,
    /// a known datum, inside the globe, tile counts that add up), the offsets, the model family
    /// (every model sums to 4,096), the thresholds, and every present tile decoded whole with
    /// its rANS state back at its start and every byte used. O(samples).
    ///
    /// # Errors
    /// [`Error::Payload`] naming the first column that fails.
    pub fn check(&self) -> Result<(), Error> {
        let mut first = 0_u64;
        for g in 0..self.grids {
            let grid = self
                .grid(g)
                .ok_or_else(|| fail(TILE_ROW0, "a grid is unreadable"))?;
            if u64::from(grid.first_tile) != first {
                return Err(fail(
                    TILE_START,
                    "a grid does not start after the one before",
                ));
            }
            first += u64::from(grid.tile_rows) * u64::from(grid.tile_cols);
            if !(MIN_LOG2..=MAX_LOG2).contains(&grid.tile_log2) {
                return Err(fail(TILE_LOG2, "a tile side is not 16, 32 or 64"));
            }
            if grid.lat_step == 0 || grid.lon_step == 0 || grid.step_mm == 0 {
                return Err(fail(LAT_STEP, "a step of zero"));
            }
            if grid.datum != DATUM_EGM2008 {
                return Err(fail(DATUM, "an unknown datum"));
            }
            let side = u64::from(grid.tile_log2);
            let rows = (u64::from(grid.tile_row0) + u64::from(grid.tile_rows)) << side;
            let cols = (u64::from(grid.tile_col0) + u64::from(grid.tile_cols)) << side;
            // Samples reach 90° S (inclusive) and 360° of longitude, plus the last tile's slack.
            let max_rows = HALF_TURN / u64::from(grid.lat_step) + (1 << side);
            let max_cols = TURN / u64::from(grid.lon_step) + (1 << side);
            if rows > max_rows || cols > max_cols {
                return Err(fail(TILE_ROWS, "a grid reaches past the globe"));
            }
        }
        let last_start = self.cols.get(9).and_then(|c| c.get(self.grids));
        if first != self.tiles as u64 || last_start.map(u64::from) != Some(first) {
            return Err(fail(
                TILE_START,
                "the grids' tiles do not add up to the tile count",
            ));
        }
        let mut last = 0;
        for t in 0..=self.tiles {
            let o = self
                .offsets
                .get(t)
                .ok_or_else(|| fail(OFFSETS, "unreadable"))? as usize;
            if (t == 0 && o != 0) || o < last {
                return Err(fail(OFFSETS, "offsets do not start at 0 and increase"));
            }
            last = o;
        }
        let tail = self
            .coder
            .payload
            .get(last..)
            .ok_or_else(|| fail(OFFSETS, "the last offset is past the payload"))?;
        if tail.len() >= 4 || tail.iter().any(|&b| b != 0) {
            return Err(fail(PAYLOAD, "bytes after the last tile"));
        }
        for m in 0..FAMILY {
            if (0..SYMBOLS).map(|k| self.coder.freq(m, k)).sum::<u32>() != SCALE {
                return Err(fail(MODELS, "a model does not sum to 4096"));
            }
        }
        if self
            .coder
            .thresholds
            .windows(2)
            .any(|w| w.first() > w.get(1))
        {
            return Err(fail(CONTEXTS, "the thresholds decrease"));
        }
        let mut s = DemScratch::new();
        for t in 0..self.tiles {
            if self.code(t).is_none() {
                continue;
            }
            if self.decode_tile(t, &mut s).is_none() || s.state != RANS_L || s.pos != s.end {
                return Err(fail(PAYLOAD, "a tile does not decode to its end"));
            }
        }
        Ok(())
    }
}

#[cfg(feature = "write")]
pub use write::{Family, GridInput, THRESHOLDS, TileOptions, encode, encode_tile};

#[cfg(feature = "write")]
mod write {
    #![allow(
        clippy::indexing_slicing,
        reason = "the writer indexes its own arrays: tile samples, 5×5 systems, 64-entry models; an out-of-range index is a writer bug"
    )]

    use alloc::vec::Vec;

    use teigeodata_core::bitpack::encode_auto;
    use teigeodata_core::table::{Codec, encode_with};

    use super::{
        CONTEXT_COUNT, CONTEXTS, Coder, DATUM, DemScratch, ESCAPE, FAMILY, Grid, LAT_STEP,
        LON_STEP, MAX_ESCAPE, MAX_LOG2, MIN_LOG2, MODELS, OFFSETS, PAYLOAD, RANS_L, SCALE,
        SCALE_BITS, STEP_MM, SYMBOLS, TILE_COL0, TILE_COLS, TILE_LOG2, TILE_ROW0, TILE_ROWS,
        TILE_START, neighbours, predict, small_contexts, zigzag,
    };
    use crate::Error;

    /// The activity thresholds the forge writes (in height steps).
    pub const THRESHOLDS: [u32; 7] = [0, 1, 3, 6, 11, 20, 36];

    /// A model family: 64 models of 64 frequencies summing to 4,096, and each symbol's cost.
    #[derive(Clone, Debug)]
    pub struct Family {
        freqs: [[u32; SYMBOLS]; FAMILY],
        /// The [`MODELS`] column's bytes.
        bytes: Vec<u8>,
        /// −log2(f / 4096) in 1/65536 bits; `u32::MAX / 4` for a frequency of 0.
        cost: [[u32; SYMBOLS]; FAMILY],
    }

    /// `x` rounded to the nearest integer, halves away from zero (no `std` here), for
    /// `|x| < 2^62`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "callers pass |x| < 2^62; the remainder is exact"
    )]
    fn round(x: f64) -> i64 {
        let t = x as i64;
        let r = x - t as f64;
        if r >= 0.5 {
            t + 1
        } else if r <= -0.5 {
            t - 1
        } else {
            t
        }
    }

    /// √x for `x ≥ 1` by Newton's method in a fixed number of steps (no `std` here), so the
    /// result is the same on every machine.
    fn sqrt(x: f64) -> f64 {
        let mut y = x;
        for _ in 0..64 {
            y = f64::midpoint(y, x / y);
        }
        y
    }

    /// `log2(x)` in 1/65536, for `1 ≤ x < 2^32`, exactly by integer squaring.
    fn log2_q16(x: u32) -> u32 {
        let k = x.ilog2();
        // The mantissa in [1, 2) as Q31.
        let mut m = (u64::from(x) << (31 - k)) & 0xFFFF_FFFF;
        let mut frac = 0_u32;
        for bit in (0..16).rev() {
            m = (m * m) >> 31;
            if m >= 1 << 32 {
                m >>= 1;
                frac |= 1 << bit;
            }
        }
        (k << 16) | frac
    }

    impl Family {
        /// The standard family: model 0 codes only zeros; models 1 to 63 are discrete Laplace
        /// (two-sided geometric) laws whose mean |residual| grows geometrically from 0.02 to
        /// about 40 steps, the escape taking the tail. Integer frequencies by largest
        /// probability first, at least 1 each; every operation is IEEE +, −, ×, ÷ or √, so the
        /// family is the same on every machine.
        #[must_use]
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "probabilities times 4096 are in 0..=4096"
        )]
        pub fn standard() -> Self {
            let mut freqs = [[0_u32; SYMBOLS]; FAMILY];
            freqs[0][0] = SCALE;
            let mut mean = 0.02_f64;
            for model in freqs.iter_mut().skip(1) {
                let theta = (sqrt(1.0 + mean * mean) - 1.0) / mean;
                // P(r) = (1 − θ) / (1 + θ) · θ^|r|; zigzag z has |r| = (z + 1) / 2.
                let norm = (1.0 - theta) / (1.0 + theta);
                let mut p = [0.0_f64; SYMBOLS];
                let mut pow = 1.0;
                let mut total = 0.0;
                for (z, slot) in p.iter_mut().enumerate().take(SYMBOLS - 1) {
                    if z % 2 == 1 {
                        pow *= theta;
                    }
                    *slot = norm * pow;
                    total += *slot;
                }
                p[SYMBOLS - 1] = (1.0 - total).max(0.0);
                let mut sum = 0;
                for (f, &pz) in model.iter_mut().zip(&p) {
                    *f = (round(pz * f64::from(SCALE)) as u32).max(1);
                    sum += *f;
                }
                // The most likely symbol absorbs the rounding.
                let top = (0..SYMBOLS)
                    .max_by_key(|&z| (model[z], SYMBOLS - z))
                    .unwrap_or(0);
                model[top] = (model[top] + SCALE).saturating_sub(sum);
                mean *= 1.130_4;
            }
            let mut cost = [[0_u32; SYMBOLS]; FAMILY];
            for (c, f) in cost.iter_mut().zip(&freqs) {
                for (cz, &fz) in c.iter_mut().zip(f) {
                    *cz = if fz == 0 {
                        u32::MAX / 4
                    } else {
                        (SCALE_BITS << 16) - log2_q16(fz)
                    };
                }
            }
            let bytes = freqs
                .iter()
                .flatten()
                .flat_map(|&f| u16::try_from(f).unwrap_or(0).to_le_bytes())
                .collect();
            Self { freqs, bytes, cost }
        }

        fn cum(&self, m: usize, s: usize) -> u32 {
            self.freqs[m][..s].iter().sum()
        }
    }

    /// Least-squares coefficients (in 1/64, as i8) of the predictor over a tile's samples.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::needless_range_loop,
        reason = "sums of products below 2^53; coefficients clamped to i8; index loops over a 5×5 system"
    )]
    fn fit(q: &[i32], log2: u32) -> [i64; 5] {
        let mut ata = [[0_i64; 5]; 5];
        let mut atb = [0_i64; 5];
        for i in 1..q.len() {
            let [a, b, c, d, aa, bb] = neighbours(q, log2, i);
            let f = [b - a, c - a, d - a, aa - a, bb - a];
            let t = i64::from(q[i]) - a;
            for j in 0..5 {
                atb[j] = atb[j].saturating_add(f[j].saturating_mul(t));
                for k in 0..5 {
                    ata[j][k] = ata[j][k].saturating_add(f[j].saturating_mul(f[k]));
                }
            }
        }
        // Gaussian elimination with partial pivoting and a tiny ridge, in a fixed order.
        let mut m = [[0.0_f64; 6]; 5];
        let trace: f64 = (0..5).map(|j| ata[j][j] as f64).sum();
        for j in 0..5 {
            for k in 0..5 {
                m[j][k] = ata[j][k] as f64;
            }
            m[j][j] += trace * 1e-9 + 1e-6;
            m[j][5] = atb[j] as f64;
        }
        for col in 0..5 {
            let mut piv = col;
            for r in col + 1..5 {
                if m[r][col].abs() > m[piv][col].abs() {
                    piv = r;
                }
            }
            m.swap(col, piv);
            let d = m[col][col];
            if d == 0.0 {
                return [0; 5];
            }
            for r in 0..5 {
                if r != col {
                    let k = m[r][col] / d;
                    for c in col..6 {
                        m[r][c] -= k * m[col][c];
                    }
                }
            }
        }
        let mut out = [0_i64; 5];
        for j in 0..5 {
            let w = m[j][5] / m[j][j] * 64.0;
            out[j] = if w.is_finite() {
                round(w).clamp(-128, 127)
            } else {
                0
            };
        }
        out
    }

    /// The cheapest model per context and the total cost (1/65536 bits, escapes' extra bytes
    /// left out: they cost the same under every model); an empty context takes model 0.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "model indices are below 64"
    )]
    fn choose(
        counts: &[[u64; SYMBOLS]; CONTEXT_COUNT],
        family: &Family,
    ) -> ([u8; CONTEXT_COUNT], u64) {
        let mut models = [0_u8; CONTEXT_COUNT];
        let mut total = 0_u64;
        for (k, m) in models.iter_mut().enumerate() {
            let mut best = (u64::MAX, 0);
            for (id, cost) in family.cost.iter().enumerate() {
                let bits: u64 = counts[k]
                    .iter()
                    .zip(cost)
                    .map(|(&n, &c)| n.saturating_mul(u64::from(c)))
                    .fold(0, u64::saturating_add);
                if bits < best.0 {
                    best = (bits, id);
                }
            }
            *m = best.1 as u8;
            total = total.saturating_add(best.0);
        }
        (models, total)
    }

    /// The coded cost of a tile under coefficients `coef`, as [`choose`] counts it, plus 24
    /// bits per escape.
    fn tile_cost(
        q: &[i32],
        log2: u32,
        coef: &[i64; 5],
        family: &Family,
        thresholds: &[u32; 7],
    ) -> u64 {
        let mut counts = [[0_u64; SYMBOLS]; CONTEXT_COUNT];
        let mut escapes = 0_u64;
        for i in 1..q.len() {
            let nb = neighbours(q, log2, i);
            let (p, k) = predict(&nb, coef, thresholds);
            let z = zigzag(i64::from(q[i]) - p);
            if z >= u64::from(ESCAPE) {
                escapes += 1;
            }
            counts[k][usize::try_from(z.min(u64::from(ESCAPE))).unwrap_or(SYMBOLS - 1)] += 1;
        }
        choose(&counts, family)
            .1
            .saturating_add(escapes * (24 << 16))
    }

    /// Coordinate descent on the coded cost from the least-squares coefficients: each
    /// coefficient tries ±4, ±2 and ±1 while that lowers the cost, at most 4 rounds. Least
    /// squares fits the mean square; the coder pays for the Laplacian spread, which this
    /// minimises directly. Deterministic: integers only.
    fn refine(
        q: &[i32],
        log2: u32,
        mut coef: [i64; 5],
        family: &Family,
        thresholds: &[u32; 7],
    ) -> [i64; 5] {
        let mut best = tile_cost(q, log2, &coef, family, thresholds);
        for _ in 0..4 {
            let before = best;
            for j in 0..5 {
                for step in [4, 2, 1] {
                    for dir in [-1, 1] {
                        loop {
                            let mut c = coef;
                            c[j] = (c[j] + dir * step).clamp(-128, 127);
                            if c[j] == coef[j] {
                                break;
                            }
                            let cost = tile_cost(q, log2, &c, family, thresholds);
                            if cost < best {
                                best = cost;
                                coef = c;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            if best == before {
                break;
            }
        }
        coef
    }

    /// Encoder choices that do not change the format.
    #[derive(Clone, Copy, Debug)]
    pub struct TileOptions {
        /// Refine the least-squares coefficients on the coded cost.
        pub refine: bool,
    }

    impl Default for TileOptions {
        fn default() -> Self {
            Self { refine: true }
        }
    }

    /// Codes one tile of quantised heights (`q`, row-major, `1 << log2` a side) as the module
    /// comment describes.
    ///
    /// # Errors
    /// [`Error::Write`] if the side is not 16, 32 or 64, `q` has the wrong length, or a
    /// residual is too large to escape (2^24 + 62).
    #[allow(
        clippy::missing_panics_doc,
        clippy::cast_possible_truncation,
        reason = "indices come from the tile's own arrays; symbols and model indices are below 64"
    )]
    pub fn encode_tile(
        q: &[i32],
        log2: u32,
        family: &Family,
        thresholds: &[u32; 7],
        opts: &TileOptions,
    ) -> Result<Vec<u8>, Error> {
        if !(MIN_LOG2..=MAX_LOG2).contains(&log2) || q.len() != 1 << (2 * log2) {
            return Err(Error::Write(
                "a DEM tile is not 16, 32 or 64 samples square",
            ));
        }
        let mut coef = fit(q, log2);
        if opts.refine {
            coef = refine(q, log2, coef, family, thresholds);
        }
        // Each sample's context and zigzag residual.
        let mut ctx = Vec::with_capacity(q.len());
        let mut zs = Vec::with_capacity(q.len());
        let mut counts = [[0_u64; SYMBOLS]; CONTEXT_COUNT];
        for i in 1..q.len() {
            let nb = neighbours(q, log2, i);
            let (p, k) = predict(&nb, &coef, thresholds);
            let z = zigzag(i64::from(q[i]) - p);
            if z >= u64::from(ESCAPE) + MAX_ESCAPE {
                return Err(Error::Write("a DEM residual too large to escape"));
            }
            counts[k][usize::try_from(z.min(u64::from(ESCAPE))).unwrap_or(SYMBOLS - 1)] += 1;
            ctx.push(k as u8);
            zs.push(z);
        }
        let (models, _) = choose(&counts, family);
        // rANS, backwards: each symbol's (cumulative, frequency), in decode order.
        let mut syms: Vec<(u32, u32)> = Vec::with_capacity(zs.len() + 8);
        for (&z, &k) in zs.iter().zip(&ctx) {
            let m = usize::from(models[usize::from(k)]);
            let s = usize::try_from(z.min(u64::from(ESCAPE))).unwrap_or(SYMBOLS - 1);
            syms.push((family.cum(m, s), family.freqs[m][s]));
            if s == ESCAPE as usize {
                let e = z - u64::from(ESCAPE);
                for shift in [16, 8, 0] {
                    syms.push((((e >> shift) & 255) as u32 * 16, 16));
                }
            }
        }
        let mut x = RANS_L;
        let mut out: Vec<u8> = Vec::new();
        for &(cum, freq) in syms.iter().rev() {
            if freq == 0 {
                return Err(Error::Write("a DEM symbol with no frequency"));
            }
            let x_max = ((RANS_L >> SCALE_BITS) << 8) * freq;
            while x >= x_max {
                out.push(x as u8);
                x >>= 8;
            }
            x = ((x / freq) << SCALE_BITS) + (x % freq) + cum;
        }
        out.reverse();
        let mut code = Vec::with_capacity(out.len() + 24);
        let mut base = zigzag(i64::from(q[0]));
        loop {
            let b = (base & 0x7f) as u8;
            base >>= 7;
            if base == 0 {
                code.push(b);
                break;
            }
            code.push(b | 0x80);
        }
        code.extend(coef.iter().map(|&c| (c as i8).cast_unsigned()));
        let bits = models
            .iter()
            .enumerate()
            .fold(0_u64, |acc, (k, &m)| acc | (u64::from(m) << (6 * k)));
        code.extend_from_slice(&bits.to_le_bytes()[..6]);
        code.extend_from_slice(&x.to_le_bytes());
        code.extend_from_slice(&out);
        // Read back: the code must decode to `q` and end where it started.
        let coder = Coder {
            models: &family.bytes,
            thresholds: *thresholds,
            small: small_contexts(thresholds),
            payload: &code,
        };
        let mut s = DemScratch::new();
        let n = q.len();
        let ok = coder.start((0, code.len()), 0, log2, &mut s).is_some()
            && coder.run(&mut s, n).is_some()
            && s.state == RANS_L
            && s.pos == code.len()
            && s.q.get(..n) == Some(q);
        if !ok {
            return Err(Error::Write("a DEM tile does not decode to its samples"));
        }
        Ok(code)
    }

    /// One grid for [`encode`]: its record (`first_tile` is ignored) and its tiles' codes,
    /// row-major over its box of tiles, `None` for an absent tile.
    #[derive(Clone, Copy, Debug)]
    pub struct GridInput<'g> {
        /// The grid's record.
        pub grid: Grid,
        /// Each tile's code from [`encode_tile`], or `None`.
        pub tiles: &'g [Option<&'g [u8]>],
    }

    /// Builds the section from coded grids.
    ///
    /// # Errors
    /// [`Error::Write`] if a grid's tile count is not its rows × columns, or the section is
    /// too large for u32 offsets.
    pub fn encode(
        grids: &[GridInput<'_>],
        family: &Family,
        thresholds: &[u32; 7],
    ) -> Result<Vec<u8>, Error> {
        let too_big = || Error::Write("a DEM section above 4 GiB");
        let mut cols: [Vec<u32>; 10] = Default::default();
        let mut offsets = alloc::vec![0_u32];
        let mut payload = Vec::new();
        let mut first = 0_u32;
        for g in grids {
            let r = &g.grid;
            if g.tiles.len() as u64 != u64::from(r.tile_rows) * u64::from(r.tile_cols) {
                return Err(Error::Write("a DEM grid's tiles are not rows × columns"));
            }
            for (col, v) in cols.iter_mut().zip([
                r.tile_row0,
                r.tile_col0,
                r.tile_rows,
                r.tile_cols,
                r.lat_step,
                r.lon_step,
                r.step_mm,
                r.datum,
                r.tile_log2,
                first,
            ]) {
                col.push(v);
            }
            for t in g.tiles {
                if let Some(code) = t {
                    payload.extend_from_slice(code);
                }
                offsets.push(u32::try_from(payload.len()).map_err(|_| too_big())?);
            }
            first = u32::try_from(offsets.len() - 1).map_err(|_| too_big())?;
        }
        cols[9].push(first);
        while payload.len() % 4 != 0 {
            payload.push(0);
        }
        let ids = [
            TILE_ROW0, TILE_COL0, TILE_ROWS, TILE_COLS, LAT_STEP, LON_STEP, STEP_MM, DATUM,
            TILE_LOG2, TILE_START,
        ];
        let mut bodies: Vec<(u16, Codec, Vec<u8>)> = ids
            .iter()
            .zip(&cols)
            .map(|(&id, v)| (id, Codec::Bitpack, encode_auto(v)))
            .collect();
        bodies.push((OFFSETS, Codec::Bitpack, encode_auto(&offsets)));
        bodies.push((MODELS, Codec::Bytes, family.bytes.clone()));
        bodies.push((CONTEXTS, Codec::Bitpack, encode_auto(thresholds)));
        bodies.push((PAYLOAD, Codec::Bytes, payload));
        let refs: Vec<(u16, Codec, &[u8])> = bodies
            .iter()
            .map(|(id, c, b)| (*id, *c, b.as_slice()))
            .collect();
        encode_with(&refs).map_err(|_| Error::Write("the DEM column table"))
    }

    #[cfg(test)]
    mod tests {
        use super::log2_q16;

        #[test]
        fn log2_is_exact_at_powers_and_close_between() {
            assert_eq!(log2_q16(1), 0);
            assert_eq!(log2_q16(4096), 12 << 16);
            // log2(3) = 1.584962500721156: 103,872.9 in 1/65536.
            assert!((i64::from(log2_q16(3)) - 103_872).abs() <= 1);
        }
    }
}
