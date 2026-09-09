//! One sans-I/O RFB server session per client. Transport supplies authentication entropy.
#![forbid(unsafe_code)]
use stormrfb::*;
/// Supply a fresh cryptographically random challenge for every VNC-auth session.
pub enum Security {
    None,
    Vnc {
        password: Vec<u8>,
        challenge: [u8; 16],
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Send(Vec<u8>),
    Ready { shared: bool },
    Key { down: bool, keysym: u32 },
    Pointer { buttons: u8, x: u16, y: u16 },
    CutText(Vec<u8>),
}
#[derive(Clone, Copy)]
enum State {
    Version,
    Security,
    Auth,
    Init,
    Ready,
    Failed,
}
pub struct Server {
    state: State,
    security: Security,
    init: ServerInit,
    format: PixelFormat,
    encodings: Vec<i32>,
    encoder: ServerEncoder,
    buffer: Vec<u8>,
    limits: Limits,
    pixels: Vec<[u8; 4]>,
    dirty: Option<Rect>,
    request: Option<(bool, Rect)>,
    pending_resize: bool,
}
impl Server {
    pub fn new(init: ServerInit, security: Security, limits: Limits) -> Result<Self> {
        init.encode(limits)?;
        let n = limits.pixels(init.width, init.height)?;
        let dirty = Some(Rect {
            width: init.width,
            height: init.height,
            ..Rect::default()
        });
        Ok(Self {
            state: State::Version,
            format: init.format,
            init,
            security,
            encodings: vec![RAW],
            encoder: ServerEncoder::new(limits),
            buffer: vec![],
            limits,
            pixels: vec![[0, 0, 0, 255]; n],
            dirty,
            request: None,
            pending_resize: false,
        })
    }
    /// Resize a ready session only after DesktopSize was negotiated. The resize
    /// is sent as the final rectangle of the next requested update.
    pub fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        if !matches!(self.state, State::Ready) || !self.encodings.contains(&DESKTOP_SIZE) {
            return Err(Error::Unsupported(DESKTOP_SIZE));
        }
        let n = self.limits.pixels(width, height)?;
        if n == 0 {
            return Err(Error::Invalid("empty framebuffer"));
        }
        self.pixels = vec![[0, 0, 0, 255]; n];
        self.init.width = width;
        self.init.height = height;
        self.dirty = Some(Rect {
            width,
            height,
            ..Rect::default()
        });
        self.pending_resize = true;
        Ok(())
    }
    pub fn greeting(&self) -> &'static [u8] {
        VERSION
    }
    /// Replace a damaged rectangle in the server's canonical RGBA framebuffer.
    pub fn damage(&mut self, rect: Rect, pixels: &[[u8; 4]]) -> Result<()> {
        if !rect.within(self.init.width, self.init.height)
            || pixels.len() != usize::from(rect.width) * usize::from(rect.height)
        {
            return Err(Error::Invalid("damage bounds"));
        }
        if rect.width == 0 || rect.height == 0 {
            return Ok(());
        }
        for row in 0..usize::from(rect.height) {
            let start =
                (usize::from(rect.y) + row) * usize::from(self.init.width) + usize::from(rect.x);
            self.pixels[start..start + usize::from(rect.width)].copy_from_slice(
                &pixels[row * usize::from(rect.width)..(row + 1) * usize::from(rect.width)],
            );
        }
        self.dirty = Some(self.dirty.map_or(rect, |old| union(old, rect)));
        Ok(())
    }
    pub fn receive(&mut self, data: &[u8]) -> Result<Vec<Event>> {
        if matches!(self.state, State::Failed) {
            return Err(Error::Invalid("server failed"));
        }
        let result = self.receive_inner(data);
        if result.is_err() {
            self.state = State::Failed;
        }
        result
    }
    fn receive_inner(&mut self, data: &[u8]) -> Result<Vec<Event>> {
        if self
            .buffer
            .len()
            .checked_add(data.len())
            .ok_or(Error::Limit)?
            > self.limits.max_bytes
        {
            return Err(Error::Limit);
        }
        self.buffer.extend(data);
        let mut pos = 0;
        let mut out = vec![];
        loop {
            if out.len() > self.limits.max_rectangles * 4 + 16 {
                return Err(Error::Limit);
            }
            let b = &self.buffer[pos..];
            match self.state {
                State::Version => {
                    if b.len() < 12 {
                        break;
                    }
                    if &b[..12] != VERSION {
                        return Err(Error::Invalid("requires RFB 3.8"));
                    }
                    pos += 12;
                    out.push(Event::Send(vec![
                        1,
                        if matches!(self.security, Security::None) {
                            1
                        } else {
                            2
                        },
                    ]));
                    self.state = State::Security;
                }
                State::Security => {
                    let Some(choice) = b.first() else {
                        break;
                    };
                    pos += 1;
                    match &self.security {
                        Security::None if *choice == 1 => {
                            out.push(Event::Send(vec![0; 4]));
                            self.state = State::Init;
                        }
                        Security::Vnc { challenge, .. } if *choice == 2 => {
                            out.push(Event::Send(challenge.to_vec()));
                            self.state = State::Auth;
                        }
                        _ => return Err(Error::Authentication),
                    }
                }
                State::Auth => {
                    if b.len() < 16 {
                        break;
                    }
                    let Security::Vnc {
                        password,
                        challenge,
                    } = &self.security
                    else {
                        unreachable!()
                    };
                    let expected = vnc_response(password, *challenge);
                    let mut difference = 0u8;
                    for (a, b) in expected.iter().zip(&b[..16]) {
                        difference |= a ^ b;
                    }
                    pos += 16;
                    self.security = Security::None;
                    if difference != 0 {
                        self.state = State::Failed;
                        let mut failure = vec![0, 0, 0, 1, 0, 0, 0, 21];
                        failure.extend(b"Authentication failed");
                        out.push(Event::Send(failure));
                        break;
                    }
                    out.push(Event::Send(vec![0; 4]));
                    self.state = State::Init;
                }
                State::Init => {
                    let Some(shared) = b.first() else {
                        break;
                    };
                    if *shared > 1 {
                        return Err(Error::Invalid("shared flag"));
                    }
                    pos += 1;
                    out.push(Event::Send(self.init.encode(self.limits)?));
                    out.push(Event::Ready {
                        shared: *shared != 0,
                    });
                    self.state = State::Ready;
                }
                State::Ready => {
                    let (message, n) = match ClientMessage::decode(b, self.limits) {
                        Ok(v) => v,
                        Err(Error::Incomplete) => break,
                        Err(e) => return Err(e),
                    };
                    pos += n;
                    match message {
                        ClientMessage::SetPixelFormat(format) => self.format = format,
                        ClientMessage::SetEncodings(encodings) => self.encodings = encodings,
                        ClientMessage::UpdateRequest { incremental, rect } => {
                            if !rect.within(self.init.width, self.init.height) {
                                return Err(Error::Invalid("update request bounds"));
                            }
                            self.request =
                                Some(self.request.map_or((incremental, rect), |(old, area)| {
                                    (old && incremental, union(area, rect))
                                }));
                        }
                        ClientMessage::Key { down, keysym } => {
                            out.push(Event::Key { down, keysym })
                        }
                        ClientMessage::Pointer { buttons, x, y } => out.push(Event::Pointer {
                            buttons,
                            x: x.min(self.init.width - 1),
                            y: y.min(self.init.height - 1),
                        }),
                        ClientMessage::CutText(t) => out.push(Event::CutText(t)),
                    }
                }
                State::Failed => break,
            }
        }
        self.buffer.drain(..pos);
        Ok(out)
    }
    /// Emit at most one outstanding requested update. Incremental requests wait for damage.
    pub fn update(&mut self) -> Result<Option<Vec<u8>>> {
        if !matches!(self.state, State::Ready) {
            return Ok(None);
        }
        let Some((incremental, area)) = self.request else {
            return Ok(None);
        };
        if self.pending_resize {
            let bytes = self.encoder.update(
                &[Rectangle::DesktopSize {
                    width: self.init.width,
                    height: self.init.height,
                }],
                self.format,
                RAW,
            )?;
            self.pending_resize = false;
            self.request = None;
            return Ok(Some(bytes));
        }
        let rect = if incremental {
            let Some(dirty) = self.dirty else {
                return Ok(None);
            };
            let Some(rect) = intersection(area, dirty) else {
                return Ok(None);
            };
            rect
        } else {
            area
        };
        let mut pixels = Vec::with_capacity(usize::from(rect.width) * usize::from(rect.height));
        for row in usize::from(rect.y)..usize::from(rect.y) + usize::from(rect.height) {
            let start = row * usize::from(self.init.width) + usize::from(rect.x);
            pixels.extend_from_slice(&self.pixels[start..start + usize::from(rect.width)]);
        }
        let encoding = self
            .encodings
            .iter()
            .find(|e| [RAW, HEXTILE, ZRLE].contains(e))
            .copied()
            .unwrap_or(RAW);
        let bytes =
            self.encoder
                .update(&[Rectangle::Pixels { rect, pixels }], self.format, encoding)?;
        if self
            .dirty
            .is_some_and(|dirty| intersection(dirty, rect) == Some(dirty))
        {
            self.dirty = None;
        }
        self.request = None;
        Ok(Some(bytes))
    }
}
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        width: ((u32::from(a.x) + u32::from(a.width)).max(u32::from(b.x) + u32::from(b.width))
            - u32::from(x)) as u16,
        height: ((u32::from(a.y) + u32::from(a.height)).max(u32::from(b.y) + u32::from(b.height))
            - u32::from(y)) as u16,
    }
}
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (u32::from(a.x) + u32::from(a.width)).min(u32::from(b.x) + u32::from(b.width));
    let bottom = (u32::from(a.y) + u32::from(a.height)).min(u32::from(b.y) + u32::from(b.height));
    if right <= u32::from(x) || bottom <= u32::from(y) {
        None
    } else {
        Some(Rect {
            x,
            y,
            width: (right - u32::from(x)) as u16,
            height: (bottom - u32::from(y)) as u16,
        })
    }
}
