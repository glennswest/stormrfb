use crate::{Error, Limits, PixelFormat, Reader, Result, put_text};
use des::{
    Des,
    cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray},
};
pub const VERSION: &[u8; 12] = b"RFB 003.008\n";

/// VNC uses eight password bytes with the bits in each DES key byte reversed.
/// Password encoding is the transport application's responsibility.
pub fn vnc_response(password: &[u8], challenge: [u8; 16]) -> [u8; 16] {
    let mut key = [0; 8];
    for (k, p) in key.iter_mut().zip(password) {
        *k = p.reverse_bits();
    }
    let cipher = Des::new(GenericArray::from_slice(&key));
    let mut out = challenge;
    for block in out.chunks_exact_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(block));
    }
    out
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInit {
    pub width: u16,
    pub height: u16,
    pub format: PixelFormat,
    pub name: Vec<u8>,
}
impl ServerInit {
    pub fn encode(&self, limits: Limits) -> Result<Vec<u8>> {
        limits.pixels(self.width, self.height)?;
        if self.width == 0 || self.height == 0 {
            return Err(Error::Invalid("empty framebuffer"));
        }
        let mut b = Vec::new();
        b.extend(self.width.to_be_bytes());
        b.extend(self.height.to_be_bytes());
        b.extend(self.format.encode()?);
        put_text(&mut b, &self.name, limits)?;
        if b.len() > limits.max_bytes {
            return Err(Error::Limit);
        }
        Ok(b)
    }
    pub fn decode(b: &[u8], limits: Limits) -> Result<(Self, usize)> {
        let mut r = Reader::new(b);
        let width = r.u16()?;
        let height = r.u16()?;
        limits.pixels(width, height)?;
        if width == 0 || height == 0 {
            return Err(Error::Invalid("empty framebuffer"));
        }
        let format = PixelFormat::decode(r.take(16)?)?;
        let name = r.text(limits)?;
        if r.pos > limits.max_bytes {
            return Err(Error::Limit);
        }
        Ok((
            Self {
                width,
                height,
                format,
                name,
            },
            r.pos,
        ))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeEvent {
    Send(Vec<u8>),
    Ready(ServerInit),
}
#[derive(Debug, Clone, Copy)]
enum State {
    Version,
    Security,
    Challenge,
    Result,
    Init,
    Ready,
    Failed,
}
/// RFB 3.8 client handshake. Each step consumes at most one protocol unit.
/// Feed only unconsumed bytes again after Incomplete; all other errors are terminal.
pub struct ClientHandshake {
    state: State,
    password: Option<Vec<u8>>,
    shared: bool,
    limits: Limits,
}
impl ClientHandshake {
    pub fn new(password: Option<Vec<u8>>, shared: bool, limits: Limits) -> Self {
        Self {
            state: State::Version,
            password,
            shared,
            limits,
        }
    }
    pub fn step(&mut self, b: &[u8]) -> Result<(HandshakeEvent, usize)> {
        let result = self.parse(b);
        if matches!(&result,Err(e) if *e!=Error::Incomplete) {
            self.state = State::Failed;
        }
        result
    }
    fn parse(&mut self, b: &[u8]) -> Result<(HandshakeEvent, usize)> {
        let mut r = Reader::new(b);
        let event = match self.state {
            State::Version => {
                if r.take(12)? != VERSION {
                    return Err(Error::Invalid("requires RFB 3.8"));
                }
                self.state = State::Security;
                HandshakeEvent::Send(VERSION.to_vec())
            }
            State::Security => {
                let n = usize::from(r.u8()?);
                if n == 0 {
                    r.text(self.limits)?;
                    return Err(Error::Authentication);
                }
                let types = r.take(n)?;
                let choice = if self.password.is_some() && types.contains(&2) {
                    2
                } else if types.contains(&1) {
                    1
                } else {
                    return Err(Error::Authentication);
                };
                self.state = if choice == 2 {
                    State::Challenge
                } else {
                    State::Result
                };
                HandshakeEvent::Send(vec![choice])
            }
            State::Challenge => {
                let challenge = r.take(16)?.try_into().unwrap();
                let response = vnc_response(
                    self.password.as_deref().ok_or(Error::Authentication)?,
                    challenge,
                );
                self.password = None;
                self.state = State::Result;
                HandshakeEvent::Send(response.to_vec())
            }
            State::Result => {
                if r.u32()? != 0 {
                    r.text(self.limits)?;
                    return Err(Error::Authentication);
                }
                self.password = None;
                self.state = State::Init;
                HandshakeEvent::Send(vec![self.shared as u8])
            }
            State::Init => {
                let (init, n) = ServerInit::decode(b, self.limits)?;
                r.pos = n;
                self.state = State::Ready;
                HandshakeEvent::Ready(init)
            }
            _ => return Err(Error::Invalid("handshake not active")),
        };
        Ok((event, r.pos))
    }
}
