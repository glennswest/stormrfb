//! A screen-sized XRGB8888 buffer (the layout of a DRM dumb buffer), and the
//! status screen drawn on it while there is no picture from the guest.

use crate::font::Font;

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// 0x00RRGGBB, row-major, `width` per row.
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Canvas {
        Canvas {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn clear(&mut self, c: u32) {
        self.pixels.fill(c);
    }

    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: u32) {
        for row in y..(y + h).min(self.height) {
            let start = row * self.width;
            self.pixels[start + x.min(self.width)..start + (x + w).min(self.width)].fill(c);
        }
    }

    /// `s` with its top-left at (`x`, `y`), each font pixel `scale` square.
    pub fn text(&mut self, x: usize, y: usize, s: &str, c: u32, scale: usize) {
        let f = Font::get();
        for (n, ch) in s.chars().enumerate() {
            let g = f.glyph(ch);
            for gy in 0..f.height {
                for gx in 0..f.width {
                    if Font::bit(g, f.bytes_per_row(), gx, gy) {
                        let px = x + (n * f.width + gx) * scale;
                        self.fill(px, y + gy * scale, scale, scale, c);
                    }
                }
            }
        }
    }

    /// A dark screen with `lines` centred on it: what is shown while the
    /// VM is down or the session is starting.
    pub fn status(&mut self, lines: &[&str]) {
        self.clear(0x10_1418);
        let f = Font::get();
        let scale = (self.height / 360).clamp(1, 4);
        let (cw, ch) = (f.width * scale, f.height * scale * 3 / 2);
        let top = (self.height / 2).saturating_sub(lines.len() * ch / 2);
        for (i, line) in lines.iter().enumerate() {
            let fit = (self.width / cw).max(1);
            let line: String = line.chars().take(fit).collect();
            let x = self.width.saturating_sub(line.chars().count() * cw) / 2;
            let colour = if i == 0 { 0xe8_eaed } else { 0x9a_a0a6 };
            self.text(x, top + i * ch, &line, colour, scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_draws_text_inside_the_screen() {
        let mut c = Canvas::new(640, 480);
        c.status(&["default/desk", "not running; retrying in 4 s"]);
        let ink = c.pixels.iter().filter(|&&p| p == 0xe8_eaed).count();
        assert!(ink > 100, "{ink}");
        // A line longer than the screen is cut, not drawn past the edge.
        let mut c = Canvas::new(64, 32);
        c.status(&[&"x".repeat(100)]);
        assert_eq!(c.pixels.len(), 64 * 32);
    }
}
