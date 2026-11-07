//! The viewer without its I/O: RFB bytes and evdev events in; bytes to send,
//! VT switches and a screen-sized picture out. The binary (`main.rs`) owns
//! the socket, the VT, DRM and the input devices; this owns what they mean,
//! so the tests drive it against `stormrfb-server` with no hardware.

use stormrfb::{ClientMessage, Limits, Rect};
use stormrfb_client::{Client, Cursor, Event};

use crate::canvas::Canvas;
use crate::fit::Fit;
use crate::keys;

/// What the input devices said, already decoded from `struct input_event`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// A key or button: `value` 1 down, 0 up, 2 autorepeat.
    Key {
        code: u16,
        value: i32,
    },
    /// An absolute pointer's position, 0.0–1.0 across the whole screen.
    AbsX(f64),
    AbsY(f64),
    /// A relative pointer's motion, in screen pixels.
    RelX(i32),
    RelY(i32),
    /// Wheel notches: positive up (or right for `HWheel`).
    Wheel(i32),
    HWheel(i32),
    /// The end of one device report (EV_SYN SYN_REPORT).
    Sync,
}

/// What to do with the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write these bytes to the server.
    Send(Vec<u8>),
    /// Switch the console to this VT (the keys for it were pressed here).
    Switch(u16),
}

/// Which keys switch VTs, as they do on the text consoles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchKeys {
    /// Alt+Fn and Ctrl+Alt+Fn (the kernel's default keymap). The guest
    /// never sees Alt+Fn.
    Alt,
    /// Only Ctrl+Alt+Fn; Alt+Fn goes to the guest.
    CtrlAlt,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub switch: SwitchKeys,
    /// Ask the guest for the screen's size where it takes ExtendedDesktopSize.
    pub resize: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            switch: SwitchKeys::Alt,
            resize: true,
        }
    }
}

/// Which guest pixels have been drawn since the picture was last stale.
struct Fresh {
    width: usize,
    seen: Vec<bool>,
    missing: usize,
}

impl Fresh {
    fn new(width: usize, height: usize) -> Fresh {
        Fresh {
            width,
            seen: vec![false; width * height],
            missing: width * height,
        }
    }

    fn mark(&mut self, r: Rect) {
        let (x0, x1) = (usize::from(r.x), usize::from(r.x) + usize::from(r.width));
        for y in usize::from(r.y)..usize::from(r.y) + usize::from(r.height) {
            let row = y * self.width;
            let Some(cells) = self.seen.get_mut(row + x0..row + x1) else {
                return;
            };
            for s in cells {
                if !*s {
                    *s = true;
                    self.missing -= 1;
                }
            }
        }
    }
}

pub struct Viewer {
    client: Client,
    opts: Options,
    canvas: Canvas,
    fit: Option<Fit>,
    ready: bool,
    /// On screen (the VT is ours). Input is taken only then.
    active: bool,
    /// Some while waiting for every guest pixel to be redrawn: after the
    /// handshake, a resize, and every acquire. No picture is shown until
    /// then, so a switch back never shows the frame from before it.
    fresh: Option<Fresh>,
    updates: u64,
    changed: bool,
    resize_asked: bool,
    cursor: Option<Cursor>,
    /// The pointer on the screen, in screen pixels.
    pointer: (f64, f64),
    pointer_moved: bool,
    buttons: u8,
    /// Keys sent down to the guest and not yet up, in press order.
    held: Vec<u16>,
    /// Keys taken here (a VT switch) whose release is not the guest's.
    swallowed: Vec<u16>,
    shift: [bool; 2],
    ctrl: [bool; 2],
    alt: [bool; 2],
    caps: bool,
}

