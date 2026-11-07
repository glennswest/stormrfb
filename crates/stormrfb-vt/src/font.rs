//! The embedded console font: Terminus 8x16 (SIL OFL 1.1, `assets/`), a
//! PSF2 file with its unicode table, as stormpanel embeds it.

use std::collections::HashMap;
use std::sync::OnceLock;

const TERMINUS_16: &[u8] = include_bytes!("../assets/terminus-16.psf");

pub struct Font {
    pub width: usize,
    pub height: usize,
    bytes_per_row: usize,
    bytes_per_glyph: usize,
    glyphs: &'static [u8],
    count: usize,
    map: HashMap<char, usize>,
}

impl Font {
    /// The built-in font, parsed once.
    pub fn get() -> &'static Font {
        static F: OnceLock<Font> = OnceLock::new();
        F.get_or_init(|| Font::parse(TERMINUS_16).expect("the embedded PSF2 font is valid"))
    }

    pub fn parse(d: &'static [u8]) -> Option<Font> {
        let u = |i: usize| -> Option<usize> {
            Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?) as usize)
        };
        if d.get(0..4)? != [0x72, 0xb5, 0x4a, 0x86] {
            return None;
        }
        let (hsize, flags, count, bpg, height, width) =
            (u(8)?, u(12)?, u(16)?, u(20)?, u(24)?, u(28)?);
        let bytes_per_row = width.div_ceil(8);
        if bpg != bytes_per_row * height || count == 0 {
            return None;
        }
        let end = hsize + count * bpg;
        let glyphs = d.get(hsize..end)?;
        let mut map = HashMap::new();
        if flags & 1 != 0 {
            // Per glyph: UTF-8 sequences, 0xFE starts a multi-char sequence
            // (ignored), 0xFF ends the glyph's entry.
            let mut i = end;
            for g in 0..count {
                let start = i;
                while i < d.len() && d[i] != 0xff {
                    i += 1;
                }
                let entry = &d[start..i.min(d.len())];
                i += 1;
                let single = entry.split(|&b| b == 0xfe).next().unwrap_or(&[]);
                if let Ok(s) = std::str::from_utf8(single) {
                    for c in s.chars() {
                        map.entry(c).or_insert(g);
                    }
                }
            }
        } else {
            for g in 0..count.min(256) {
                map.insert(char::from(g as u8), g);
            }
        }
        Some(Font {
            width,
            height,
            bytes_per_row,
            bytes_per_glyph: bpg,
            glyphs,
            count,
            map,
        })
    }

    /// The bitmap of `c` (the font's '?' when it lacks it).
    pub fn glyph(&self, c: char) -> &[u8] {
        let g = self
            .map
            .get(&c)
            .or_else(|| self.map.get(&'?'))
            .copied()
            .unwrap_or(0)
            .min(self.count - 1);
        &self.glyphs[g * self.bytes_per_glyph..(g + 1) * self.bytes_per_glyph]
    }

    /// Whether pixel (`x`, `y`) of a glyph is set.
    pub fn bit(glyph: &[u8], bytes_per_row: usize, x: usize, y: usize) -> bool {
        glyph[y * bytes_per_row + x / 8] & (0x80 >> (x % 8)) != 0
    }

    pub fn bytes_per_row(&self) -> usize {
        self.bytes_per_row
    }

    pub fn has(&self, c: char) -> bool {
        self.map.contains_key(&c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_font_is_8x16_with_ascii() {
        let f = Font::get();
        assert_eq!((f.width, f.height), (8, 16));
        for c in ' '..='~' {
            assert!(f.has(c), "missing {c:?}");
        }
        assert!(f.glyph('A').iter().any(|&b| b != 0));
        assert!(f.glyph(' ').iter().all(|&b| b == 0));
    }

    #[test]
    fn refuses_a_file_that_is_not_psf2() {
        assert!(Font::parse(b"not a font at all, no magic here").is_none());
    }
}
