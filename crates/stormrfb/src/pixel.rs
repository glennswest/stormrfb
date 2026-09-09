use crate::{Error, Reader, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: bool,
    pub red_max: u16,
    pub green_max: u16,
    pub blue_max: u16,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
}
impl Default for PixelFormat {
    fn default() -> Self {
        Self::RGBX
    }
}
impl PixelFormat {
    pub const RGBX: Self = Self {
        bits_per_pixel: 32,
        depth: 24,
        big_endian: false,
        red_max: 255,
        green_max: 255,
        blue_max: 255,
        red_shift: 0,
        green_shift: 8,
        blue_shift: 16,
    };
    pub fn validate(self) -> Result<()> {
        if ![16, 32].contains(&self.bits_per_pixel)
            || self.depth == 0
            || self.depth > self.bits_per_pixel
        {
            return Err(Error::Invalid("pixel depth"));
        }
        let mut used = 0u64;
        for (max, shift) in [
            (self.red_max, self.red_shift),
            (self.green_max, self.green_shift),
            (self.blue_max, self.blue_shift),
        ] {
            if max == 0 || !(u32::from(max) + 1).is_power_of_two() || shift >= self.bits_per_pixel {
                return Err(Error::Invalid("pixel channel"));
            }
            let mask = u64::from(max) << shift;
            if mask >> self.bits_per_pixel != 0 || used & mask != 0 {
                return Err(Error::Invalid("overlapping pixel channels"));
            }
            used |= mask;
        }
        if used.count_ones() > u32::from(self.depth) {
            return Err(Error::Invalid("pixel depth"));
        }
        Ok(())
    }
    pub fn bytes(self) -> usize {
        usize::from(self.bits_per_pixel / 8)
    }
    pub fn encode(self) -> Result<[u8; 16]> {
        self.validate()?;
        let mut b = [0; 16];
        b[..4].copy_from_slice(&[self.bits_per_pixel, self.depth, self.big_endian as u8, 1]);
        b[4..6].copy_from_slice(&self.red_max.to_be_bytes());
        b[6..8].copy_from_slice(&self.green_max.to_be_bytes());
        b[8..10].copy_from_slice(&self.blue_max.to_be_bytes());
        b[10..13].copy_from_slice(&[self.red_shift, self.green_shift, self.blue_shift]);
        Ok(b)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let bpp = r.u8()?;
        let depth = r.u8()?;
        let be = r.u8()?;
        let tc = r.u8()?;
        if be > 1 || tc != 1 {
            return Err(Error::Invalid("only true colour supported"));
        }
        let f = Self {
            bits_per_pixel: bpp,
            depth,
            big_endian: be != 0,
            red_max: r.u16()?,
            green_max: r.u16()?,
            blue_max: r.u16()?,
            red_shift: r.u8()?,
            green_shift: r.u8()?,
            blue_shift: r.u8()?,
        };
        r.take(3)?;
        f.validate()?;
        Ok(f)
    }
    pub fn read(self, bytes: &[u8]) -> Result<[u8; 4]> {
        self.validate()?;
        let b = bytes.get(..self.bytes()).ok_or(Error::Incomplete)?;
        let value = match (self.bits_per_pixel, self.big_endian) {
            (16, false) => u32::from(u16::from_le_bytes(b.try_into().unwrap())),
            (16, true) => u32::from(u16::from_be_bytes(b.try_into().unwrap())),
            (32, false) => u32::from_le_bytes(b.try_into().unwrap()),
            _ => u32::from_be_bytes(b.try_into().unwrap()),
        };
        let channel = |max: u16, shift: u8| {
            (((value >> shift) & u32::from(max)) * 255 / u32::from(max)) as u8
        };
        Ok([
            channel(self.red_max, self.red_shift),
            channel(self.green_max, self.green_shift),
            channel(self.blue_max, self.blue_shift),
            255,
        ])
    }
    pub fn write(self, rgba: [u8; 4], out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        let v = ((u32::from(rgba[0]) * u32::from(self.red_max) / 255) << self.red_shift)
            | ((u32::from(rgba[1]) * u32::from(self.green_max) / 255) << self.green_shift)
            | ((u32::from(rgba[2]) * u32::from(self.blue_max) / 255) << self.blue_shift);
        match (self.bits_per_pixel, self.big_endian) {
            (16, false) => out.extend((v as u16).to_le_bytes()),
            (16, true) => out.extend((v as u16).to_be_bytes()),
            (32, false) => out.extend(v.to_le_bytes()),
            _ => out.extend(v.to_be_bytes()),
        };
        Ok(())
    }
}
