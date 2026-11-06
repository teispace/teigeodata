//! Geoid undulation N from a `GeographicLib` PGM grid (elevation-reference note §3): 16-bit
//! big-endian pixels with `Offset` and `Scale` header comments, N = offset + scale · pixel.
//! Rows run from 90° N to 90° S (spacing 180°/(rows − 1)); columns from 0° E eastwards
//! (spacing 360°/columns, wrapping). Bilinear: for `egm2008-5` the error is RMS 12 mm, at most
//! 0.48 m. An ellipsoidal height h becomes orthometric as H = h − N.

use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;

pub(crate) struct Geoid {
    offset: f64,
    scale: f64,
    w: usize,
    h: usize,
    px: Vec<u16>,
}

impl Geoid {
    /// Reads the first `.pgm` member of a zip (the `GeographicLib` distribution).
    pub(crate) fn open_zip(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let name = zip
            .file_names()
            .filter_map(Result::ok)
            .find(|n| n.ends_with(".pgm"))
            .map(std::borrow::Cow::into_owned)
            .ok_or("no .pgm in the geoid archive")?;
        let mut bytes = Vec::new();
        zip.by_name(&name)
            .map_err(|e| e.to_string())?
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Self::parse(&bytes)
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        // Header: `P5`, comment lines, `w h`, `maxval`, one whitespace byte, then pixels.
        let (mut offset, mut scale) = (None, None);
        let mut nums: Vec<usize> = Vec::new();
        let mut at = 0;
        while nums.len() < 3 {
            let end = bytes
                .get(at..)
                .and_then(|b| b.iter().position(|&c| c == b'\n'))
                .ok_or("a truncated header")?;
            let line =
                String::from_utf8_lossy(bytes.get(at..at + end).unwrap_or_default()).into_owned();
            at += end + 1;
            if let Some(c) = line.strip_prefix('#') {
                let mut f = c.split_whitespace();
                match (f.next(), f.next().and_then(|v| v.parse::<f64>().ok())) {
                    (Some("Offset"), Some(v)) => offset = Some(v),
                    (Some("Scale"), Some(v)) => scale = Some(v),
                    _ => {}
                }
            } else if line != "P5" {
                nums.extend(
                    line.split_whitespace()
                        .filter_map(|v| v.parse::<usize>().ok()),
                );
            }
        }
        let (Some(&w), Some(&h), Some(&maxval)) = (nums.first(), nums.get(1), nums.get(2)) else {
            return Err("a malformed header".into());
        };
        if maxval != 65535 || w == 0 || h < 2 {
            return Err(format!("unsupported grid {w}×{h}, maxval {maxval}"));
        }
        let body = bytes.get(at..at + 2 * w * h).ok_or("a truncated grid")?;
        let px = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&p| u16::from_be_bytes(p))
            .collect();
        Ok(Self {
            offset: offset.ok_or("no Offset in the header")?,
            scale: scale.ok_or("no Scale in the header")?,
            w,
            h,
            px,
        })
    }

    fn at(&self, row: usize, col: usize) -> f64 {
        let v = self
            .px
            .get(row.min(self.h - 1) * self.w + col % self.w)
            .copied()
            .unwrap_or(0);
        self.offset + self.scale * f64::from(v)
    }

    /// N in metres at (lat, lon) in degrees, bilinear.
    pub(crate) fn undulation(&self, lat: f64, lon: f64) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "grid sizes")]
        let (fw, fh) = (self.w as f64, (self.h - 1) as f64);
        let y = ((90.0 - lat.clamp(-90.0, 90.0)) / 180.0 * fh).clamp(0.0, fh);
        let x = lon.rem_euclid(360.0) / 360.0 * fw;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "non-negative, in the grid"
        )]
        let (row, col) = (y.floor() as usize, x.floor() as usize);
        #[allow(clippy::cast_precision_loss, reason = "grid indices")]
        let (ty, tx) = (y - row as f64, x - col as f64);
        let top = self.at(row, col) * (1.0 - tx) + self.at(row, col + 1) * tx;
        let bottom = self.at(row + 1, col) * (1.0 - tx) + self.at(row + 1, col + 1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// `forge geoid <source> <lat> <lon> [<lat> <lon> ...]`: N at each point, for checks.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<String, String> {
    let (source, coords) = args
        .split_first()
        .ok_or("usage: forge geoid <source> <lat> <lon> [...]")?;
    let lock = crate::lock::Lock::read(&root.join("data/sources.lock"))?;
    let g = Geoid::open_zip(
        &root
            .join("data/cache/store/sha256")
            .join(&lock.get(source)?.sha256),
    )?;
    let v: Vec<f64> = coords
        .iter()
        .map(|c| c.parse::<f64>().map_err(|e| format!("`{c}`: {e}")))
        .collect::<Result<_, _>>()?;
    let mut out = String::new();
    for p in v.chunks(2) {
        if let [lat, lon] = p {
            let _ = writeln!(out, "{lat} {lon} {:.3}", g.undulation(*lat, *lon));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::Geoid;

    #[test]
    fn parses_and_interpolates() {
        // A 4×3 grid (90° spacing in both axes): rows 90N, 0, 90S; pixel = row·10 + col.
        let mut b = b"P5\n# Offset -100\n# Scale 0.5\n4 3\n65535\n".to_vec();
        for row in 0..3_u16 {
            for col in 0..4_u16 {
                b.extend((row * 10 + col).to_be_bytes());
            }
        }
        let g = Geoid::parse(&b).unwrap_or_else(|e| panic!("{e}"));
        let n = |lat, lon| g.undulation(lat, lon);
        assert!((n(90.0, 0.0) - -100.0).abs() < 1e-9);
        assert!((n(0.0, 90.0) - (-100.0 + 0.5 * 11.0)).abs() < 1e-9);
        // Half-way in both axes between rows 0–1 and columns 1–2: pixel 6.5.
        assert!((n(45.0, 135.0) - (-100.0 + 0.5 * 6.5)).abs() < 1e-9);
        // Longitude wraps: 315° is between column 3 and column 0.
        assert!((n(0.0, -45.0) - (-100.0 + 0.5 * 11.5)).abs() < 1e-9);
        assert!(Geoid::parse(b"P5\n4 3\n255\n").is_err());
    }
}
