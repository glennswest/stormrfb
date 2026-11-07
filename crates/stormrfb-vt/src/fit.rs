//! The guest's framebuffer fitted to the screen with its aspect kept:
//! nearest-neighbour scaling, letterboxed in black, and the mapping between
//! screen and guest coordinates both ways.

use stormrfb::Rect;

use crate::canvas::Canvas;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    /// The guest framebuffer's size.
    pub src_w: usize,
    pub src_h: usize,
    /// Where the image lands on the screen.
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    /// For each screen column/row of the image, its guest column/row.
    xmap: Vec<usize>,
    ymap: Vec<usize>,
}

impl Fit {
    /// `src` fitted into `dst`, as large as fits with its aspect kept.
    pub fn new(src_w: usize, src_h: usize, dst_w: usize, dst_h: usize) -> Fit {
        let (src_w, src_h) = (src_w.max(1), src_h.max(1));
        let (dst_w, dst_h) = (dst_w.max(1), dst_h.max(1));
        let (w, h) = if src_w * dst_h <= dst_w * src_h {
            ((src_w * dst_h / src_h).max(1), dst_h)
        } else {
            (dst_w, (src_h * dst_w / src_w).max(1))
        };
        Fit {
            src_w,
            src_h,
            x: (dst_w - w) / 2,
            y: (dst_h - h) / 2,
            w,
            h,
            xmap: (0..w).map(|i| i * src_w / w).collect(),
            ymap: (0..h).map(|j| j * src_h / h).collect(),
        }
    }

    /// The guest pixel under screen position (`px`, `py`), clamped to the
    /// image, so the letterbox maps to its nearest edge.
    pub fn to_guest(&self, px: f64, py: f64) -> (u16, u16) {
        let gx = ((px - self.x as f64) * self.src_w as f64 / self.w as f64).floor();
        let gy = ((py - self.y as f64) * self.src_h as f64 / self.h as f64).floor();
        (
            gx.clamp(0.0, (self.src_w - 1) as f64) as u16,
            gy.clamp(0.0, (self.src_h - 1) as f64) as u16,
        )
    }

    /// The screen position of guest pixel (`gx`, `gy`)'s top-left corner.
    pub fn to_screen(&self, gx: usize, gy: usize) -> (usize, usize) {
        (
            self.x + (gx * self.w).div_ceil(self.src_w),
            self.y + (gy * self.h).div_ceil(self.src_h),
        )
    }

    /// The screen columns (relative to the image) whose guest column lies
    /// in `from..to`: the first `i` with `i * src_w / w >= from`.
    fn span(from: usize, to: usize, src: usize, dst: usize) -> (usize, usize) {
        ((from * dst).div_ceil(src), (to * dst).div_ceil(src).min(dst))
    }

    /// Draws the guest pixels of `damage` (from `rgba`, the guest
    /// framebuffer, `src_w` wide) into `canvas`.
    pub fn blit(&self, rgba: &[u8], damage: Rect, canvas: &mut Canvas) {
        let (x0, x1) = Fit::span(
            usize::from(damage.x),
            usize::from(damage.x) + usize::from(damage.width),
            self.src_w,
            self.w,
        );
        let (y0, y1) = Fit::span(
            usize::from(damage.y),
            usize::from(damage.y) + usize::from(damage.height),
            self.src_h,
            self.h,
        );
        if rgba.len() < self.src_w * self.src_h * 4 {
            return;
        }
        for j in y0..y1 {
            let cy = self.y + j;
            if cy >= canvas.height {
                break;
            }
            let src_row = self.ymap[j] * self.src_w;
            let dst_row = cy * canvas.width + self.x;
            for i in x0..x1 {
                if self.x + i >= canvas.width {
                    break;
                }
                let p = (src_row + self.xmap[i]) * 4;
                canvas.pixels[dst_row + i] = xrgb(&rgba[p..p + 4]);
            }
        }
    }
}

