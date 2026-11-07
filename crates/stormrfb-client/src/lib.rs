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
/// Renderer seam for native consumers, so another renderer can be added
/// without touching the client. Nothing in this workspace implements it:
/// the WASM binding and the development harness read [`Framebuffer::rgba`].
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
                let row_bytes = usize::from(rect.width) * 4;
                if row_bytes != 0 {
                    let source = pixels.as_flattened();
                    for row in 0..usize::from(rect.height) {
                        let offset = ((usize::from(rect.y) + row) * usize::from(self.width)
                            + usize::from(rect.x))
                            * 4;
                        let target = &mut self.rgba[offset..offset + row_bytes];
                        target.copy_from_slice(&source[row * row_bytes..(row + 1) * row_bytes]);
                        // Public callers may supply non-opaque pixels, even though
                        // the wire decoder already normalizes RGBX alpha.
                        for alpha in target.iter_mut().skip(3).step_by(4) {
                            *alpha = 255;
                        }
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
            // Also a layout announcement or a refusal: reallocate (and
            // clear) only when the size changed. [`Client`] does not pass
            // it on unless it did.
            Rectangle::ExtendedDesktopSize { width, height, .. } => {
                if (width, height) != (self.width, self.height) {
                    *self = Self::new(width, height, self.limits)?;
                }
                Ok(Event::Resized { width, height })
            }
            // A capability, not a picture: [`Client`] consumes it.
            Rectangle::QemuExtendedKey => Err(Error::Invalid("not a framebuffer rectangle")),
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
    extended_keys: bool,
    desktop_resize: bool,
    screens: Vec<Screen>,
    resize_status: Option<u16>,
    paused: bool,
    updates: u64,
    /// The server sent a fence (it supports -312).
    server_fence: bool,
    /// The server sent EndOfContinuousUpdates (it supports -313).
    server_continuous: bool,
    /// Continuous updates are enabled (asked for and not stopped).
    continuous: bool,
    /// The server stopped continuous updates on its own: do not ask again.
    continuous_refused: bool,
    /// Stops sent (by `pause`) whose EndOfContinuousUpdates has not come.
    stops_sent: u32,
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
            extended_keys: false,
            desktop_resize: false,
            screens: Vec::new(),
            resize_status: None,
            paused: false,
            updates: 0,
            server_fence: false,
            server_continuous: false,
            continuous: false,
            continuous_refused: false,
            stops_sent: 0,
        }
    }
    /// Whether continuous updates (-313) are on: the server sends updates
    /// without requests. The client turns them on by itself once the server
    /// has shown it supports both ContinuousUpdates and Fence, as noVNC
    /// does, and answers the server's fences.
    pub fn continuous(&self) -> bool {
        self.continuous
    }
    fn whole(&self) -> Rect {
        self.framebuffer
            .as_ref()
            .map_or(Rect::default(), |f| f.rect())
    }
    /// EnableContinuousUpdates for the whole framebuffer, when the server
    /// takes it and it is not on, paused or refused.
    fn enable_continuous(&mut self) -> Result<Option<Vec<u8>>> {
        if !self.server_fence
            || !self.server_continuous
            || self.continuous
            || self.continuous_refused
            || self.paused
        {
            return Ok(None);
        }
        self.continuous = true;
        ClientMessage::EnableContinuousUpdates {
            enable: true,
            rect: self.whole(),
        }
        .encode(self.limits)
        .map(Some)
    }
    pub fn framebuffer(&self) -> Option<&Framebuffer> {
        self.framebuffer.as_ref()
    }
    /// Whether the server acknowledged QEMU Extended Key Event (-258), so
    /// [`ClientMessage::QemuKey`] may be sent. It arrives with the first
    /// update after the handshake, so it is `false` until then.
    pub fn extended_keys(&self) -> bool {
        self.extended_keys
    }
    /// Whether the server sent an ExtendedDesktopSize (-308) rectangle, so
    /// [`Client::request_resize`] may be used. A server that supports it
    /// sends one with the first update after the handshake.
    pub fn desktop_resize(&self) -> bool {
        self.desktop_resize
    }
    /// The screen layout from the server's latest ExtendedDesktopSize;
    /// empty until one arrives.
    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }
    /// The status of the server's latest answer to a
    /// [`Client::request_resize`]: `RESIZE_OK` or a `RESIZE_*` error code.
    /// `None` until an answer arrives. A granted size arrives as
    /// [`Event::Resized`].
    pub fn resize_status(&self) -> Option<u16> {
        self.resize_status
    }
    /// Stop asking for updates: the update in flight still arrives, and no
    /// request follows it, so the server sends nothing more. For a viewer
    /// that is not on screen (a console's VT switched away). Returns bytes
    /// to send: with continuous updates on, the message that stops them;
    /// otherwise nothing.
    pub fn pause(&mut self) -> Result<Vec<u8>> {
        self.paused = true;
        if !self.continuous {
            return Ok(vec![]);
        }
        self.continuous = false;
        self.stops_sent += 1;
        ClientMessage::EnableContinuousUpdates {
            enable: false,
            rect: self.whole(),
        }
        .encode(self.limits)
    }
    /// Whether [`Client::pause`] is in effect.
    pub fn paused(&self) -> bool {
        self.paused
    }
    /// Ask for updates again, starting with the whole framebuffer
    /// (non-incremental), so nothing drawn while paused is missed. Returns
    /// the bytes to send: the request, and continuous updates turned back
    /// on where the server takes them.
    pub fn resume(&mut self) -> Result<Vec<u8>> {
        if self.failed || self.handshake.is_some() {
            return Err(Error::Invalid("client not ready"));
        }
        self.paused = false;
        self.resized = false;
        let mut b = ClientMessage::UpdateRequest {
            incremental: false,
            rect: self.whole(),
        }
        .encode(self.limits)?;
        if let Some(e) = self.enable_continuous()? {
            b.extend(e);
        }
        Ok(b)
    }
    /// FramebufferUpdates completed so far.
    pub fn updates(&self) -> u64 {
        self.updates
    }
    /// SetDesktopSize: ask the server for a `width` x `height` framebuffer,
    /// as one screen that keeps the id and flags of the current first
    /// screen. `Unsupported(EXTENDED_DESKTOP_SIZE)` before
    /// [`Client::desktop_resize`].
    pub fn request_resize(&self, width: u16, height: u16) -> Result<Vec<u8>> {
        if width == 0 || height == 0 {
            return Err(Error::Invalid("empty framebuffer"));
        }
        let first = self.screens.first();
        let mut screen = Screen::whole(first.map_or(0, |s| s.id), width, height);
        screen.flags = first.map_or(0, |s| s.flags);
        self.send(ClientMessage::SetDesktopSize {
            width,
            height,
            screens: vec![screen],
        })
    }
    /// Encode a client message. The client owns `SetPixelFormat` and
    /// `SetEncodings`; `QemuKey` before [`Client::extended_keys`] is
    /// `Unsupported(QEMU_EXTENDED_KEY)`.
    pub fn send(&self, message: ClientMessage) -> Result<Vec<u8>> {
        if self.failed || self.handshake.is_some() {
            return Err(Error::Invalid("client not ready"));
        }
        if matches!(
            message,
            ClientMessage::SetPixelFormat(_)
                | ClientMessage::SetEncodings(_)
                | ClientMessage::Fence { .. }
                | ClientMessage::EnableContinuousUpdates { .. }
        ) {
            return Err(Error::Invalid(
                "client owns format, encodings, fences and continuous updates",
            ));
        }
        if matches!(message, ClientMessage::QemuKey { .. }) && !self.extended_keys {
            return Err(Error::Unsupported(QEMU_EXTENDED_KEY));
        }
        if matches!(message, ClientMessage::SetDesktopSize { .. }) && !self.desktop_resize {
            return Err(Error::Unsupported(EXTENDED_DESKTOP_SIZE));
        }
        message.encode(self.limits)
    }
    /// A key, by scancode where the server takes one and by keysym where it
    /// does not. `keycode` is [`qemu_keycode`]'s value. With it and an
    /// acknowledging server this sends `QemuKey` (and `keysym` may be 0);
    /// otherwise it sends `Key`, which needs a keysym.
    pub fn key_event(&self, down: bool, keysym: u32, keycode: Option<u32>) -> Result<Vec<u8>> {
        match keycode {
            Some(keycode) if self.extended_keys => self.send(ClientMessage::QemuKey {
                down,
                keysym,
                keycode,
            }),
            _ if keysym == 0 => Err(Error::Unsupported(QEMU_EXTENDED_KEY)),
            _ => self.send(ClientMessage::Key { down, keysym }),
        }
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
            if events.len()
                > self
                    .limits
                    .max_rectangles
                    .saturating_mul(4)
                    .saturating_add(16)
            {
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
                            ServerEvent::Rectangle(Rectangle::QemuExtendedKey) => {
                                self.extended_keys = true;
                            }
                            ServerEvent::Rectangle(Rectangle::ExtendedDesktopSize {
                                reason,
                                status,
                                width,
                                height,
                                screens,
                            }) => {
                                self.desktop_resize = true;
                                self.screens = screens;
                                if reason == RESIZE_BY_CLIENT {
                                    self.resize_status = Some(status);
                                }
                                let f = self.framebuffer.as_mut().unwrap();
                                if (width, height) != (f.width(), f.height()) {
                                    events.push(f.apply(Rectangle::DesktopSize { width, height })?);
                                    self.resized = true;
                                }
                            }
                            ServerEvent::Rectangle(rect) => {
                                let e = self.framebuffer.as_mut().unwrap().apply(rect)?;
                                if matches!(e, Event::Resized { .. }) {
                                    self.resized = true;
                                }
                                events.push(e);
                            }
                            ServerEvent::UpdateEnd => {
                                self.updates += 1;
                                if self.continuous && self.resized {
                                    // The same area, at the new size.
                                    self.resized = false;
                                    events.push(Event::Send(
                                        ClientMessage::EnableContinuousUpdates {
                                            enable: true,
                                            rect: self.whole(),
                                        }
                                        .encode(self.limits)?,
                                    ));
                                } else if !self.paused && !self.continuous {
                                    events.push(Event::Send(
                                        ClientMessage::UpdateRequest {
                                            incremental: !self.resized,
                                            rect: self.framebuffer.as_ref().unwrap().rect(),
                                        }
                                        .encode(self.limits)?,
                                    ));
                                    self.resized = false;
                                }
                            }
                            ServerEvent::Fence { flags, payload } => {
                                self.server_fence = true;
                                if flags & FENCE_REQUEST != 0 {
                                    events.push(Event::Send(
                                        ClientMessage::Fence {
                                            flags: flags & FENCE_FLAGS,
                                            payload,
                                        }
                                        .encode(self.limits)?,
                                    ));
                                }
                                if let Some(b) = self.enable_continuous()? {
                                    events.push(Event::Send(b));
                                }
                            }
                            ServerEvent::EndOfContinuousUpdates => {
                                self.server_continuous = true;
                                if self.stops_sent > 0 {
                                    // The answer to a pause()'s stop.
                                    self.stops_sent -= 1;
                                } else if self.continuous {
                                    // Stopped by the server, not by pause():
                                    // back to asking.
                                    self.continuous = false;
                                    self.continuous_refused = true;
                                    if !self.paused {
                                        events.push(Event::Send(
                                            ClientMessage::UpdateRequest {
                                                incremental: true,
                                                rect: self.whole(),
                                            }
                                            .encode(self.limits)?,
                                        ));
                                    }
                                }
                                if let Some(b) = self.enable_continuous()? {
                                    events.push(Event::Send(b));
                                }
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
    if let Some(n) = key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=12).contains(n))
    {
        return Some(0xffbd + n);
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