impl Viewer {
    /// A viewer for a screen `width` x `height`, before the handshake.
    pub fn new(
        width: usize,
        height: usize,
        password: Option<Vec<u8>>,
        limits: Limits,
        opts: Options,
    ) -> Viewer {
        Viewer {
            client: Client::new(password, limits),
            opts,
            canvas: Canvas::new(width, height),
            fit: None,
            ready: false,
            active: true,
            fresh: None,
            updates: 0,
            changed: true,
            resize_asked: false,
            cursor: None,
            pointer: (width as f64 / 2.0, height as f64 / 2.0),
            pointer_moved: false,
            buttons: 0,
            held: vec![],
            swallowed: vec![],
            shift: [false; 2],
            ctrl: [false; 2],
            alt: [false; 2],
            caps: false,
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The handshake is done and the guest's framebuffer is known.
    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// The fitted picture, once every pixel of it is current; `None` while
    /// it is starting, resizing or coming back from a switch.
    pub fn frame(&self) -> Option<&Canvas> {
        (self.ready && self.fresh.is_none()).then_some(&self.canvas)
    }

    /// Whether the screen needs presenting again since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The guest pixel under the pointer.
    pub fn pointer(&self) -> Option<(u16, u16)> {
        self.fit
            .as_ref()
            .map(|f| f.to_guest(self.pointer.0, self.pointer.1))
    }

    /// Draws the guest's cursor (sent as the Cursor pseudo-encoding, so not
    /// in the framebuffer) at the pointer, into a screen-sized buffer.
    pub fn draw_cursor(&self, dst: &mut [u32], stride: usize) {
        let (Some(fit), Some(c), Some((gx, gy))) = (&self.fit, &self.cursor, self.pointer()) else {
            return;
        };
        fit.sprite(
            dst,
            stride,
            i32::from(gx) - i32::from(c.x),
            i32::from(gy) - i32::from(c.y),
            usize::from(c.width),
            usize::from(c.height),
            &c.rgba,
        );
    }

    /// Bytes from the server. An error ends the session: reconnect with a
    /// new `Viewer`.
    pub fn receive(&mut self, data: &[u8]) -> stormrfb::Result<Vec<Action>> {
        let mut out = vec![];
        for e in self.client.receive(data)? {
            match e {
                Event::Send(b) => out.push(Action::Send(b)),
                Event::Ready { .. } => {
                    self.ready = true;
                    self.reframe();
                    if !self.active {
                        out.extend(self.pause()?);
                    }
                }
                Event::Resized { .. } => self.reframe(),
                Event::Damage(r) => self.paint(r),
                Event::Cursor(c) => {
                    self.cursor = Some(c);
                    self.changed = true;
                }
                Event::Bell | Event::CutText(_) => {}
            }
        }
        if self.client.updates() != self.updates {
            self.updates = self.client.updates();
            if self.fresh.as_ref().is_some_and(|f| f.missing == 0) {
                self.fresh = None;
                self.changed = true;
            }
            if let Some(b) = self.resize_request()? {
                out.push(Action::Send(b));
            }
        }
        Ok(out)
    }

    /// SetDesktopSize for the screen's size, once per session, when the
    /// server takes it and the guest is another size.
    fn resize_request(&mut self) -> stormrfb::Result<Option<Vec<u8>>> {
        if !self.opts.resize || self.resize_asked || !self.client.desktop_resize() {
            return Ok(None);
        }
        self.resize_asked = true;
        let (Ok(w), Ok(h)) = (
            u16::try_from(self.canvas.width),
            u16::try_from(self.canvas.height),
        ) else {
            return Ok(None);
        };
        let fb = self.client.framebuffer().map(|f| (f.width(), f.height()));
        if fb == Some((w, h)) {
            return Ok(None);
        }
        self.client.request_resize(w, h).map(Some)
    }

    /// The guest framebuffer is new (or a new size): fit it again and wait
    /// for all of it.
    fn reframe(&mut self) {
        let Some(fb) = self.client.framebuffer() else {
            return;
        };
        let (w, h) = (usize::from(fb.width()), usize::from(fb.height()));
        self.fit = Some(Fit::new(w, h, self.canvas.width, self.canvas.height));
        self.canvas.clear(0);
        self.fresh = Some(Fresh::new(w, h));
        self.changed = true;
    }

    fn paint(&mut self, r: Rect) {
        let (Some(fit), Some(fb)) = (&self.fit, self.client.framebuffer()) else {
            return;
        };
        fit.blit(fb.rgba(), r, &mut self.canvas);
        if let Some(f) = &mut self.fresh {
            f.mark(r);
        }
        self.changed = true;
    }

    /// Pauses the client: with continuous updates on, the message that
    /// stops them.
    fn pause(&mut self) -> stormrfb::Result<Option<Action>> {
        let b = self.client.pause()?;
        Ok((!b.is_empty()).then_some(Action::Send(b)))
    }

    /// The VT is being switched away: let go of every key and button the
    /// guest thinks is held, and stop asking for updates.
    pub fn release(&mut self) -> Vec<Action> {
        let mut out = vec![];
        if self.ready {
            for code in std::mem::take(&mut self.held).into_iter().rev() {
                if let Ok(b) = self.key(false, code) {
                    out.push(Action::Send(b));
                }
            }
            if self.buttons != 0 {
                self.buttons = 0;
                out.extend(self.pointer_message().map(Action::Send));
            }
            if let Ok(b) = self.pause() {
                out.extend(b);
            }
        }
        self.held.clear();
        self.swallowed.clear();
        self.shift = [false; 2];
        self.ctrl = [false; 2];
        self.alt = [false; 2];
        self.pointer_moved = false;
        self.active = false;
        out
    }

    /// The VT is ours again: ask for the whole framebuffer, and show
    /// nothing from before until it has all arrived.
    pub fn acquire(&mut self) -> Vec<Action> {
        self.active = true;
        self.changed = true;
        if !self.ready {
            return vec![];
        }
        if let Some(fb) = self.client.framebuffer() {
            self.fresh = Some(Fresh::new(
                usize::from(fb.width()),
                usize::from(fb.height()),
            ));
        }
        self.client.resume().map(Action::Send).into_iter().collect()
    }

    /// One input event. Ignored while the VT is away or before the
    /// handshake.
    pub fn input(&mut self, ev: Input) -> Vec<Action> {
        if !self.active || !self.ready {
            return vec![];
        }
        let (w, h) = (self.canvas.width as f64, self.canvas.height as f64);
        match ev {
            Input::Key { code, value } => {
                if let Some(bit) = keys::button(code) {
                    let before = self.buttons;
                    if value == 0 {
                        self.buttons &= !bit;
                    } else {
                        self.buttons |= bit;
                    }
                    self.pointer_moved |= before != self.buttons;
                    return vec![];
                }
                return self.keyboard(code, value);
            }
            Input::AbsX(f) => self.pointer.0 = (f.clamp(0.0, 1.0) * w).min(w - 1.0),
            Input::AbsY(f) => self.pointer.1 = (f.clamp(0.0, 1.0) * h).min(h - 1.0),
            Input::RelX(d) => self.pointer.0 = self.clamp_x(self.pointer.0 + f64::from(d)),
            Input::RelY(d) => self.pointer.1 = self.clamp_y(self.pointer.1 + f64::from(d)),
            Input::Wheel(n) => return self.wheel(n, 8, 16),
            Input::HWheel(n) => return self.wheel(n, 64, 32),
            Input::Sync => {
                if std::mem::take(&mut self.pointer_moved) {
                    if self.cursor.is_some() {
                        self.changed = true;
                    }
                    return self
                        .pointer_message()
                        .map(Action::Send)
                        .into_iter()
                        .collect();
                }
                return vec![];
            }
        }
        self.pointer_moved = true;
        vec![]
    }

    fn clamp_x(&self, x: f64) -> f64 {
        match &self.fit {
            Some(f) => x.clamp(f.x as f64, (f.x + f.w - 1) as f64),
            None => x,
        }
    }

    fn clamp_y(&self, y: f64) -> f64 {
        match &self.fit {
            Some(f) => y.clamp(f.y as f64, (f.y + f.h - 1) as f64),
            None => y,
        }
    }

    fn pointer_message(&self) -> Option<Vec<u8>> {
        let (x, y) = self.pointer()?;
        self.client
            .send(ClientMessage::Pointer {
                buttons: self.buttons,
                x,
                y,
            })
            .ok()
    }

    /// A wheel notch is a press and release of button 4/5 (6/7 sideways).
    fn wheel(&mut self, n: i32, up: u8, down: u8) -> Vec<Action> {
        let bit = if n > 0 { up } else { down };
        let mut out = vec![];
        for _ in 0..n.unsigned_abs().min(16) {
            for b in [self.buttons | bit, self.buttons] {
                let (x, y) = self.pointer().unwrap_or((0, 0));
                if let Ok(m) = self
                    .client
                    .send(ClientMessage::Pointer { buttons: b, x, y })
                {
                    out.push(Action::Send(m));
                }
            }
        }
        out
    }

    fn keyboard(&mut self, code: u16, value: i32) -> Vec<Action> {
        let down = value != 0;
        match code {
            keys::KEY_LEFTSHIFT => self.shift[0] = down,
            keys::KEY_RIGHTSHIFT => self.shift[1] = down,
            keys::KEY_LEFTCTRL => self.ctrl[0] = down,
            keys::KEY_RIGHTCTRL => self.ctrl[1] = down,
            keys::KEY_LEFTALT => self.alt[0] = down,
            keys::KEY_RIGHTALT => self.alt[1] = down,
            keys::KEY_CAPSLOCK if value == 1 => self.caps = !self.caps,
            _ => {}
        }
        if value == 0
            && let Some(i) = self.swallowed.iter().position(|&c| c == code)
        {
            self.swallowed.remove(i);
            return vec![];
        }
        if value == 1
            && let Some(vt) = self.switch_to(code)
        {
            self.swallowed.push(code);
            return vec![Action::Switch(vt)];
        }
        let known = self.held.contains(&code);
        match value {
            // A release of a key pressed before this VT was ours.
            0 if !known => return vec![],
            0 => self.held.retain(|&c| c != code),
            1 if !known => self.held.push(code),
            // Autorepeat of a key the guest never saw go down.
            2 if !known => return vec![],
            _ => {}
        }
        self.key(down, code).map(Action::Send).into_iter().collect()
    }

    fn switch_to(&self, code: u16) -> Option<u16> {
        let vt = keys::function_key(code)?;
        let alt = self.alt.iter().any(|&a| a);
        let ctrl = self.ctrl.iter().any(|&c| c);
        match self.opts.switch {
            SwitchKeys::Alt if alt => Some(vt),
            SwitchKeys::CtrlAlt if alt && ctrl => Some(vt),
            _ => None,
        }
    }

    /// A key as the server takes it: QEMU extended (scancode, the guest's
    /// own layout) where acknowledged, else a US-layout keysym.
    fn key(&self, down: bool, code: u16) -> stormrfb::Result<Vec<u8>> {
        let shift = self.shift.iter().any(|&s| s) ^ (self.caps && keys::is_letter(code));
        self.client
            .key_event(down, keys::keysym(code, shift), keys::scancode(code))
    }
}