impl Fit {
    /// Draws a guest-space sprite (a cursor: `rgba`, `sw` x `sh`, alpha 0 or
    /// 255) with its top-left at guest (`gx`, `gy`), scaled as the image is,
    /// into `dst`: a screen-sized XRGB buffer `stride` pixels per row.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite(
        &self,
        dst: &mut [u32],
        stride: usize,
        gx: i32,
        gy: i32,
        sw: usize,
        sh: usize,
        rgba: &[u8],
    ) {
        if rgba.len() < sw * sh * 4 {
            return;
        }
        let clip = |g: i32, n: usize, max: usize| {
            let a = g.max(0) as usize;
            let b = (g + n as i32).clamp(0, max as i32) as usize;
            (a.min(max), b)
        };
        let (gx0, gx1) = clip(gx, sw, self.src_w);
        let (gy0, gy1) = clip(gy, sh, self.src_h);
        if gx0 >= gx1 || gy0 >= gy1 {
            return;
        }
        let (x0, x1) = Fit::span(gx0, gx1, self.src_w, self.w);
        let (y0, y1) = Fit::span(gy0, gy1, self.src_h, self.h);
        for j in y0..y1 {
            let sy = (self.ymap[j] as i32 - gy) as usize;
            let row = (self.y + j) * stride + self.x;
            for i in x0..x1 {
                let sx = (self.xmap[i] as i32 - gx) as usize;
                let p = (sy * sw + sx) * 4;
                if rgba[p + 3] != 0
                    && let Some(d) = dst.get_mut(row + i)
                {
                    *d = xrgb(&rgba[p..p + 4]);
                }
            }
        }
    }
}

/// An RGBA pixel as DRM's XRGB8888.
pub fn xrgb(p: &[u8]) -> u32 {
    (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_size_is_one_to_one() {
        let f = Fit::new(1920, 1080, 1920, 1080);
        assert_eq!((f.x, f.y, f.w, f.h), (0, 0, 1920, 1080));
        assert_eq!(f.to_guest(100.0, 200.0), (100, 200));
        assert_eq!(f.to_screen(100, 200), (100, 200));
    }

    #[test]
    fn aspect_is_kept_with_bars() {
        // 4:3 on 16:9: pillarboxed.
        let f = Fit::new(1024, 768, 1920, 1080);
        assert_eq!((f.x, f.y, f.w, f.h), (240, 0, 1440, 1080));
        // 16:10 on 16:9 is narrower: pillarboxed too.
        let f = Fit::new(1280, 800, 1920, 1080);
        assert_eq!((f.w, f.h), (1728, 1080));
        // 16:9 on 4:3: letterboxed.
        let f = Fit::new(1920, 1080, 1024, 768);
        assert_eq!((f.x, f.y, f.w, f.h), (0, 96, 1024, 576));
    }

    #[test]
    fn screen_to_guest_clamps_into_the_image() {
        let f = Fit::new(1024, 768, 1920, 1080);
        assert_eq!(f.to_guest(0.0, 0.0), (0, 0));
        assert_eq!(f.to_guest(1919.0, 1079.0), (1023, 767));
        assert_eq!(f.to_guest(240.0 + 720.0, 540.0), (512, 384));
    }

    #[test]
    fn blit_of_every_damage_is_the_whole_image() {
        // A 3x2 guest on a 7x5 screen: each pixel's colour is its index.
        let (sw, sh) = (3usize, 2usize);
        let rgba: Vec<u8> = (0..sw * sh).flat_map(|i| [i as u8, 0, 0, 255]).collect();
        let f = Fit::new(sw, sh, 7, 5);
        let mut whole = Canvas::new(7, 5);
        f.blit(&rgba, rect(0, 0, 3, 2), &mut whole);
        let mut parts = Canvas::new(7, 5);
        for y in 0..2 {
            for x in 0..3 {
                f.blit(&rgba, rect(x, y, 1, 1), &mut parts);
            }
        }
        assert_eq!(whole.pixels, parts.pixels);
        for j in 0..f.h {
            for i in 0..f.w {
                let want = (f.ymap[j] * sw + f.xmap[i]) as u32;
                assert_eq!(whole.pixels[(f.y + j) * 7 + f.x + i], want << 16);
            }
        }
    }

    #[test]
    fn a_sprite_is_clipped_and_transparent_where_masked() {
        let f = Fit::new(4, 4, 8, 8);
        let mut dst = vec![0u32; 64];
        // 2x2: opaque white except the bottom-right, at guest (3, 3), so
        // only its top-left pixel is on the image.
        let mut rgba = vec![255u8; 16];
        rgba[15] = 0;
        f.sprite(&mut dst, 8, 3, 3, 2, 2, &rgba);
        let lit: Vec<usize> = (0..64).filter(|&i| dst[i] != 0).collect();
        assert_eq!(lit, vec![54, 55, 62, 63]);
        let mut dst = vec![0u32; 64];
        f.sprite(&mut dst, 8, 0, 0, 2, 2, &rgba);
        assert_eq!(dst[0], 0xffffff);
        assert_eq!(dst[2 * 8 + 2], 0, "the masked pixel");
    }

    fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}
