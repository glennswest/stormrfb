//! Transport-independent framebuffer client. Feed a byte stream, drain events.
#![forbid(unsafe_code)]
use stormrfb::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Send(Vec<u8>),
    Ready { name: Vec<u8> },
    Damage(Rect),
    Resized { width: u16, height: u16 },
    Cursor(Cursor),
    Bell,
    CutText(Vec<u8>),
}
/// Renderer seam: browser and development harness consume the same framebuffer.
pub trait Renderer {
    type Error;
    fn paint(
        &mut self,
        framebuffer: &Framebuffer,
        damage: Rect,
    ) -> std::result::Result<(), Self::Error>;
}
pub struct Framebuffer {
    width: u16,
    height: u16,
    rgba: Vec<u8>,
    limits: Limits,
}
impl Framebuffer {
    pub fn new(width: u16, height: u16, limits: Limits) -> Result<Self> {
        let n = limits.pixels(width, height)?;
        if n == 0 {
            return Err(Error::Invalid("empty framebuffer"));
        }
        let mut rgba = vec![0; n * 4];
        for a in rgba.iter_mut().skip(3).step_by(4) {
            *a = 255;
        }
        Ok(Self {
            width,
            height,
            rgba,
            limits,
        })
    }
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    pub fn rect(&self) -> Rect {
        Rect {
            width: self.width,
            height: self.height,
            ..Rect::default()
        }
    }
    pub fn apply(&mut self, rectangle: Rectangle) -> Result<Event> {
        match rectangle {
            Rectangle::Pixels { rect, pixels } => {
                if !rect.within(self.width, self.height)
                    || pixels.len() != usize::from(rect.width) * usize::from(rect.height)
                {
                    return Err(Error::Invalid("framebuffer rectangle"));
                }
                for row in 0..usize::from(rect.height) {
                    for col in 0..usize::from(rect.width) {
                        let offset = ((usize::from(rect.y) + row) * usize::from(self.width)
                            + usize::from(rect.x)
                            + col)
                            * 4;
                        self.rgba[offset..offset + 4]
                            .copy_from_slice(&pixels[row * usize::from(rect.width) + col]);
                        self.rgba[offset + 3] = 255;
                    }
                }
                Ok(Event::Damage(rect))
            }
            Rectangle::Copy {
                rect,
                source_x,
                source_y,
            } => {
                let source = Rect {
                    x: source_x,
                    y: source_y,
                    ..rect
                };
                if !rect.within(self.width, self.height) || !source.within(self.width, self.height)
                {
                    return Err(Error::Invalid("CopyRect bounds"));
                }
                let stride = usize::from(self.width) * 4;
                let count = usize::from(rect.width) * 4;
                // Vertical direction plus memmove within each row handles all overlap.
                for i in 0..usize::from(rect.height) {
                    let row = if rect.y > source_y {
                        usize::from(rect.height) - 1 - i
                    } else {
                        i
                    };
                    let src = (usize::from(source_y) + row) * stride + usize::from(source_x) * 4;
                    let dst = (usize::from(rect.y) + row) * stride + usize::from(rect.x) * 4;
                    self.rgba.copy_within(src..src + count, dst);
                }
                Ok(Event::Damage(rect))
            }
            Rectangle::DesktopSize { width, height } => {
                *self = Self::new(width, height, self.limits)?;
                Ok(Event::Resized { width, height })
            }
            Rectangle::Cursor {
                hotspot_x,
                hotspot_y,
                width,
                height,
                pixels,
                mask,
            } => {
                let n = self.limits.pixels(width, height)?;
                let stride = usize::from(width).div_ceil(8);
                if pixels.len() != n
                    || mask.len() != stride * usize::from(height)
                    || (n != 0 && (hotspot_x >= width || hotspot_y >= height))
                {
                    return Err(Error::Invalid("cursor"));
                }
                let mut rgba = Vec::with_capacity(n * 4);
                for (i, p) in pixels.iter().enumerate() {
                    rgba.extend(p);
                    rgba[i * 4 + 3] = if mask
                        [(i / usize::from(width)) * stride + (i % usize::from(width)) / 8]
                        & (128 >> (i % usize::from(width) % 8))
                        != 0
                    {
                        255
                    } else {
                        0
                    };
                }
                Ok(Event::Cursor(Cursor {
                    x: hotspot_x,
                    y: hotspot_y,
                    width,
                    height,
                    rgba,
                }))
            }
        }
    }
}
pub struct Client {
    handshake: Option<ClientHandshake>,
    decoder: Option<ServerDecoder>,
    framebuffer: Option<Framebuffer>,
    buffer: Vec<u8>,
    limits: Limits,
    failed: bool,
    resized: bool,
}
impl Client {
    pub fn new(password: Option<Vec<u8>>, limits: Limits) -> Self {
        Self {
            handshake: Some(ClientHandshake::new(password, true, limits)),
            decoder: None,
            framebuffer: None,
            buffer: Vec::new(),
            limits,
            failed: false,
            resized: false,
        }
    }
    pub fn framebuffer(&self) -> Option<&Framebuffer> {
        self.framebuffer.as_ref()
    }
    pub fn send(&self, message: ClientMessage) -> Result<Vec<u8>> {
        if self.failed || self.handshake.is_some() {
            return Err(Error::Invalid("client not ready"));
        }
        if matches!(
            message,
            ClientMessage::SetPixelFormat(_) | ClientMessage::SetEncodings(_)
        ) {
            return Err(Error::Invalid("client owns format and encodings"));
        }
        message.encode(self.limits)
    }
    /// Input and returned events are bounded per call. Drain returned sends in order.
    /// Any error is terminal; reconnect using a fresh Client.
    pub fn receive(&mut self, data: &[u8]) -> Result<Vec<Event>> {
        if self.failed {
            return Err(Error::Invalid("client failed"));
        }
        let result = self.receive_inner(data);
        if result.is_err() {
            self.failed = true;
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
        let mut events = vec![];
        loop {
            if events.len() > self.limits.max_rectangles * 4 + 16 {
                return Err(Error::Limit);
            }
            if let Some(handshake) = &mut self.handshake {
                match handshake.step(&self.buffer[pos..]) {
                    Ok((event, n)) => {
                        pos += n;
                        match event {
                            HandshakeEvent::Send(b) => events.push(Event::Send(b)),
                            HandshakeEvent::Ready(init) => {
                                self.framebuffer =
                                    Some(Framebuffer::new(init.width, init.height, self.limits)?);
                                self.decoder =
                                    Some(ServerDecoder::new(PixelFormat::RGBX, self.limits)?);
                                self.handshake = None;
                                events.push(Event::Ready { name: init.name });
                                for m in [
                                    ClientMessage::SetPixelFormat(PixelFormat::RGBX),
                                    ClientMessage::SetEncodings(ENCODINGS.to_vec()),
                                    ClientMessage::UpdateRequest {
                                        incremental: false,
                                        rect: self.framebuffer.as_ref().unwrap().rect(),
                                    },
                                ] {
                                    events.push(Event::Send(m.encode(self.limits)?));
                                }
                            }
                        }
                    }
                    Err(Error::Incomplete) => break,
                    Err(e) => return Err(e),
                }
            } else {
                match self.decoder.as_mut().unwrap().next(&self.buffer[pos..]) {
                    Ok((event, n)) => {
                        pos += n;
                        match event {
                            ServerEvent::Rectangle(rect) => {
                                let e = self.framebuffer.as_mut().unwrap().apply(rect)?;
                                if matches!(e, Event::Resized { .. }) {
                                    self.resized = true;
                                }
                                events.push(e);
                            }
                            ServerEvent::UpdateEnd => {
                                events.push(Event::Send(
                                    ClientMessage::UpdateRequest {
                                        incremental: !self.resized,
                                        rect: self.framebuffer.as_ref().unwrap().rect(),
                                    }
                                    .encode(self.limits)?,
                                ));
                                self.resized = false;
                            }
                            ServerEvent::Bell => events.push(Event::Bell),
                            ServerEvent::CutText(t) => events.push(Event::CutText(t)),
                            ServerEvent::UpdateStart | ServerEvent::ColourMap { .. } => {}
                        }
                    }
                    Err(Error::Incomplete) => break,
                    Err(e) => return Err(e),
                }
            }
        }
        self.buffer.drain(..pos);
        Ok(events)
    }
}
/// DOM KeyboardEvent.key to X11 keysym (Unicode keysyms for non-Latin-1).
pub fn keysym(key: &str) -> Option<u32> {
    let special = match key {
        "Backspace" => 0xff08,
        "Tab" => 0xff09,
        "Enter" => 0xff0d,
        "Escape" => 0xff1b,
        "Insert" => 0xff63,
        "Delete" => 0xffff,
        "Home" => 0xff50,
        "End" => 0xff57,
        "PageUp" => 0xff55,
        "PageDown" => 0xff56,
        "ArrowLeft" => 0xff51,
        "ArrowUp" => 0xff52,
        "ArrowRight" => 0xff53,
        "ArrowDown" => 0xff54,
        "Shift" => 0xffe1,
        "Control" => 0xffe3,
        "Alt" => 0xffe9,
        "Meta" => 0xffeb,
        "CapsLock" => 0xffe5,
        "NumLock" => 0xff7f,
        "ScrollLock" => 0xff14,
        "Pause" => 0xff13,
        "PrintScreen" => 0xff61,
        _ => 0,
    };
    if special != 0 {
        return Some(special);
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=12).contains(&n) {
            return Some(0xffbd + n);
        }
    }
    let mut chars = key.chars();
    let c = chars.next()? as u32;
    if chars.next().is_some() {
        return None;
    }
    Some(if c <= 255 { c } else { 0x01000000 | c })
}
pub fn pointer_buttons(dom: u16) -> u8 {
    ((dom & 1) | ((dom & 4) >> 1) | ((dom & 2) << 1)) as u8
}
