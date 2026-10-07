//! One sans-I/O RFB server session per client. Transport supplies authentication entropy.
//!
//! Updates are pixel rectangles only (never CopyRect or Cursor), encoded with
//! the first of Raw/Hextile/ZRLE in the client's `SetEncodings` order; the
//! encoder picks each Hextile and ZRLE tile's subencoding (solid, palette,
//! RLE, subrectangles) by size, raw only when nothing is smaller. A client that advertises
//! QEMU Extended Key Event (-258) is acknowledged in the next update, and
//! its extended key events arrive as [`Event::QemuKey`].
//!
//! A client that advertises ExtendedDesktopSize (-308) is sent the layout
//! in the next update and every resize as -308 (otherwise as DesktopSize).
//! Its SetDesktopSize arrives as [`Event::SetDesktopSize`], and the
//! application answers with [`Server::accept_resize`] or
//! [`Server::refuse_resize`]. A request outside [`Limits`] or with an invalid
//! layout is refused without an event.
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
    Ready {
        shared: bool,
    },
    Key {
        down: bool,
        keysym: u32,
    },
    /// A QEMU extended key event: `keycode` is qemu's number (XT make code,
    /// bit 7 for the 0xE0 prefix; see `stormrfb::qemu_keycode`).
    QemuKey {
        down: bool,
        keysym: u32,
        keycode: u32,
    },
    Pointer {
        buttons: u8,
        x: u16,
        y: u16,
    },
    CutText(Vec<u8>),
    /// The client asks for this size and layout (SetDesktopSize). It fits
    /// the session's [`Limits`] and `stormrfb::valid_layout` holds. Answer
    /// with [`Server::accept_resize`] or [`Server::refuse_resize`]; a newer
    /// request replaces an unanswered one.
    SetDesktopSize {
        width: u16,
        height: u16,
        screens: Vec<Screen>,
    },
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
    /// -258 was advertised and not yet acknowledged.
    ack_extended_keys: bool,
    extended_keys_acked: bool,
    screens: Vec<Screen>,
    /// -308 was newly advertised: send the layout in the next update.
    announce_layout: bool,
    /// The client's unanswered SetDesktopSize.
    requested: Option<(u16, u16, Vec<Screen>)>,
    /// Status of the latest answer to the client, not yet sent.
    resize_reply: Option<u16>,
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
            ack_extended_keys: false,
            extended_keys_acked: false,
            screens: vec![Screen::whole(0, init.width, init.height)],
            announce_layout: false,
            requested: None,
            resize_reply: None,
        })
    }
    fn extended_desktop_size(&self) -> bool {
        self.encodings.contains(&EXTENDED_DESKTOP_SIZE)
    }
    /// Resize a ready session only after DesktopSize or ExtendedDesktopSize
    /// was negotiated. The layout becomes one screen (keeping the first
    /// screen's id). The resize is sent alone as the next requested update,
    /// as ExtendedDesktopSize when the client advertised it.
    pub fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        if !matches!(self.state, State::Ready)
            || !(self.encodings.contains(&DESKTOP_SIZE) || self.extended_desktop_size())
        {
            return Err(Error::Unsupported(DESKTOP_SIZE));
        }
        let id = self.screens.first().map_or(0, |s| s.id);
        self.set_size(width, height, vec![Screen::whole(id, width, height)])
    }
    /// Grant the client's latest [`Event::SetDesktopSize`]: the framebuffer
    /// becomes that size and layout (cleared to black, so supply its pixels
    /// with [`Server::damage`]), and the next update answers the client with
    /// status `RESIZE_OK`. A later [`Server::resize`] still overrides it.
    pub fn accept_resize(&mut self) -> Result<()> {
        let Some((width, height, screens)) = self.requested.take() else {
            return Err(Error::Invalid("no resize request"));
        };
        self.set_size(width, height, screens)?;
        self.resize_reply = Some(RESIZE_OK);
        Ok(())
    }
    /// Refuse the client's latest [`Event::SetDesktopSize`] with a
    /// `RESIZE_*` error status (`RESIZE_PROHIBITED`, …), sent with the
    /// current size and layout in the next update.
    pub fn refuse_resize(&mut self, status: u16) -> Result<()> {
        if status == RESIZE_OK {
            return Err(Error::Invalid("refusal status"));
        }
        if self.requested.take().is_none() {
            return Err(Error::Invalid("no resize request"));
        }
        self.resize_reply = Some(status);
        Ok(())
    }
    fn set_size(&mut self, width: u16, height: u16, screens: Vec<Screen>) -> Result<()> {
        let n = self.limits.pixels(width, height)?;
        if n == 0 {
            return Err(Error::Invalid("empty framebuffer"));
        }
        self.screens = screens;
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
            if out.len()
                > self
                    .limits
                    .max_rectangles
                    .saturating_mul(4)
                    .saturating_add(16)
            {
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
                        ClientMessage::SetEncodings(encodings) => {
                            self.ack_extended_keys =
                                encodings.contains(&QEMU_EXTENDED_KEY) && !self.extended_keys_acked;
                            let extended = encodings.contains(&EXTENDED_DESKTOP_SIZE);
                            self.announce_layout = extended
                                && (self.announce_layout || !self.extended_desktop_size());
                            self.encodings = encodings;
                        }
                        ClientMessage::SetDesktopSize {
                            width,
                            height,
                            screens,
                        } => {
                            if !self.extended_desktop_size() {
                                return Err(Error::Unsupported(EXTENDED_DESKTOP_SIZE));
                            }
                            if !valid_layout(width, height, &screens) {
                                self.requested = None;
                                self.resize_reply = Some(RESIZE_INVALID_LAYOUT);
                            } else if self.limits.pixels(width, height).is_err() {
                                self.requested = None;
                                self.resize_reply = Some(RESIZE_OUT_OF_RESOURCES);
                            } else {
                                self.requested = Some((width, height, screens.clone()));
                                out.push(Event::SetDesktopSize {
                                    width,
                                    height,
                                    screens,
                                });
                            }
                        }
                        ClientMessage::UpdateRequest { incremental, rect } => {
                            // The client still knows the old dimensions until it
                            // receives DesktopSize; accept its triggering request.
                            if self.pending_resize {
                                self.request = Some((
                                    false,
                                    Rect {
                                        width: self.init.width,
                                        height: self.init.height,
                                        ..Rect::default()
                                    },
                                ));
                                continue;
                            }
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
                        ClientMessage::QemuKey {
                            down,
                            keysym,
                            keycode,
                        } => out.push(Event::QemuKey {
                            down,
                            keysym,
                            keycode,
                        }),
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
            let rect = self.layout().unwrap_or(Rectangle::DesktopSize {
                width: self.init.width,
                height: self.init.height,
            });
            let bytes = self.encoder.update(&[rect], self.format, RAW)?;
            self.pending_resize = false;
            self.announce_layout = false;
            self.resize_reply = None;
            self.request = None;
            return Ok(Some(bytes));
        }
        let rect = if incremental {
            self.dirty.and_then(|dirty| intersection(area, dirty))
        } else {
            Some(area)
        };
        let layout = self.layout();
        // An acknowledgement or a resize answer answers a request on its
        // own, damage or not.
        if rect.is_none() && !self.ack_extended_keys && layout.is_none() {
            return Ok(None);
        }
        let mut rects = Vec::with_capacity(3);
        if let Some(rect) = rect {
            let mut pixels = Vec::with_capacity(usize::from(rect.width) * usize::from(rect.height));
            for row in usize::from(rect.y)..usize::from(rect.y) + usize::from(rect.height) {
                let start = row * usize::from(self.init.width) + usize::from(rect.x);
                pixels.extend_from_slice(&self.pixels[start..start + usize::from(rect.width)]);
            }
            rects.push(Rectangle::Pixels { rect, pixels });
        }
        rects.extend(layout);
        if self.ack_extended_keys {
            rects.push(Rectangle::QemuExtendedKey);
        }
        let encoding = self
            .encodings
            .iter()
            .find(|e| [RAW, HEXTILE, ZRLE].contains(e))
            .copied()
            .unwrap_or(RAW);
        let bytes = self.encoder.update(&rects, self.format, encoding)?;
        if self.ack_extended_keys {
            self.ack_extended_keys = false;
            self.extended_keys_acked = true;
        }
        self.announce_layout = false;
        self.resize_reply = None;
        if let Some(rect) = rect
            && self
                .dirty
                .is_some_and(|dirty| intersection(dirty, rect) == Some(dirty))
        {
            self.dirty = None;
        }
        self.request = None;
        Ok(Some(bytes))
    }
}
impl Server {
    /// The ExtendedDesktopSize rectangle the next update carries, if any:
    /// one per update, with the current size and layout, answering the
    /// client's latest request when there is an answer to send.
    fn layout(&self) -> Option<Rectangle> {
        if !self.extended_desktop_size()
            || !(self.pending_resize || self.announce_layout || self.resize_reply.is_some())
        {
            return None;
        }
        Some(Rectangle::ExtendedDesktopSize {
            reason: if self.resize_reply.is_some() {
                RESIZE_BY_CLIENT
            } else {
                RESIZE_BY_SERVER
            },
            status: self.resize_reply.unwrap_or(RESIZE_OK),
            width: self.init.width,
            height: self.init.height,
            screens: self.screens.clone(),
        })
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
