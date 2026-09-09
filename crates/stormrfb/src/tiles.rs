use crate::{Error, PixelFormat, Reader, Result};

pub(crate) fn raw(r: &mut Reader<'_>, f: PixelFormat, n: usize) -> Result<Vec<[u8; 4]>> {
    let bytes = r.take(n.checked_mul(f.bytes()).ok_or(Error::Limit)?)?;
    bytes.chunks_exact(f.bytes()).map(|b| f.read(b)).collect()
}
fn blit(
    out: &mut [[u8; 4]],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    pixels: &[[u8; 4]],
) {
    for row in 0..h {
        out[(y + row) * stride + x..(y + row) * stride + x + w]
            .copy_from_slice(&pixels[row * w..(row + 1) * w]);
    }
}
pub(crate) fn hextile(
    r: &mut Reader<'_>,
    f: PixelFormat,
    w: usize,
    h: usize,
) -> Result<Vec<[u8; 4]>> {
    let mut out = vec![[0, 0, 0, 255]; w * h];
    let mut bg = None;
    let mut fg = None;
    for y in (0..h).step_by(16) {
        for x in (0..w).step_by(16) {
            let tw = 16.min(w - x);
            let th = 16.min(h - y);
            let flags = r.u8()?;
            if flags & !31 != 0 {
                return Err(Error::Invalid("hextile flags"));
            }
            if flags & 1 != 0 {
                let p = raw(r, f, tw * th)?;
                blit(&mut out, w, x, y, tw, th, &p);
                continue;
            }
            if flags & 2 != 0 {
                bg = Some(f.read(r.take(f.bytes())?)?);
            }
            if flags & 4 != 0 {
                fg = Some(f.read(r.take(f.bytes())?)?);
            }
            let mut tile = vec![bg.ok_or(Error::Invalid("missing hextile background"))?; tw * th];
            if flags & 8 != 0 {
                let n = r.u8()?;
                for _ in 0..n {
                    let color = if flags & 16 != 0 {
                        f.read(r.take(f.bytes())?)?
                    } else {
                        fg.ok_or(Error::Invalid("missing hextile foreground"))?
                    };
                    let xy = r.u8()?;
                    let wh = r.u8()?;
                    let sx = usize::from(xy >> 4);
                    let sy = usize::from(xy & 15);
                    let sw = usize::from(wh >> 4) + 1;
                    let sh = usize::from(wh & 15) + 1;
                    if sx + sw > tw || sy + sh > th {
                        return Err(Error::Invalid("hextile subrectangle bounds"));
                    }
                    for row in sy..sy + sh {
                        tile[row * tw + sx..row * tw + sx + sw].fill(color);
                    }
                }
            }
            blit(&mut out, w, x, y, tw, th, &tile);
        }
    }
    Ok(out)
}
/// Byte omitted by ZRLE CPIXEL, if the true-colour channels fit in 24 bits.
pub(crate) fn omitted(f: PixelFormat) -> Option<usize> {
    if f.bits_per_pixel != 32 || f.depth > 24 {
        return None;
    }
    let mask = (u32::from(f.red_max) << f.red_shift)
        | (u32::from(f.green_max) << f.green_shift)
        | (u32::from(f.blue_max) << f.blue_shift);
    let significance = if mask & 0xff000000 == 0 {
        3
    } else if mask & 0xff == 0 {
        0
    } else {
        return None;
    };
    Some(if f.big_endian {
        3 - significance
    } else {
        significance
    })
}
fn cpixel(r: &mut Reader<'_>, f: PixelFormat) -> Result<[u8; 4]> {
    if let Some(skip) = omitted(f) {
        let b = r.take(3)?;
        let mut full = [0; 4];
        let mut j = 0;
        for (i, v) in full.iter_mut().enumerate() {
            if i != skip {
                *v = b[j];
                j += 1;
            }
        }
        f.read(&full)
    } else {
        f.read(r.take(f.bytes())?)
    }
}
fn run(r: &mut Reader<'_>, remaining: usize) -> Result<usize> {
    let mut n = 1usize;
    loop {
        let b = usize::from(r.u8()?);
        n = n.checked_add(b).ok_or(Error::Limit)?;
        if n > remaining {
            return Err(Error::Invalid("ZRLE run overflow"));
        }
        if b != 255 {
            return Ok(n);
        }
    }
}
pub(crate) fn zrle(data: &[u8], f: PixelFormat, w: usize, h: usize) -> Result<Vec<[u8; 4]>> {
    let mut r = Reader::new(data);
    let mut out = vec![[0, 0, 0, 255]; w * h];
    for y in (0..h).step_by(64) {
        for x in (0..w).step_by(64) {
            let tw = 64.min(w - x);
            let th = 64.min(h - y);
            let n = tw * th;
            let mode = r.u8()?;
            let mut tile = Vec::with_capacity(n);
            match mode {
                0 => {
                    for _ in 0..n {
                        tile.push(cpixel(&mut r, f)?);
                    }
                }
                1 => {
                    let c = cpixel(&mut r, f)?;
                    tile.resize(n, c);
                }
                2..=16 => {
                    let palette = (0..mode)
                        .map(|_| cpixel(&mut r, f))
                        .collect::<Result<Vec<_>>>()?;
                    let bits = if mode <= 2 {
                        1
                    } else if mode <= 4 {
                        2
                    } else {
                        4
                    };
                    for _ in 0..th {
                        let mut byte = 0;
                        for col in 0..tw {
                            if col % (8 / bits) == 0 {
                                byte = r.u8()?;
                            }
                            let idx = usize::from((byte >> (8 - bits)) & ((1 << bits) - 1));
                            tile.push(
                                *palette
                                    .get(idx)
                                    .ok_or(Error::Invalid("ZRLE palette index"))?,
                            );
                            byte <<= bits;
                        }
                    }
                }
                128 => {
                    while tile.len() < n {
                        let c = cpixel(&mut r, f)?;
                        let count = run(&mut r, n - tile.len())?;
                        tile.resize(tile.len() + count, c);
                    }
                }
                130..=255 => {
                    let palette = (0..(mode & 127))
                        .map(|_| cpixel(&mut r, f))
                        .collect::<Result<Vec<_>>>()?;
                    while tile.len() < n {
                        let index = r.u8()?;
                        let c = *palette
                            .get(usize::from(index & 127))
                            .ok_or(Error::Invalid("ZRLE palette index"))?;
                        let count = if index & 128 != 0 {
                            run(&mut r, n - tile.len())?
                        } else {
                            1
                        };
                        tile.resize(tile.len() + count, c);
                    }
                }
                _ => return Err(Error::Invalid("reserved ZRLE subencoding")),
            }
            blit(&mut out, w, x, y, tw, th, &tile);
        }
    }
    if r.pos != data.len() {
        return Err(Error::Invalid("trailing ZRLE tile data"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zrle_modes_and_row_padding() {
        let f = PixelFormat::RGBX;
        assert_eq!(
            zrle(&[1, 1, 2, 3], f, 3, 2).unwrap(),
            vec![[1, 2, 3, 255]; 6]
        );
        let p = zrle(&[2, 1, 2, 3, 4, 5, 6, 0b01000000, 0b10100000], f, 3, 2).unwrap();
        assert_eq!(
            p,
            vec![
                [1, 2, 3, 255],
                [4, 5, 6, 255],
                [1, 2, 3, 255],
                [4, 5, 6, 255],
                [1, 2, 3, 255],
                [4, 5, 6, 255]
            ]
        );
        assert_eq!(
            zrle(&[128, 1, 2, 3, 5], f, 3, 2).unwrap(),
            vec![[1, 2, 3, 255]; 6]
        );
        assert_eq!(
            zrle(&[130, 1, 2, 3, 4, 5, 6, 128, 5], f, 3, 2).unwrap(),
            vec![[1, 2, 3, 255]; 6]
        );
        assert!(zrle(&[128, 1, 2, 3, 6], f, 3, 2).is_err());
        assert!(zrle(&[127], f, 1, 1).is_err());
        assert!(zrle(&[129], f, 1, 1).is_err());
        assert!(zrle(&[3, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xc0], f, 1, 1).is_err());
    }
    #[test]
    fn cpixel_high_channels_both_endiannesses() {
        for big_endian in [false, true] {
            let f = PixelFormat {
                red_shift: 24,
                green_shift: 16,
                blue_shift: 8,
                big_endian,
                ..PixelFormat::RGBX
            };
            let data = if big_endian {
                vec![1, 10, 20, 30]
            } else {
                vec![1, 30, 20, 10]
            };
            assert_eq!(zrle(&data, f, 1, 1).unwrap(), vec![[10, 20, 30, 255]]);
        }
    }
    #[test]
    fn hextile_carry_and_bounds() {
        let mut b = vec![14, 1, 2, 3, 0, 4, 5, 6, 0, 1, 0, 0];
        b.extend([0]);
        let p = hextile(&mut Reader::new(&b), PixelFormat::RGBX, 17, 1).unwrap();
        assert_eq!(p[0], [4, 5, 6, 255]);
        assert_eq!(p[16], [1, 2, 3, 255]);
        assert!(hextile(&mut Reader::new(&[0]), PixelFormat::RGBX, 1, 1).is_err());
        b[11] = 0xff;
        assert!(hextile(&mut Reader::new(&b), PixelFormat::RGBX, 17, 1).is_err());
    }
}
