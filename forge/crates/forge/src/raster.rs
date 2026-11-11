//! `GeoTIFF` rasters on a latitude-longitude grid: the georeferencing every reader shares, and a
//! reader that visits the cells inside a box one internal chunk (tile or strip) at a time, so
//! memory stays at one chunk whatever the file's size.
//!
//! The grid comes from `ModelPixelScale`, `ModelTiepoint` and the `GeoKey` raster type
//! (`PixelIsPoint` tiles such as Copernicus are not shifted by half a pixel). Classic TIFF and
//! `BigTIFF`, LZW or DEFLATE, with or without a horizontal predictor (the `tiff` crate).

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

/// A raster's grid: pixel centres at `lon0 + i·dx`, `lat0 − j·dy`, `w` × `h` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Georef {
    pub(crate) lon0: f64,
    pub(crate) lat0: f64,
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) w: usize,
    pub(crate) h: usize,
}

/// Opens `path` and reads its grid.
pub(crate) fn open(path: &Path) -> Result<(Decoder<BufReader<File>>, Georef), String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    open_reader(BufReader::new(file), &path.display().to_string())
}

/// Opens a raster from any seekable reader (a file, or HTTP ranges of a cloud-optimised one)
/// and reads its grid; `what` names it in errors. For a projected raster (UTM) the grid's
/// `lon0`, `lat0` are its easting and northing.
pub(crate) fn open_reader<R: Read + Seek>(
    reader: R,
    what: &str,
) -> Result<(Decoder<R>, Georef), String> {
    let err = |e: tiff::TiffError| format!("{what}: {e}");
    let mut d = Decoder::new(reader).map_err(err)?;
    let (w, h) = d.dimensions().map_err(err)?;
    let scale = d.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(err)?;
    let tie = d.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(err)?;
    let keys = d
        .get_tag_u16_vec(Tag::GeoKeyDirectoryTag)
        .unwrap_or_default();
    let (Some(&dx), Some(&dy), Some(&i0), Some(&j0), Some(&x0), Some(&y0)) = (
        scale.first(),
        scale.get(1),
        tie.first(),
        tie.get(1),
        tie.get(3),
        tie.get(4),
    ) else {
        return Err(format!("{what}: missing georeferencing tags"));
    };
    if !(dx > 0.0 && dy > 0.0) {
        return Err(format!("{what}: pixel size is not positive"));
    }
    // GeoKey 1025 (GTRasterTypeGeoKey): 1 = PixelIsArea (tie point at a pixel's corner), 2 =
    // PixelIsPoint (at its centre). Keys are 4-tuples after a 4-short header.
    let is_point = keys
        .chunks(4)
        .skip(1)
        .any(|k| k.first() == Some(&1025) && k.get(3) == Some(&2));
    let half = if is_point { 0.0 } else { 0.5 };
    let g = Georef {
        lon0: x0 + (half - i0) * dx,
        lat0: y0 - (half - j0) * dy,
        dx,
        dy,
        w: usize::try_from(w).unwrap_or(0),
        h: usize::try_from(h).unwrap_or(0),
    };
    Ok((d, g))
}

