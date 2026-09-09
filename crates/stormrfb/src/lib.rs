//! Bounded, sans-I/O RFB 3.8 protocol primitives.
#![forbid(unsafe_code)]
mod handshake;
mod pixel;
mod wire;
pub use handshake::*;
pub use pixel::*;
pub use wire::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Incomplete,
    Invalid(&'static str),
    Limit,
    Unsupported(i32),
    Authentication,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_pixels: usize,
    pub max_bytes: usize,
    pub max_text: usize,
    pub max_rectangles: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pixels: 16_777_216,
            max_bytes: 128 * 1024 * 1024,
            max_text: 1024 * 1024,
            max_rectangles: 4096,
        }
    }
}
impl Limits {
    pub fn pixels(&self, width: u16, height: u16) -> Result<usize> {
        let n = usize::from(width) * usize::from(height);
        if n > self.max_pixels || n.checked_mul(4).ok_or(Error::Limit)? > self.max_bytes {
            return Err(Error::Limit);
        }
        Ok(n)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
impl Rect {
    pub fn within(self, width: u16, height: u16) -> bool {
        u32::from(self.x) + u32::from(self.width) <= u32::from(width)
            && u32::from(self.y) + u32::from(self.height) <= u32::from(height)
    }
}

pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Limit)?;
        let s = self.data.get(self.pos..end).ok_or(Error::Incomplete)?;
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn rect(&mut self) -> Result<Rect> {
        Ok(Rect {
            x: self.u16()?,
            y: self.u16()?,
            width: self.u16()?,
            height: self.u16()?,
        })
    }
    pub fn text(&mut self, limits: Limits) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        if n > limits.max_text || n > limits.max_bytes {
            return Err(Error::Limit);
        }
        Ok(self.take(n)?.to_vec())
    }
}
pub(crate) fn put_rect(out: &mut Vec<u8>, r: Rect) {
    for v in [r.x, r.y, r.width, r.height] {
        out.extend(v.to_be_bytes());
    }
}
pub(crate) fn put_text(out: &mut Vec<u8>, text: &[u8], limits: Limits) -> Result<()> {
    if text.len() > limits.max_text || text.len() > limits.max_bytes {
        return Err(Error::Limit);
    }
    out.extend(
        u32::try_from(text.len())
            .map_err(|_| Error::Limit)?
            .to_be_bytes(),
    );
    out.extend(text);
    Ok(())
}
