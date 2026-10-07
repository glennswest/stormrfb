//! One RFB session over real TCP on the pod's loopback: the commit's
//! `stormrfb-server` on a thread, a client on the caller's, and a
//! deterministic scene between them, so the client's framebuffer can be
//! checked pixel for pixel against what the server was given.
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use stormrfb::*;
// The crate exports a one-parameter `Result`; these functions return `Result<T, String>`.
use std::result::Result;
use stormrfb_client::{Client, Event as C, Framebuffer};
use stormrfb_server::{Event as S, Security, Server};

/// The server's last message: the client stops when it reads it.
pub const DONE: &[u8] = b"stormrfb-test:done";
/// No session in any suite should take longer than this.
pub const DEADLINE: Duration = Duration::from_secs(120);

/// What the server paints: frame 0 is the whole screen, then a window moves
/// across it one frame per requested update.
#[derive(Clone, Debug)]
pub struct Scene {
    pub width: u16,
    pub height: u16,
    pub frames: u32,
    pub window: (u16, u16),
    /// At this frame the server resizes to (w, h) and repaints it all.
    pub resize: Option<(u32, u16, u16)>,
    /// Write every message in pieces of this many bytes, both ways.
    pub chunk: Option<usize>,
    pub seed: u32,
}

impl Scene {
    pub fn new(width: u16, height: u16, frames: u32) -> Self {
        Self {
            width,
            height,
            frames,
            window: ((width / 4).max(1), (height / 4).max(1)),
            resize: None,
            chunk: None,
            seed: 1,
        }
    }

    /// The damage of frame `k` on a `w`×`h` screen.
    fn damage(&self, k: u32, w: u16, h: u16, repaint: bool) -> (Rect, Vec<[u8; 4]>) {
        if k == 0 || repaint {
            let rect = Rect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            };
            let mut px = Vec::with_capacity(usize::from(w) * usize::from(h));
            for y in 0..h {
                for x in 0..w {
                    px.push([x as u8, y as u8, (x ^ y) as u8 ^ self.seed as u8, 255]);
                }
            }
            return (rect, px);
        }
        let ww = self.window.0.min(w);
        let wh = self.window.1.min(h);
        let rect = Rect {
            x: ((k as u64 * 37) % (u64::from(w - ww) + 1)) as u16,
            y: ((k as u64 * 23) % (u64::from(h - wh) + 1)) as u16,
            width: ww,
            height: wh,
        };
        let mut px = Vec::with_capacity(usize::from(ww) * usize::from(wh));
        for j in 0..wh {
            for i in 0..ww {
                // 8×8 blocks of one colour plus a moving diagonal line:
                // solid areas and edges, like a desktop.
                let block = mix(self.seed, k, u32::from(i / 8), u32::from(j / 8));
                let v = if (u32::from(i) + u32::from(j) + k).is_multiple_of(29) {
                    !block
                } else {
                    block
                };
                px.push([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]);
            }
        }
        (rect, px)
    }

    /// Replay the scene without a server: what the client must end up with.
    pub fn expected(&self) -> (u16, u16, Vec<[u8; 4]>) {
        let (mut w, mut h) = (self.width, self.height);
        let mut fb = vec![[0, 0, 0, 255]; usize::from(w) * usize::from(h)];
        for k in 0..self.frames {
            let repaint = matches!(self.resize, Some((r, ..)) if r == k);
            if let Some((r, nw, nh)) = self.resize
                && r == k
            {
                (w, h) = (nw, nh);
                fb = vec![[0, 0, 0, 255]; usize::from(w) * usize::from(h)];
            }
            let (rect, px) = self.damage(k, w, h, repaint);
            for row in 0..usize::from(rect.height) {
                let at = (usize::from(rect.y) + row) * usize::from(w) + usize::from(rect.x);
                let n = usize::from(rect.width);
                fb[at..at + n].copy_from_slice(&px[row * n..(row + 1) * n]);
            }
        }
        (w, h, fb)
    }
}

