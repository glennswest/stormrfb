use crate::{tiles, *};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rectangle {
    Pixels {
        rect: Rect,
        pixels: Vec<[u8; 4]>,
    },
    Copy {
        rect: Rect,
        source_x: u16,
        source_y: u16,
    },
    Cursor {
        hotspot_x: u16,
        hotspot_y: u16,
        width: u16,
        height: u16,
        pixels: Vec<[u8; 4]>,
        mask: Vec<u8>,
    },
    DesktopSize {
        width: u16,
        height: u16,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    UpdateStart,
    Rectangle(Rectangle),
    UpdateEnd,
    Bell,
    CutText(Vec<u8>),
    ColourMap { first: u16, colors: Vec<[u16; 3]> },
}
/// Incremental server-message decoder. Caller retains unconsumed bytes.
/// Errors other than Incomplete poison the decoder, including malformed ZRLE.
pub struct ServerDecoder {
    format: PixelFormat,
    limits: Limits,
    remaining: Option<u16>,
    inflater: Decompress,
    failed: bool,
}
impl ServerDecoder {
    pub fn new(format: PixelFormat, limits: Limits) -> Result<Self> {
        format.validate()?;
        Ok(Self {
            format,
            limits,
            remaining: None,
            inflater: Decompress::new(true),
            failed: false,
        })
    }
    pub fn next(&mut self, b: &[u8]) -> Result<(ServerEvent, usize)> {
        if self.failed {
            return Err(Error::Invalid("decoder failed"));
        }
        let result = self.parse(b);
        if matches!(&result,Err(e) if *e!=Error::Incomplete) {
            self.failed = true;
        }
        result
    }
    fn parse(&mut self, b: &[u8]) -> Result<(ServerEvent, usize)> {
        let mut r = Reader::new(b);
        if let Some(n) = self.remaining {
            if n == 0 {
                self.remaining = None;
                return Ok((ServerEvent::UpdateEnd, 0));
            }
            let rect = r.rect()?;
            let encoding = r.u32()? as i32;
            if encoding == LAST_RECT {
                self.remaining = None;
                return Ok((ServerEvent::UpdateEnd, r.pos));
            }
            let pixels = self.limits.pixels(rect.width, rect.height)?;
            let value = match encoding {
                RAW => Rectangle::Pixels {
                    rect,
                    pixels: tiles::raw(&mut r, self.format, pixels)?,
                },
                COPY_RECT => Rectangle::Copy {
                    rect,
                    source_x: r.u16()?,
                    source_y: r.u16()?,
                },
                HEXTILE => Rectangle::Pixels {
                    rect,
                    pixels: tiles::hextile(
                        &mut r,
                        self.format,
                        usize::from(rect.width),
                        usize::from(rect.height),
                    )?,
                },
                ZRLE => {
                    let len = r.u32()? as usize;
                    if len > self.limits.max_bytes {
                        return Err(Error::Limit);
                    }
                    let compressed = r.take(len)?;
                    // CPIXEL + per-pixel run overhead + tile headers/palettes.
                    let cap = pixels
                        .checked_mul(6)
                        .and_then(|n| n.checked_add(1024))
                        .ok_or(Error::Limit)?
                        .min(self.limits.max_bytes);
                    let data = self.inflate(compressed, cap)?;
                    let decoded = tiles::zrle(
                        &data,
                        self.format,
                        usize::from(rect.width),
                        usize::from(rect.height),
                    )
                    .map_err(|e| {
                        if e == Error::Incomplete {
                            Error::Invalid("truncated ZRLE tiles")
                        } else {
                            e
                        }
                    })?;
                    Rectangle::Pixels {
                        rect,
                        pixels: decoded,
                    }
                }
                CURSOR => {
                    let data = tiles::raw(&mut r, self.format, pixels)?;
                    let mask = r
                        .take(usize::from(rect.width).div_ceil(8) * usize::from(rect.height))?
                        .to_vec();
                    if pixels != 0 && (rect.x >= rect.width || rect.y >= rect.height) {
                        return Err(Error::Invalid("cursor hotspot"));
                    }
                    Rectangle::Cursor {
                        hotspot_x: rect.x,
                        hotspot_y: rect.y,
                        width: rect.width,
                        height: rect.height,
                        pixels: data,
                        mask,
                    }
                }
                DESKTOP_SIZE => {
                    if rect.x != 0 || rect.y != 0 || pixels == 0 {
                        return Err(Error::Invalid("desktop size"));
                    }
                    Rectangle::DesktopSize {
                        width: rect.width,
                        height: rect.height,
                    }
                }
                n => return Err(Error::Unsupported(n)),
            };
            if r.pos > self.limits.max_bytes {
                return Err(Error::Limit);
            }
            self.remaining = Some(n - 1);
            return Ok((ServerEvent::Rectangle(value), r.pos));
        }
        let event = match r.u8()? {
            0 => {
                r.take(1)?;
                let n = r.u16()?;
                if usize::from(n) > self.limits.max_rectangles && n != u16::MAX {
                    return Err(Error::Limit);
                }
                self.remaining = Some(n);
                ServerEvent::UpdateStart
            }
            1 => {
                r.take(1)?;
                let first = r.u16()?;
                let n = usize::from(r.u16()?);
                if usize::from(first) + n > 65536 || n * 6 > self.limits.max_bytes {
                    return Err(Error::Limit);
                }
                r.data.get(r.pos..r.pos + n * 6).ok_or(Error::Incomplete)?;
                let mut colors = Vec::with_capacity(n);
                for _ in 0..n {
                    colors.push([r.u16()?, r.u16()?, r.u16()?]);
                }
                ServerEvent::ColourMap { first, colors }
            }
            2 => ServerEvent::Bell,
            3 => {
                r.take(3)?;
                ServerEvent::CutText(r.text(self.limits)?)
            }
            n => return Err(Error::Unsupported(i32::from(n))),
        };
        Ok((event, r.pos))
    }
    fn inflate(&mut self, input: &[u8], cap: usize) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut pos = 0;
        loop {
            let before_in = self.inflater.total_in();
            let before_out = self.inflater.total_out();
            let mut chunk = [0; 8192];
            let status = self
                .inflater
                .decompress(&input[pos..], &mut chunk, FlushDecompress::Sync)
                .map_err(|_| Error::Invalid("zlib stream"))?;
            let read = (self.inflater.total_in() - before_in) as usize;
            let written = (self.inflater.total_out() - before_out) as usize;
            pos += read;
            if out.len() + written > cap {
                return Err(Error::Limit);
            }
            out.extend_from_slice(&chunk[..written]);
            if status == Status::StreamEnd {
                return Err(Error::Invalid("ZRLE stream ended before connection"));
            }
            if read == 0 && written == 0 {
                if pos != input.len() {
                    return Err(Error::Invalid("zlib stalled"));
                }
                break;
            }
            if pos == input.len() && written < chunk.len() {
                break;
            }
        }
        Ok(out)
    }
}
/// Shared rectangle encoder; ZRLE compressor lives for the entire connection.
pub struct ServerEncoder {
    compressor: Compress,
    limits: Limits,
}
impl ServerEncoder {
    pub fn new(limits: Limits) -> Self {
        Self {
            compressor: Compress::new(Compression::fast(), true),
            limits,
        }
    }
    pub fn update(
        &mut self,
        rects: &[Rectangle],
        format: PixelFormat,
        encoding: i32,
    ) -> Result<Vec<u8>> {
        format.validate()?;
        if rects.len() > self.limits.max_rectangles {
            return Err(Error::Limit);
        }
        let count = u16::try_from(rects.len()).map_err(|_| Error::Limit)?;
        // Validate the entire update before advancing compression state.
        let mut estimate = 4usize;
        for rect in rects {
            let (w, h, len) = match rect {
                Rectangle::Pixels { rect, pixels } => (rect.width, rect.height, Some(pixels.len())),
                Rectangle::Copy { rect, .. } => (rect.width, rect.height, None),
                Rectangle::DesktopSize { width, height } => {
                    if *width == 0 || *height == 0 {
                        return Err(Error::Invalid("desktop size"));
                    }
                    (*width, *height, None)
                }
                Rectangle::Cursor {
                    width,
                    height,
                    pixels,
                    mask,
                    hotspot_x,
                    hotspot_y,
                } => {
                    if mask.len() != usize::from(*width).div_ceil(8) * usize::from(*height)
                        || (!pixels.is_empty() && (*hotspot_x >= *width || *hotspot_y >= *height))
                    {
                        return Err(Error::Invalid("cursor"));
                    }
                    (*width, *height, Some(pixels.len()))
                }
            };
            let n = self.limits.pixels(w, h)?;
            if let Some(len) = len {
                if len != n {
                    return Err(Error::Invalid("pixel count"));
                }
            }
            estimate = estimate.checked_add(n * 6 + 1024).ok_or(Error::Limit)?;
        }
        if estimate > self.limits.max_bytes {
            return Err(Error::Limit);
        }
        if ![RAW, HEXTILE, ZRLE].contains(&encoding) {
            return Err(Error::Unsupported(encoding));
        }
        let mut out = vec![0, 0];
        out.extend(count.to_be_bytes());
        for rect in rects {
            match rect {
                Rectangle::Pixels { rect, pixels } => {
                    put_rect(&mut out, *rect);
                    out.extend(encoding.to_be_bytes());
                    match encoding {
                        RAW => {
                            for p in pixels {
                                format.write(*p, &mut out)?;
                            }
                        }
                        HEXTILE | ZRLE => {
                            let size = if encoding == HEXTILE { 16 } else { 64 };
                            let mut tiles = Vec::new();
                            let w = usize::from(rect.width);
                            let h = usize::from(rect.height);
                            for y in (0..h).step_by(size) {
                                for x in (0..w).step_by(size) {
                                    tiles.push(if encoding == HEXTILE { 1 } else { 0 });
                                    for row in y..(y + size).min(h) {
                                        for col in x..(x + size).min(w) {
                                            let start = tiles.len();
                                            format.write(pixels[row * w + col], &mut tiles)?;
                                            if encoding == ZRLE {
                                                if let Some(skip) = tiles::omitted(format) {
                                                    tiles.remove(start + skip);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if encoding == ZRLE {
                                let compressed = self.deflate(&tiles)?;
                                out.extend(
                                    u32::try_from(compressed.len())
                                        .map_err(|_| Error::Limit)?
                                        .to_be_bytes(),
                                );
                                out.extend(compressed);
                            } else {
                                out.extend(tiles);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                Rectangle::Copy {
                    rect,
                    source_x,
                    source_y,
                } => {
                    put_rect(&mut out, *rect);
                    out.extend(COPY_RECT.to_be_bytes());
                    out.extend(source_x.to_be_bytes());
                    out.extend(source_y.to_be_bytes());
                }
                Rectangle::DesktopSize { width, height } => {
                    put_rect(
                        &mut out,
                        Rect {
                            width: *width,
                            height: *height,
                            ..Rect::default()
                        },
                    );
                    out.extend(DESKTOP_SIZE.to_be_bytes());
                }
                Rectangle::Cursor {
                    hotspot_x,
                    hotspot_y,
                    width,
                    height,
                    pixels,
                    mask,
                } => {
                    put_rect(
                        &mut out,
                        Rect {
                            x: *hotspot_x,
                            y: *hotspot_y,
                            width: *width,
                            height: *height,
                        },
                    );
                    out.extend(CURSOR.to_be_bytes());
                    for p in pixels {
                        format.write(*p, &mut out)?;
                    }
                    out.extend(mask);
                }
            }
        }
        Ok(out)
    }
    fn deflate(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut pos = 0;
        loop {
            let bi = self.compressor.total_in();
            let bo = self.compressor.total_out();
            let mut chunk = [0; 8192];
            self.compressor
                .compress(&input[pos..], &mut chunk, FlushCompress::Sync)
                .map_err(|_| Error::Invalid("zlib encoder"))?;
            let read = (self.compressor.total_in() - bi) as usize;
            let written = (self.compressor.total_out() - bo) as usize;
            pos += read;
            out.extend(&chunk[..written]);
            if pos == input.len() && written < chunk.len() {
                break;
            }
            if read == 0 && written == 0 {
                return Err(Error::Invalid("zlib encoder stalled"));
            }
        }
        Ok(out)
    }
}
impl ServerEvent {
    pub fn encode_control(&self, limits: Limits) -> Result<Vec<u8>> {
        let mut b = Vec::new();
        match self {
            Self::Bell => b.push(2),
            Self::CutText(t) => {
                b.extend([3, 0, 0, 0]);
                put_text(&mut b, t, limits)?;
            }
            Self::ColourMap { first, colors } => {
                if usize::from(*first) + colors.len() > 65536 || colors.len() * 6 > limits.max_bytes
                {
                    return Err(Error::Limit);
                }
                b.extend([1, 0]);
                b.extend(first.to_be_bytes());
                b.extend(
                    u16::try_from(colors.len())
                        .map_err(|_| Error::Limit)?
                        .to_be_bytes(),
                );
                for c in colors {
                    for v in c {
                        b.extend(v.to_be_bytes());
                    }
                }
            }
            _ => return Err(Error::Invalid("not a control message")),
        }
        Ok(b)
    }
}
