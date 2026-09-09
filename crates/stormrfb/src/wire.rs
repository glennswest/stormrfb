use crate::{Error, Limits, PixelFormat, Reader, Rect, Result, put_rect, put_text};
pub const RAW: i32 = 0;
pub const COPY_RECT: i32 = 1;
pub const HEXTILE: i32 = 5;
pub const ZRLE: i32 = 16;
pub const CURSOR: i32 = -239;
pub const DESKTOP_SIZE: i32 = -223;
pub const LAST_RECT: i32 = -224;
pub const ENCODINGS: &[i32] = &[
    ZRLE,
    HEXTILE,
    COPY_RECT,
    RAW,
    CURSOR,
    DESKTOP_SIZE,
    LAST_RECT,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    SetPixelFormat(PixelFormat),
    SetEncodings(Vec<i32>),
    UpdateRequest { incremental: bool, rect: Rect },
    Key { down: bool, keysym: u32 },
    Pointer { buttons: u8, x: u16, y: u16 },
    CutText(Vec<u8>),
}
impl ClientMessage {
    pub fn encode(&self, limits: Limits) -> Result<Vec<u8>> {
        let mut b = Vec::new();
        match self {
            Self::SetPixelFormat(f) => {
                b.extend([0, 0, 0, 0]);
                b.extend(f.encode()?);
            }
            Self::SetEncodings(v) => {
                if v.len() > limits.max_rectangles {
                    return Err(Error::Limit);
                }
                b.extend([2, 0]);
                b.extend(
                    u16::try_from(v.len())
                        .map_err(|_| Error::Limit)?
                        .to_be_bytes(),
                );
                for n in v {
                    b.extend(n.to_be_bytes());
                }
            }
            Self::UpdateRequest { incremental, rect } => {
                b.extend([3, *incremental as u8]);
                put_rect(&mut b, *rect);
            }
            Self::Key { down, keysym } => {
                b.extend([4, *down as u8, 0, 0]);
                b.extend(keysym.to_be_bytes());
            }
            Self::Pointer { buttons, x, y } => {
                b.extend([5, *buttons]);
                b.extend(x.to_be_bytes());
                b.extend(y.to_be_bytes());
            }
            Self::CutText(t) => {
                b.extend([6, 0, 0, 0]);
                put_text(&mut b, t, limits)?;
            }
        }
        if b.len() > limits.max_bytes {
            return Err(Error::Limit);
        }
        Ok(b)
    }
    /// Returns a message and consumed bytes. Incomplete input has no side effects.
    pub fn decode(bytes: &[u8], limits: Limits) -> Result<(Self, usize)> {
        let mut r = Reader::new(bytes);
        let m = match r.u8()? {
            0 => {
                r.take(3)?;
                Self::SetPixelFormat(PixelFormat::decode(r.take(16)?)?)
            }
            2 => {
                r.take(1)?;
                let n = usize::from(r.u16()?);
                if n > limits.max_rectangles || n * 4 > limits.max_bytes {
                    return Err(Error::Limit);
                }
                r.data.get(r.pos..r.pos + n * 4).ok_or(Error::Incomplete)?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(r.u32()? as i32);
                }
                Self::SetEncodings(v)
            }
            3 => {
                let incremental = boolean(r.u8()?)?;
                Self::UpdateRequest {
                    incremental,
                    rect: r.rect()?,
                }
            }
            4 => {
                let down = boolean(r.u8()?)?;
                r.take(2)?;
                Self::Key {
                    down,
                    keysym: r.u32()?,
                }
            }
            5 => Self::Pointer {
                buttons: r.u8()?,
                x: r.u16()?,
                y: r.u16()?,
            },
            6 => {
                r.take(3)?;
                Self::CutText(r.text(limits)?)
            }
            n => return Err(Error::Unsupported(i32::from(n))),
        };
        if r.pos > limits.max_bytes {
            return Err(Error::Limit);
        }
        Ok((m, r.pos))
    }
}
pub(crate) fn boolean(n: u8) -> Result<bool> {
    match n {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Invalid("boolean")),
    }
}