fn mix(seed: u32, k: u32, x: u32, y: u32) -> u32 {
    let mut v = seed.wrapping_mul(0x9e37_79b9)
        ^ k.wrapping_mul(0x85eb_ca6b)
        ^ x.wrapping_mul(0xc2b2_ae35)
        ^ y.wrapping_mul(0x27d4_eb2f);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2c1b_3c6d);
    v ^ (v >> 12)
}

/// What the server side saw and sent.
#[derive(Debug, Default)]
pub struct Served {
    pub bytes: u64,
    pub frames: u32,
    /// Input the client sent: keys, pointer, cut text.
    pub input: Vec<S>,
    /// The session's terminal error, if it had one.
    pub error: Option<String>,
}

/// What the client side ended with.
#[derive(Debug, Default)]
pub struct Seen {
    pub name: Vec<u8>,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub rects: u64,
    pub resized: u32,
    pub bells: u32,
    pub cut: Vec<Vec<u8>>,
    pub bytes: u64,
}

pub struct Run {
    pub seen: Result<Seen, String>,
    pub served: Served,
    pub elapsed: Duration,
}

/// Listen on an ephemeral loopback port and serve one connection.
pub fn listen(
    scene: Scene,
    security: Security,
    limits: Limits,
    extras: Vec<Vec<u8>>,
) -> std::io::Result<(SocketAddr, JoinHandle<Served>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let handle = thread::spawn(move || match listener.accept() {
        Ok((stream, _)) => serve(stream, scene, security, limits, extras),
        Err(e) => Served {
            error: Some(format!("accept: {e}")),
            ..Served::default()
        },
    });
    Ok((addr, handle))
}

/// A whole session: server thread, the given client on this one.
pub fn run(
    scene: &Scene,
    security: Security,
    limits: Limits,
    extras: Vec<Vec<u8>>,
    client: impl FnOnce(&mut TcpStream) -> Result<Seen, String>,
) -> std::io::Result<Run> {
    let (addr, handle) = listen(scene.clone(), security, limits, extras)?;
    let start = Instant::now();
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    let seen = client(&mut stream);
    let elapsed = start.elapsed();
    let _ = stream.shutdown(std::net::Shutdown::Both);
    drop(stream);
    let served = handle
        .join()
        .map_err(|_| std::io::Error::other("server thread panicked"))?;
    Ok(Run {
        seen,
        served,
        elapsed,
    })
}

fn write(stream: &mut TcpStream, bytes: &[u8], chunk: Option<usize>) -> std::io::Result<()> {
    match chunk {
        None => stream.write_all(bytes),
        Some(n) => {
            for piece in bytes.chunks(n.max(1)) {
                stream.write_all(piece)?;
                stream.flush()?;
            }
            Ok(())
        }
    }
}

fn serve(
    mut stream: TcpStream,
    scene: Scene,
    security: Security,
    limits: Limits,
    extras: Vec<Vec<u8>>,
) -> Served {
    let mut served = Served::default();
    if let Err(e) = serve_inner(&mut stream, &scene, security, limits, extras, &mut served) {
        served.error = Some(e);
    }
    served
}

