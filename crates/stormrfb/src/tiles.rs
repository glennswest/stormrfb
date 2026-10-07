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
    // Check framing before allocating a framebuffer-sized output. A fragmented
    // Hextile rectangle must not trigger a large allocation on every byte.
    let mut scan = Reader::new(&r.data[r.pos..]);
    for y in (0..h).step_by(16) {
        for x in (0..w).step_by(16) {
            let flags = scan.u8()?;
            if flags & !31 != 0 {
                return Err(Error::Invalid("hextile flags"));
            }
            if flags & 1 != 0 {
                scan.take(16.min(w - x) * 16.min(h - y) * f.bytes())?;
            } else {
                if flags & 2 != 0 {
                    scan.take(f.bytes())?;
                }
                if flags & 4 != 0 {
                    scan.take(f.bytes())?;
                }
                if flags & 8 != 0 {
                    let count = usize::from(scan.u8()?);
                    scan.take(count * (2 + if flags & 16 != 0 { f.bytes() } else { 0 }))?;
                }
            }
        }
    }
    // Every output pixel is assigned by a complete tile before returning.
    let mut out = vec![[0; 4]; w * h];
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
    // Tiles overwrite the entire output; zero allocation avoids a redundant
    // per-pixel opaque-black fill. Scratch space is reused for every tile.
    let mut out = vec![[0; 4]; w * h];
    let mut scratch = [[0; 4]; 64 * 64];
    let mut palette = [[0; 4]; 127];
    for y in (0..h).step_by(64) {
        for x in (0..w).step_by(64) {
            let tw = 64.min(w - x);
            let th = 64.min(h - y);
            let n = tw * th;
            let tile = &mut scratch[..n];
            let mode = r.u8()?;
            match mode {
                0 => {
                    for pixel in tile.iter_mut() {
                        *pixel = cpixel(&mut r, f)?;
                    }
                }
                1 => tile.fill(cpixel(&mut r, f)?),
                2..=16 => {
                    let palette = &mut palette[..usize::from(mode)];
                    for color in palette.iter_mut() {
                        *color = cpixel(&mut r, f)?;
                    }
                    let bits = if mode <= 2 {
                        1
                    } else if mode <= 4 {
                        2
                    } else {
                        4
                    };
                    for row in 0..th {
                        let mut byte = 0;
                        for col in 0..tw {
                            if col % (8 / bits) == 0 {
                                byte = r.u8()?;
                            }
                            let idx = usize::from((byte >> (8 - bits)) & ((1 << bits) - 1));
                            tile[row * tw + col] = *palette
                                .get(idx)
                                .ok_or(Error::Invalid("ZRLE palette index"))?;
                            byte <<= bits;
                        }
                    }
                }
                128 => {
                    let mut filled = 0;
                    while filled < n {
                        let color = cpixel(&mut r, f)?;
                        let count = run(&mut r, n - filled)?;
                        tile[filled..filled + count].fill(color);
                        filled += count;
                    }
                }
                130..=255 => {
                    let palette = &mut palette[..usize::from(mode & 127)];
                    for color in palette.iter_mut() {
                        *color = cpixel(&mut r, f)?;
                    }
                    let mut filled = 0;
                    while filled < n {
                        let index = r.u8()?;
                        let color = *palette
                            .get(usize::from(index & 127))
                            .ok_or(Error::Invalid("ZRLE palette index"))?;
                        let count = if index & 128 != 0 {
                            run(&mut r, n - filled)?
                        } else {
                            1
                        };
                        tile[filled..filled + count].fill(color);
                        filled += count;
                    }
                }
                _ => return Err(Error::Invalid("reserved ZRLE subencoding")),
            }
            blit(&mut out, w, x, y, tw, th, tile);
        }
    }
    if r.pos != data.len() {
        return Err(Error::Invalid("trailing ZRLE tile data"));
    }
    Ok(out)
}