/// Calls `f(lon, lat, value)` for every pixel centre inside the box `(min_lon, min_lat,
/// max_lon, max_lat)` (degrees) whose value is finite and positive (no-data values are
/// negative or not finite in the population grids). Reads only the chunks that meet the box,
/// one at a time.
pub(crate) fn for_each_positive(
    path: &Path,
    (min_lon, min_lat, max_lon, max_lat): (f64, f64, f64, f64),
    mut visit: impl FnMut(f64, f64, f64),
) -> Result<(), String> {
    let err = |e: tiff::TiffError| format!("{}: {e}", path.display());
    let (mut dec, g) = open(path)?;
    // The pixel columns and rows whose centres can be inside the box.
    #[allow(clippy::cast_precision_loss, reason = "a raster's pixel count")]
    let (wf, hf) = (g.w as f64, g.h as f64);
    let col = |lon: f64| ((lon - g.lon0) / g.dx).clamp(-1.0, wf);
    let row = |lat: f64| ((g.lat0 - lat) / g.dy).clamp(-1.0, hf);
    let (c0, c1) = (col(min_lon).ceil(), col(max_lon).floor());
    let (r0, r1) = (row(max_lat).ceil(), row(min_lat).floor());
    if c1 < c0.max(0.0) || r1 < r0.max(0.0) {
        return Ok(());
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to the raster's pixel range"
    )]
    let (c0, c1, r0, r1) = (
        c0.max(0.0) as usize,
        (c1 as usize).min(g.w.saturating_sub(1)),
        r0.max(0.0) as usize,
        (r1 as usize).min(g.h.saturating_sub(1)),
    );
    let (cw, ch) = dec.chunk_dimensions();
    let (cw, ch) = (cw as usize, ch as usize);
    if cw == 0 || ch == 0 {
        return Err(format!("{}: empty chunks", path.display()));
    }
    let across = g.w.div_ceil(cw);
    for cy in r0 / ch..=r1 / ch {
        for cx in c0 / cw..=c1 / cw {
            let index = u32::try_from(cy * across + cx).map_err(|e| e.to_string())?;
            // The chunk's data width, without the padding of the last column of chunks.
            let width = dec.chunk_data_dimensions(index).0 as usize;
            let values: Vec<f64> = match dec.read_chunk(index).map_err(err)? {
                DecodingResult::F64(v) => v,
                DecodingResult::F32(v) => v.into_iter().map(f64::from).collect(),
                _ => {
                    return Err(format!(
                        "{}: not a Float32 or Float64 raster",
                        path.display()
                    ));
                }
            };
            if width == 0 {
                continue;
            }
            for (k, &v) in values.iter().enumerate() {
                if !(v.is_finite() && v > 0.0) {
                    continue;
                }
                let (i, j) = (cx * cw + k % width, cy * ch + k / width);
                if !(c0..=c1).contains(&i) || !(r0..=r1).contains(&j) {
                    continue;
                }
                #[allow(clippy::cast_precision_loss, reason = "pixel indices")]
                visit(g.lon0 + i as f64 * g.dx, g.lat0 - j as f64 * g.dy, v);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{for_each_positive, open};
    use tiff::encoder::{TiffEncoder, colortype::Gray32Float};
    use tiff::tags::Tag;

    /// A 5 × 4 `Float32` grid, `PixelIsArea`, 1° pixels from 80° E, 30° N, two rows per strip.
    fn write(path: &std::path::Path, values: &[f32]) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = TiffEncoder::new(file).unwrap();
        let mut img = enc.new_image::<Gray32Float>(5, 4).unwrap();
        img.rows_per_strip(2).unwrap();
        let dir = img.encoder();
        dir.write_tag(Tag::ModelPixelScaleTag, &[1.0_f64, 1.0, 0.0][..])
            .unwrap();
        dir.write_tag(
            Tag::ModelTiepointTag,
            &[0.0_f64, 0.0, 0.0, 80.0, 30.0, 0.0][..],
        )
        .unwrap();
        // GeoKey directory: version 1.1.0, one key: 1025 (raster type) = 1, PixelIsArea.
        dir.write_tag(
            Tag::GeoKeyDirectoryTag,
            &[1_u16, 1, 0, 1, 1025, 0, 1, 1][..],
        )
        .unwrap();
        img.write_data(values).unwrap();
    }

    #[test]
    fn positive_cells_in_the_box_are_visited_once_each() {
        let dir = std::env::temp_dir().join(format!("forge-raster-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grid.tif");
        #[rustfmt::skip]
        let values = [
            1.0, 2.0, 0.0, -99_999.0, 5.0,
            6.0, f32::NAN, 8.0, 9.0, 10.0,
            11.0, 12.0, 13.0, 14.0, 15.0,
            16.0, 17.0, -1.0, 19.0, 20.0,
        ];
        write(&path, &values);
        let (_, g) = open(&path).unwrap();
        assert_eq!((g.lon0, g.lat0, g.w, g.h), (80.5, 29.5, 5, 4));
        let mut seen = Vec::new();
        let mut visit = |b| {
            seen.clear();
            for_each_positive(&path, b, |lon, lat, v| seen.push((lon, lat, v))).unwrap();
            seen.clone()
        };
        // The whole grid: every finite positive cell, at its centre.
        let all = visit((70.0, 20.0, 90.0, 40.0));
        assert_eq!(all.len(), 16);
        assert_eq!(all.iter().map(|x| x.2).sum::<f64>(), 178.0);
        assert!(all.contains(&(80.5, 29.5, 1.0)) && all.contains(&(84.5, 26.5, 20.0)));
        // Columns 1..=3 and rows 1..=2 (centres 81.5–83.5 E, 27.5–28.5 N), across both strips.
        let inner = visit((81.2, 27.2, 83.8, 28.8));
        let v: Vec<f64> = inner.iter().map(|x| x.2).collect();
        assert_eq!(v, [8.0, 9.0, 12.0, 13.0, 14.0]);
        // A box beside the grid reads nothing.
        assert_eq!(visit((90.0, 20.0, 95.0, 25.0)), []);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