fn serve_inner(
    stream: &mut TcpStream,
    scene: &Scene,
    security: Security,
    limits: Limits,
    extras: Vec<Vec<u8>>,
    served: &mut Served,
) -> Result<(), String> {
    let io = |e: std::io::Error| format!("io: {e}");
    stream.set_nodelay(true).map_err(io)?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1)))
        .map_err(io)?;
    let init = ServerInit {
        width: scene.width,
        height: scene.height,
        format: PixelFormat::RGBX,
        name: b"stormrfb-test".to_vec(),
    };
    let mut s = Server::new(init, security, limits).map_err(|e| format!("Server::new: {e}"))?;
    write(stream, s.greeting(), scene.chunk).map_err(io)?;
    let (mut w, mut h) = (scene.width, scene.height);
    let mut ready = false;
    let mut staged = false;
    let mut done = false;
    let mut k = 0;
    let mut buf = vec![0; 64 * 1024];
    let start = Instant::now();
    while start.elapsed() < DEADLINE {
        match stream.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                let events = s.receive(&buf[..n]).map_err(|e| e.to_string())?;
                for e in events {
                    match e {
                        S::Send(b) => {
                            served.bytes += b.len() as u64;
                            write(stream, &b, scene.chunk).map_err(io)?;
                        }
                        S::Ready { .. } => {
                            ready = true;
                            for b in &extras {
                                write(stream, b, scene.chunk).map_err(io)?;
                            }
                        }
                        other => served.input.push(other),
                    }
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::BrokenPipe) => {
                return Ok(());
            }
            Err(e) => return Err(io(e)),
        }
        if !ready {
            continue;
        }
        if !staged && k < scene.frames {
            let repaint = matches!(scene.resize, Some((r, ..)) if r == k);
            if let Some((r, nw, nh)) = scene.resize
                && r == k
            {
                s.resize(nw, nh).map_err(|e| format!("resize: {e}"))?;
                (w, h) = (nw, nh);
            }
            let (rect, px) = scene.damage(k, w, h, repaint);
            s.damage(rect, &px).map_err(|e| format!("damage: {e}"))?;
            staged = true;
        }
        if let Some(b) = s.update().map_err(|e| format!("update: {e}"))? {
            served.bytes += b.len() as u64;
            write(stream, &b, scene.chunk).map_err(io)?;
            if staged {
                staged = false;
                k += 1;
                served.frames = k;
            }
        }
        if k == scene.frames && !staged && !done {
            let b = ServerEvent::CutText(DONE.to_vec())
                .encode_control(limits)
                .map_err(|e| e.to_string())?;
            write(stream, &b, scene.chunk).map_err(io)?;
            done = true;
        }
    }
    Err("server deadline".into())
}

/// Drive the consumers' client, `stormrfb_client::Client`, until the scene is done.
pub fn client(
    stream: &mut TcpStream,
    password: Option<Vec<u8>>,
    limits: Limits,
    inputs: &[ClientMessage],
    chunk: Option<usize>,
) -> Result<Seen, String> {
    let io = |e: std::io::Error| format!("io: {e}");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(io)?;
    let mut c = Client::new(password, limits);
    let mut seen = Seen::default();
    let mut buf = vec![0; 64 * 1024];
    let start = Instant::now();
    let mut done = false;
    while !done {
        if start.elapsed() > DEADLINE {
            return Err("client deadline".into());
        }
        let n = match stream.read(&mut buf) {
            Ok(0) => return Err("server closed the connection".into()),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err("no data from the server for 5 s".into());
            }
            Err(e) => return Err(io(e)),
        };
        seen.bytes += n as u64;
        let mut out: Vec<Vec<u8>> = vec![];
        for e in c.receive(&buf[..n]).map_err(|e| e.to_string())? {
            match e {
                C::Send(b) => out.push(b),
                C::Ready { name } => {
                    seen.name = name;
                    for m in inputs {
                        out.push(c.send(m.clone()).map_err(|e| e.to_string())?);
                    }
                }
                C::Damage(_) => seen.rects += 1,
                C::Resized { .. } => seen.resized += 1,
                C::Bell => seen.bells += 1,
                C::CutText(t) if t == DONE => done = true,
                C::CutText(t) => seen.cut.push(t),
                C::Cursor(_) => {}
            }
        }
        for b in out {
            write(stream, &b, chunk).map_err(io)?;
        }
    }
    let fb = c.framebuffer().ok_or("no framebuffer")?;
    seen.width = fb.width();
    seen.height = fb.height();
    seen.rgba = fb.rgba().to_vec();
    Ok(seen)
}

