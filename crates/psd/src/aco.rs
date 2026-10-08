//! Photoshop colour swatches (`.aco`, and the `Swatches.psp` preferences file, which is an `.aco`
//! followed by an `8BIMphry` preset hierarchy naming the swatch groups).
//!
//! Layout (Adobe's public "Photoshop File Formats Specification", Color Swatches): a version 1
//! section (u16 version, u16 count, count × [u16 space, 4 × u16]) optionally followed by a
//! version 2 section with the same colours plus a unicode name each. Spaces: 0 RGB, 1 HSB,
//! 2 CMYK, 7 Lab, 8 Grayscale (others are kept but have no RGB conversion).

use crate::error::{PsdError, Result};
use crate::io::{Reader, read_unicode_units};

/// Most colours read from one file.
pub const MAX_COLORS: u16 = u16::MAX;

/// One swatch.
#[derive(Debug, Clone, PartialEq)]
pub struct AcoColor {
    /// Name (version 2 files; empty in version 1 only files).
    pub name: String,
    /// Colour space id (0 RGB, 1 HSB, 2 CMYK, 7 Lab, 8 Gray…).
    pub space: u16,
    /// Raw component words.
    pub values: [u16; 4],
    /// Group path from the preset hierarchy (`Swatches.psp`), empty when ungrouped.
    pub group: String,
}

impl AcoColor {
    /// sRGB-ish 8-bit RGB for display, `None` for spaces without a conversion. CMYK and Lab use
    /// simple textbook formulas (no colour management), good enough for a swatch chip.
    pub fn rgb8(&self) -> Option<[u8; 3]> {
        let [w, x, y, z] = self.values;
        let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        match self.space {
            0 => Some([(w >> 8) as u8, (x >> 8) as u8, (y >> 8) as u8]),
            1 => {
                let (h, s, v) = (f64::from(w) / 65535.0 * 6.0, f64::from(x) / 65535.0, f64::from(y) / 65535.0);
                let i = (h.floor() as i32).rem_euclid(6);
                let f = h - h.floor();
                let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
                let (r, g, b) = match i {
                    0 => (v, t, p),
                    1 => (q, v, p),
                    2 => (p, v, t),
                    3 => (p, q, v),
                    4 => (t, p, v),
                    _ => (v, p, q),
                };
                Some([to8(r), to8(g), to8(b)])
            }
            2 => {
                // Stored inverted: 0 = 100 % ink.
                let ink = |v: u16| 1.0 - f64::from(v) / 65535.0;
                let k = ink(z);
                Some([to8((1.0 - ink(w)) * (1.0 - k)), to8((1.0 - ink(x)) * (1.0 - k)), to8((1.0 - ink(y)) * (1.0 - k))])
            }
            7 => {
                let l = f64::from(w) / 100.0;
                let (a, b) = (f64::from(x as i16) / 100.0, f64::from(y as i16) / 100.0);
                let fy = (l + 16.0) / 116.0;
                let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
                let inv = |t: f64| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f64 / 29.0).powi(2) * (t - 4.0 / 29.0) };
                let (xx, yy, zz) = (0.950_47 * inv(fx), inv(fy), 1.088_83 * inv(fz));
                let lin = [3.2406 * xx - 1.5372 * yy - 0.4986 * zz, -0.9689 * xx + 1.8758 * yy + 0.0415 * zz, 0.0557 * xx - 0.2040 * yy + 1.0570 * zz];
                let gamma = |c: f64| if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
                Some([to8(gamma(lin[0])), to8(gamma(lin[1])), to8(gamma(lin[2]))])
            }
            8 => Some([to8(1.0 - f64::from(w) / 10000.0); 3]),
            _ => None,
        }
    }
}

fn read_section(r: &mut Reader<'_>, named: bool) -> Result<Vec<AcoColor>> {
    let n = r.u16()?;
    r.check_count(u64::from(n), if named { 14 } else { 10 })?;
    let mut out = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        let space = r.u16()?;
        let values = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
        let name = if named { String::from_utf16_lossy(&read_unicode_units(r)?).trim_end_matches('\0').to_string() } else { String::new() };
        out.push(AcoColor { name, space, values, group: String::new() });
    }
    Ok(out)
}

/// Parses an `.aco` / `Swatches.psp`. Names come from the version 2 section when present; group
/// names from a trailing preset hierarchy when present. Never panics.
pub fn parse(data: &[u8]) -> Result<Vec<AcoColor>> {
    let mut r = Reader::new(data);
    let mut colors = match r.u16()? {
        1 => read_section(&mut r, false)?,
        2 => return read_section(&mut r, true),
        v => return Err(PsdError::invalid(format!("swatch file version {v}"))),
    };
    if r.remaining() >= 4 && r.u16().ok() == Some(2) {
        // A damaged version 2 section keeps the version 1 colours (unnamed).
        let mut probe = r.clone();
        if let Ok(named) = read_section(&mut probe, true)
            && named.len() == colors.len()
        {
            colors = named;
            r = probe;
        }
    }
    let rest = r.peek_rest();
    if let Some(at) = rest.windows(8).position(|w| w == b"8BIMphry") {
        let body = rest.get(at + 12..).unwrap_or_default();
        let len = rest.get(at + 8..at + 12).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let groups = crate::phry::groups(body.get(..len.min(body.len())).unwrap_or_default());
        for (c, g) in colors.iter_mut().zip(groups) {
            c.group = g;
        }
    }
    Ok(colors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::WriteExt;

    fn sample() -> Vec<u8> {
        let mut b = Vec::new();
        let cols: [(u16, [u16; 4], &str); 3] =
            [(0, [0x3d3d, 0x4646, 0x3333, 0], "bwald"), (8, [10000, 0, 0, 0], "Black"), (2, [65535, 0, 0, 65535], "CMYK Red")];
        b.put_u16(1);
        b.put_u16(3);
        for (s, v, _) in cols {
            b.put_u16(s);
            v.iter().for_each(|x| b.put_u16(*x));
        }
        b.put_u16(2);
        b.put_u16(3);
        for (s, v, n) in cols {
            b.put_u16(s);
            v.iter().for_each(|x| b.put_u16(*x));
            let u: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
            b.put_u32(u.len() as u32);
            u.iter().for_each(|x| b.put_u16(*x));
        }
        b
    }

    #[test]
    fn reads_names_and_converts() {
        let c = parse(&sample()).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].name, "bwald");
        assert_eq!(c[0].rgb8(), Some([0x3d, 0x46, 0x33]));
        assert_eq!(c[1].rgb8(), Some([0, 0, 0]));
        assert_eq!(c[2].rgb8(), Some([255, 0, 0]), "0 = 100 % ink: magenta + yellow");
    }

    #[test]
    fn truncated_and_garbage_never_panic() {
        let full = sample();
        for cut in 0..full.len() {
            let _ = parse(&full[..cut]);
        }
        // Version 1 alone still parses (unnamed).
        let v1 = parse(&full[..4 + 3 * 10]).unwrap();
        assert_eq!(v1.len(), 3);
        assert!(v1[0].name.is_empty());
        let mut x = 7u32;
        for len in [1usize, 2, 9, 40, 300] {
            let g: Vec<u8> = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (x >> 16) as u8
                })
                .collect();
            let _ = parse(&g);
        }
        // Every colour space converts or declines without panicking.
        for space in 0..12 {
            let _ = AcoColor { name: String::new(), space, values: [u16::MAX, 0, u16::MAX, 1], group: String::new() }.rgb8();
        }
    }
}