// Encoders. They work on wire pixels (`bpp` bytes each, row-major), so two
// colours that one pixel format maps to the same bytes are one colour.

fn key(px: &[u8]) -> u32 {
    let mut b = [0; 4];
    b[..px.len()].copy_from_slice(px);
    u32::from_le_bytes(b)
}
/// Copy one tile's wire pixels and their keys out of the rectangle.
fn gather(
    wire: &[u8],
    bpp: usize,
    w: usize,
    (x, y, tw, th): (usize, usize, usize, usize),
    bytes: &mut Vec<u8>,
    keys: &mut Vec<u32>,
) {
    bytes.clear();
    keys.clear();
    for row in y..y + th {
        let line = &wire[(row * w + x) * bpp..(row * w + x + tw) * bpp];
        bytes.extend_from_slice(line);
        keys.extend(line.chunks_exact(bpp).map(key));
    }
}
/// Most common key and the number of distinct keys in a tile.
fn census(keys: &[u32], sorted: &mut Vec<u32>) -> (usize, usize) {
    sorted.clear();
    sorted.extend_from_slice(keys);
    sorted.sort_unstable();
    let (mut best, mut best_n, mut distinct) = (0, 0, 0);
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i + 1;
        while j < sorted.len() && sorted[j] == sorted[i] {
            j += 1;
        }
        distinct += 1;
        if j - i > best_n {
            best_n = j - i;
            best = i;
        }
        i = j;
    }
    let colour = sorted[best];
    (
        keys.iter().position(|&k| k == colour).unwrap_or(0),
        distinct,
    )
}

/// Hextile, choosing per 16×16 tile: background only (nothing at all when
/// it is the previous tile's background), one foreground in subrectangles,
/// coloured subrectangles, or raw when that is no larger. The background
/// and foreground are respecified after a raw tile, and the foreground
/// after a coloured one, rather than relying on what a decoder carries.
pub(crate) fn encode_hextile(wire: &[u8], bpp: usize, w: usize, h: usize, out: &mut Vec<u8>) {
    let (mut bytes, mut keys, mut sorted) = (vec![], vec![], vec![]);
    let mut subrects: Vec<(usize, u8, u8)> = vec![];
    let mut covered = [false; 256];
    let mut bg: Option<u32> = None;
    let mut fg: Option<u32> = None;
    for y in (0..h).step_by(16) {
        for x in (0..w).step_by(16) {
            let tw = 16.min(w - x);
            let th = 16.min(h - y);
            gather(wire, bpp, w, (x, y, tw, th), &mut bytes, &mut keys);
            let px = |i: usize| &bytes[i * bpp..(i + 1) * bpp];
            let (bg_at, distinct) = census(&keys, &mut sorted);
            let bg_key = keys[bg_at];
            let new_bg = bg != Some(bg_key);
            if distinct == 1 {
                out.push(if new_bg { 2 } else { 0 });
                if new_bg {
                    out.extend_from_slice(px(bg_at));
                }
                bg = Some(bg_key);
                continue;
            }
            // Greedy cover of every non-background pixel: run right, then
            // grow down while the whole run matches. Overlap is harmless,
            // a pixel is only ever painted its own colour.
            subrects.clear();
            covered[..tw * th].fill(false);
            for sy in 0..th {
                for sx in 0..tw {
                    let i = sy * tw + sx;
                    if covered[i] || keys[i] == bg_key {
                        continue;
                    }
                    let c = keys[i];
                    let mut sw = 1;
                    while sx + sw < tw && keys[i + sw] == c {
                        sw += 1;
                    }
                    let mut sh = 1;
                    while sy + sh < th
                        && keys[(sy + sh) * tw + sx..(sy + sh) * tw + sx + sw]
                            .iter()
                            .all(|&k| k == c)
                    {
                        sh += 1;
                    }
                    for row in sy..sy + sh {
                        covered[row * tw + sx..row * tw + sx + sw].fill(true);
                    }
                    subrects.push((i, (sx << 4 | sy) as u8, ((sw - 1) << 4 | (sh - 1)) as u8));
                }
            }
            let mono = distinct == 2;
            let fg_key = keys[subrects[0].0];
            let new_fg = mono && fg != Some(fg_key);
            let size = 2
                + if new_bg { bpp } else { 0 }
                + if new_fg { bpp } else { 0 }
                + subrects.len() * (2 + if mono { 0 } else { bpp });
            if subrects.len() > 255 || size > tw * th * bpp {
                out.push(1);
                out.extend_from_slice(&bytes);
                bg = None;
                fg = None;
                continue;
            }
            let mut flags = 8;
            if new_bg {
                flags |= 2;
            }
            if new_fg {
                flags |= 4;
            }
            if !mono {
                flags |= 16;
            }
            out.push(flags);
            if new_bg {
                out.extend_from_slice(px(bg_at));
            }
            if new_fg {
                out.extend_from_slice(px(subrects[0].0));
            }
            out.push(subrects.len() as u8);
            for &(i, xy, wh) in &subrects {
                if !mono {
                    out.extend_from_slice(px(i));
                }
                out.extend([xy, wh]);
            }
            bg = Some(bg_key);
            fg = if mono { Some(fg_key) } else { None };
        }
    }
}