/// A client built from the codec alone, so the test chooses the pixel
/// format and encodings (`Client` owns both and always prefers ZRLE).
pub fn codec_client(
    stream: &mut TcpStream,
    format: PixelFormat,
    encodings: Vec<i32>,
    limits: Limits,
) -> Result<Seen, String> {
    let io = |e: std::io::Error| format!("io: {e}");
    let err = |e: Error| e.to_string();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(io)?;
    let mut handshake = Some(ClientHandshake::new(None, true, limits));
    let mut decoder: Option<ServerDecoder> = None;
    let mut fb: Option<Framebuffer> = None;
    let mut seen = Seen::default();
    let mut pending: Vec<u8> = vec![];
    let mut buf = vec![0; 64 * 1024];
    let start = Instant::now();
    let mut done = false;
    while !done {
        if start.elapsed() > DEADLINE {
            return Err("client deadline".into());
        }
        let n = match stream.read(&mut buf) {
            Ok(0) => return Err("server closed the connection".into()),
            Ok(n) => n,
            Err(e) => return Err(io(e)),
        };
        seen.bytes += n as u64;
        pending.extend_from_slice(&buf[..n]);
        let mut pos = 0;
        let mut out: Vec<Vec<u8>> = vec![];
        loop {
            if let Some(h) = &mut handshake {
                match h.step(&pending[pos..]) {
                    Ok((event, used)) => {
                        pos += used;
                        match event {
                            HandshakeEvent::Send(b) => out.push(b),
                            HandshakeEvent::Ready(init) => {
                                let f = Framebuffer::new(init.width, init.height, limits)
                                    .map_err(err)?;
                                let full = f.rect();
                                fb = Some(f);
                                decoder = Some(ServerDecoder::new(format, limits).map_err(err)?);
                                handshake = None;
                                seen.name = init.name;
                                for m in [
                                    ClientMessage::SetPixelFormat(format),
                                    ClientMessage::SetEncodings(encodings.clone()),
                                    ClientMessage::UpdateRequest {
                                        incremental: false,
                                        rect: full,
                                    },
                                ] {
                                    out.push(m.encode(limits).map_err(err)?);
                                }
                            }
                        }
                    }
                    Err(Error::Incomplete) => break,
                    Err(e) => return Err(err(e)),
                }
            } else {
                let d = decoder.as_mut().unwrap();
                let f = fb.as_mut().unwrap();
                match d.next(&pending[pos..]) {
                    Ok((event, used)) => {
                        pos += used;
                        match event {
                            ServerEvent::Rectangle(r) => match f.apply(r).map_err(err)? {
                                C::Resized { .. } => seen.resized += 1,
                                _ => seen.rects += 1,
                            },
                            ServerEvent::UpdateEnd => out.push(
                                ClientMessage::UpdateRequest {
                                    incremental: true,
                                    rect: f.rect(),
                                }
                                .encode(limits)
                                .map_err(err)?,
                            ),
                            ServerEvent::CutText(t) if t == DONE => done = true,
                            ServerEvent::CutText(t) => seen.cut.push(t),
                            ServerEvent::Bell => seen.bells += 1,
                            _ => {}
                        }
                    }
                    Err(Error::Incomplete) => break,
                    Err(e) => return Err(err(e)),
                }
            }
        }
        pending.drain(..pos);
        for b in out {
            stream.write_all(&b).map_err(io)?;
        }
    }
    let f = fb.ok_or("no framebuffer")?;
    seen.width = f.width();
    seen.height = f.height();
    seen.rgba = f.rgba().to_vec();
    Ok(seen)
}

/// Does the client hold exactly the scene, as `format` can carry it?
pub fn check(scene: &Scene, seen: &Seen, format: PixelFormat) -> Result<(), String> {
    let (w, h, expected) = scene.expected();
    if (seen.width, seen.height) != (w, h) {
        return Err(format!(
            "framebuffer {}x{}, expected {w}x{h}",
            seen.width, seen.height
        ));
    }
    for (i, px) in expected.iter().enumerate() {
        let mut wire = vec![];
        format.write(*px, &mut wire).map_err(|e| e.to_string())?;
        let want = format.read(&wire).map_err(|e| e.to_string())?;
        let got = &seen.rgba[i * 4..i * 4 + 4];
        if got != want {
            let (x, y) = (i % usize::from(w), i / usize::from(w));
            return Err(format!("pixel ({x},{y}) is {got:?}, expected {want:?}"));
        }
    }
    Ok(())
}

/// FNV-1a over RGBA, as the crates' fixture tests hash.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