fn put_run(out: &mut Vec<u8>, len: usize) {
    let mut v = len - 1;
    while v >= 255 {
        out.push(255);
        v -= 255;
    }
    out.push(v as u8);
}
fn run_bytes(len: usize) -> usize {
    (len - 1) / 255 + 1
}

/// ZRLE tile data (before zlib), choosing per 64×64 tile the smallest of
/// solid, packed palette, plain RLE, palette RLE and raw. `skip` is the
/// byte [`omitted`] from each CPIXEL.
pub(crate) fn encode_zrle(
    wire: &[u8],
    bpp: usize,
    skip: Option<usize>,
    w: usize,
    h: usize,
    out: &mut Vec<u8>,
) {
    let c = bpp - usize::from(skip.is_some());
    let cpixel = |out: &mut Vec<u8>, px: &[u8]| match skip {
        Some(s) => {
            out.extend_from_slice(&px[..s]);
            out.extend_from_slice(&px[s + 1..]);
        }
        None => out.extend_from_slice(px),
    };
    let (mut bytes, mut keys) = (vec![], vec![]);
    let mut palette: Vec<usize> = Vec::with_capacity(128);
    let mut index = std::collections::HashMap::<u32, u8>::with_capacity(256);
    let mut runs: Vec<(usize, usize)> = vec![];
    for y in (0..h).step_by(64) {
        for x in (0..w).step_by(64) {
            let tw = 64.min(w - x);
            let th = 64.min(h - y);
            let n = tw * th;
            gather(wire, bpp, w, (x, y, tw, th), &mut bytes, &mut keys);
            let px = |i: usize| &bytes[i * bpp..(i + 1) * bpp];
            palette.clear();
            index.clear();
            runs.clear();
            let mut fits = true;
            let mut start = 0;
            for i in 0..n {
                if fits && !index.contains_key(&keys[i]) {
                    if palette.len() == 127 {
                        fits = false;
                    } else {
                        index.insert(keys[i], palette.len() as u8);
                        palette.push(i);
                    }
                }
                if i + 1 == n || keys[i + 1] != keys[i] {
                    runs.push((start, i + 1 - start));
                    start = i + 1;
                }
            }
            if fits && palette.len() == 1 {
                out.push(1);
                cpixel(out, px(0));
                continue;
            }
            let raw = n * c;
            let plain: usize = runs.iter().map(|&(_, len)| c + run_bytes(len)).sum();
            let (packed, bits, pal_rle) = if fits {
                let p = palette.len();
                let bits = if p <= 2 {
                    1
                } else if p <= 4 {
                    2
                } else {
                    4
                };
                let packed = if p <= 16 {
                    p * c + th * (tw * bits).div_ceil(8)
                } else {
                    usize::MAX
                };
                let pal_rle = p * c
                    + runs
                        .iter()
                        .map(|&(_, len)| if len == 1 { 1 } else { 1 + run_bytes(len) })
                        .sum::<usize>();
                (packed, bits, pal_rle)
            } else {
                (usize::MAX, 0, usize::MAX)
            };
            let best = raw.min(plain).min(packed).min(pal_rle);
            if best == raw {
                out.push(0);
                for i in 0..n {
                    cpixel(out, px(i));
                }
            } else if best == packed {
                out.push(palette.len() as u8);
                for &i in &palette {
                    cpixel(out, px(i));
                }
                for row in 0..th {
                    let (mut byte, mut used) = (0u8, 0);
                    for col in 0..tw {
                        byte = byte << bits | index[&keys[row * tw + col]];
                        used += bits;
                        if used == 8 {
                            out.push(byte);
                            (byte, used) = (0, 0);
                        }
                    }
                    if used != 0 {
                        out.push(byte << (8 - used));
                    }
                }
            } else if best == pal_rle {
                out.push(128 | palette.len() as u8);
                for &i in &palette {
                    cpixel(out, px(i));
                }
                for &(i, len) in &runs {
                    let idx = index[&keys[i]];
                    if len == 1 {
                        out.push(idx);
                    } else {
                        out.push(128 | idx);
                        put_run(out, len);
                    }
                }
            } else {
                out.push(128);
                for &(i, len) in &runs {
                    cpixel(out, px(i));
                    put_run(out, len);
                }
            }
        }
    }
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

    fn formats() -> Vec<PixelFormat> {
        let rgb565 = PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            red_max: 31,
            green_max: 63,
            blue_max: 31,
            red_shift: 11,
            green_shift: 5,
            blue_shift: 0,
            ..PixelFormat::RGBX
        };
        vec![
            PixelFormat::RGBX,
            PixelFormat {
                red_shift: 24,
                green_shift: 16,
                blue_shift: 8,
                big_endian: true,
                ..PixelFormat::RGBX
            },
            rgb565,
            PixelFormat {
                big_endian: true,
                ..rgb565
            },
        ]
    }
    /// Scenes that exercise each choice: solid, two colours, a few, many
    /// with runs, noise, and stripes whose runs pass 255.
    fn scenes(w: usize, h: usize) -> Vec<Vec<[u8; 4]>> {
        let mut seed = 0x9e3779b9u32;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let colours: Vec<[u8; 4]> = (0..200)
            .map(|_| {
                let v = rnd().to_le_bytes();
                [v[0], v[1], v[2], 255]
            })
            .collect();
        let n = w * h;
        let mut out = vec![
            vec![colours[0]; n],
            (0..n)
                .map(|i| colours[usize::from((i % w) * 7 % 11 == 0 || i / w == 3)])
                .collect(),
            (0..n).map(|i| colours[(i / 5 + i % w / 9) % 5]).collect(),
            (0..n).map(|i| colours[(i / 3) % 40]).collect(),
            (0..n).map(|i| colours[(i / 17) % 200]).collect(),
            (0..n).map(|i| colours[usize::from(i * 2 >= n)]).collect(),
            (0..n).map(|i| colours[i / 300 % 200]).collect(),
        ];
        out.push(
            (0..n)
                .map(|_| {
                    let v = rnd().to_le_bytes();
                    [v[0], v[1], v[2], 7]
                })
                .collect(),
        );
        out
    }
    fn wire(f: PixelFormat, pixels: &[[u8; 4]]) -> (Vec<u8>, Vec<[u8; 4]>) {
        let mut b = vec![];
        for &p in pixels {
            f.write(p, &mut b).unwrap();
        }
        let expect = b
            .chunks_exact(f.bytes())
            .map(|c| f.read(c).unwrap())
            .collect();
        (b, expect)
    }
    #[test]
    fn encoders_round_trip_every_scene_and_format() {
        for f in formats() {
            for (w, h) in [(1, 1), (16, 16), (17, 3), (64, 64), (130, 70)] {
                for (k, scene) in scenes(w, h).iter().enumerate() {
                    let (b, expect) = wire(f, scene);
                    let mut hx = vec![];
                    encode_hextile(&b, f.bytes(), w, h, &mut hx);
                    let mut r = Reader::new(&hx);
                    assert_eq!(
                        hextile(&mut r, f, w, h).unwrap(),
                        expect,
                        "hextile {f:?} {w}x{h} #{k}"
                    );
                    assert_eq!(r.pos, hx.len());
                    assert!(hx.len() <= b.len() + w.div_ceil(16) * h.div_ceil(16));
                    let mut z = vec![];
                    encode_zrle(&b, f.bytes(), omitted(f), w, h, &mut z);
                    assert_eq!(
                        zrle(&z, f, w, h).unwrap(),
                        expect,
                        "zrle {f:?} {w}x{h} #{k}"
                    );
                    let c = f.bytes() - usize::from(omitted(f).is_some());
                    assert!(z.len() <= w * h * c + w.div_ceil(64) * h.div_ceil(64));
                }
            }
        }
    }
    #[test]
    fn encoders_choose_each_subencoding() {
        let f = PixelFormat::RGBX;
        let mode = |pixels: &[[u8; 4]], w, h| {
            let (b, _) = wire(f, pixels);
            let mut z = vec![];
            encode_zrle(&b, 4, omitted(f), w, h, &mut z);
            z[0]
        };
        let a = [1, 2, 3, 255];
        let b = [9, 8, 7, 255];
        assert_eq!(mode(&[a; 64 * 64], 64, 64), 1);
        let checker: Vec<_> = (0..64 * 64)
            .map(|i| if (i + i / 64) % 2 == 0 { a } else { b })
            .collect();
        assert_eq!(mode(&checker, 64, 64), 2);
        // Two long runs: plain RLE beats a palette.
        let halves: Vec<_> = (0..64 * 64).map(|i| if i < 2048 { a } else { b }).collect();
        assert_eq!(mode(&halves, 64, 64), 128);
        // 40 colours in runs of 3: palette RLE.
        let pal: Vec<_> = (0..64 * 64)
            .map(|i| [(i / 3 % 40) as u8, 0, 0, 255])
            .collect();
        assert_eq!(mode(&pal, 64, 64), 128 | 40);
        let runs: Vec<_> = (0..64 * 64).map(|i| [(i / 32) as u8, 0, 0, 255]).collect();
        assert_eq!(mode(&runs, 64, 64), 128);
        let noise: Vec<_> = (0..64 * 64u32)
            .map(|i| {
                let v = i.wrapping_mul(2654435761).to_le_bytes();
                [v[3], v[2], v[1], 255]
            })
            .collect();
        assert_eq!(mode(&noise, 64, 64), 0);

        let hx = |pixels: &[[u8; 4]], w, h| {
            let (b, _) = wire(f, pixels);
            let mut o = vec![];
            encode_hextile(&b, 4, w, h, &mut o);
            o
        };
        // Two solid tiles: the second carries the background, one byte.
        assert_eq!(hx(&[a; 32 * 16], 32, 16), vec![2, 1, 2, 3, 0, 0]);
        // A mono tile: bg + fg + one subrect.
        let mut t = vec![a; 256];
        t[17] = b;
        t[18] = b;
        assert_eq!(
            hx(&t, 16, 16),
            vec![14, 1, 2, 3, 0, 9, 8, 7, 0, 1, 0x11, 0x10]
        );
        assert_eq!(hx(&noise[..256], 16, 16)[0], 1);
    }
}
