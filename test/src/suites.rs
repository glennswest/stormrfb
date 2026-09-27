//! The short and medium suites.
//!
//! short (< 2 min): the commit's server and client complete a session over
//! loopback TCP with the framebuffer exact, VNC auth admits and refuses, the
//! recorded QEMU and TigerVNC streams decode to their captured hashes, and
//! every encoding the server emits round-trips in every pixel-format shape.
//!
//! medium (< 30 min): short, then input, resize, fragmentation, hostile
//! input both ways, mutated real streams, small limits, concurrent sessions,
//! a 1080p moving window measured, and an optional real server.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use stormrfb::*;
// The crate exports a one-parameter `Result`; these functions return `Result<T, String>`.
use std::result::Result;
use stormrfb_client::{Client, Event as C, Framebuffer};
use stormrfb_server::{Event as S, Security};

use crate::env::{self, Env};
use crate::report::{Outcome, Report};
use crate::session::{self, Scene, check, fnv};

const QEMU: &[u8] = include_bytes!("../../fixtures/qemu-zrle.rfb");
const TIGERVNC: &[u8] = include_bytes!("../../fixtures/tigervnc-zrle.rfb");

pub fn short(env: &Env, r: &mut Report) {
    let _ = env;
    r.run("loopback-session", loopback_session);
    r.run("vnc-auth", vnc_auth);
    r.run("fixture-qemu", || fixture(QEMU, 720, 400, 0x06133f0cfae0b305));
    r.run("fixture-tigervnc", || {
        fixture(TIGERVNC, 73, 69, 0x091a3de23b92f334)
    });
    r.run("encodings-and-formats", encodings_and_formats);
}

pub fn medium(env: &Env, r: &mut Report) {
    short(env, r);
    r.run("input-events", input_events);
    r.run("resize", resize);
    r.run("fragmentation", fragmentation);
    r.run("hostile-client-bytes", hostile_client_bytes);
    r.run("hostile-server-bytes", hostile_server_bytes);
    let budget = (env.timeout / 6).min(Duration::from_secs(300));
    r.run("mutated-fixtures", || mutated_fixtures(budget));
    r.run("small-limits", small_limits);
    r.run("concurrent-sessions", concurrent_sessions);
    r.run("moving-window-1080p", moving_window_1080p);
    r.run("real-server", || real_server(env));
}

fn io(e: std::io::Error) -> Outcome {
    Outcome::Infra(format!("loopback: {e}"))
}

macro_rules! fail {
    ($($t:tt)*) => { return Outcome::Fail(format!($($t)*)) };
}

fn per_frame(elapsed: Duration, frames: u32) -> f64 {
    elapsed.as_secs_f64() * 1000.0 / f64::from(frames.max(1))
}

fn loopback_session() -> Outcome {
    let scene = Scene::new(640, 480, 30);
    let run = match session::run(&scene, Security::None, Limits::default(), vec![], |s| {
        session::client(s, None, Limits::default(), &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    let seen = match run.seen {
        Ok(seen) => seen,
        Err(e) => fail!("client: {e}; server: {:?}", run.served.error),
    };
    if let Err(e) = check(&scene, &seen, PixelFormat::RGBX) {
        fail!("{e}");
    }
    if seen.name != b"stormrfb-test" {
        fail!("desktop name {:?}", String::from_utf8_lossy(&seen.name));
    }
    Outcome::Pass(format!(
        "640x480, {} frames exact over TCP, {} bytes to the client, {:.3} ms/frame",
        run.served.frames,
        seen.bytes,
        per_frame(run.elapsed, run.served.frames)
    ))
}

fn challenge() -> [u8; 16] {
    // The test's own challenge, not a secret: the crate takes it from its caller.
    let mut c = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom")
        && f.read_exact(&mut c).is_ok()
    {
        return c;
    }
    let t = Instant::now().elapsed().as_nanos() as u64 ^ u64::from(std::process::id());
    for (i, b) in c.iter_mut().enumerate() {
        *b = (t >> (i % 8 * 8)) as u8 ^ i as u8;
    }
    c
}

fn vnc_auth() -> Outcome {
    let scene = Scene::new(64, 48, 3);
    let secure = || Security::Vnc {
        password: b"stormrfb".to_vec(),
        challenge: challenge(),
    };
    let right = match session::run(&scene, secure(), Limits::default(), vec![], |s| {
        session::client(s, Some(b"stormrfb".to_vec()), Limits::default(), &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    match &right.seen {
        Ok(seen) => {
            if let Err(e) = check(&scene, seen, PixelFormat::RGBX) {
                fail!("right password: {e}");
            }
        }
        Err(e) => fail!("right password refused: {e}"),
    }
    let wrong = match session::run(&scene, secure(), Limits::default(), vec![], |s| {
        session::client(s, Some(b"wrong".to_vec()), Limits::default(), &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    match wrong.seen {
        Err(e) if e.contains("Authentication") => {}
        Err(e) => fail!("wrong password: expected Authentication, got {e}"),
        Ok(_) => fail!("wrong password admitted"),
    }
    if wrong.served.frames != 0 {
        fail!("wrong password: the server sent {} frames", wrong.served.frames);
    }
    let none = match session::run(&scene, secure(), Limits::default(), vec![], |s| {
        session::client(s, None, Limits::default(), &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    if none.seen.is_ok() {
        fail!("no password admitted to a VNC-auth server");
    }
    Outcome::Pass("right password admitted with the scene exact; wrong and absent refused".into())
}

/// Replay a recorded server stream through the codec and the framebuffer;
/// the hashes were captured independently (QMP screendump, XGetImage).
fn fixture(bytes: &[u8], width: u16, height: u16, expected: u64) -> Outcome {
    let limits = Limits::default();
    let mut decoder = match ServerDecoder::new(PixelFormat::RGBX, limits) {
        Ok(d) => d,
        Err(e) => fail!("{e}"),
    };
    let mut fb = match Framebuffer::new(width, height, limits) {
        Ok(f) => f,
        Err(e) => fail!("{e}"),
    };
    let (mut pos, mut updates) = (0, 0);
    let start = Instant::now();
    loop {
        match decoder.next(&bytes[pos..]) {
            Ok((event, n)) => {
                pos += n;
                match event {
                    ServerEvent::Rectangle(rect) => {
                        if let Err(e) = fb.apply(rect) {
                            fail!("apply at byte {pos}: {e}");
                        }
                    }
                    ServerEvent::UpdateEnd => {
                        updates += 1;
                        let hash = fnv(fb.rgba());
                        if hash != expected {
                            fail!("update {updates}: hash {hash:#018x}, expected {expected:#018x}");
                        }
                    }
                    _ => {}
                }
            }
            Err(Error::Incomplete) => break,
            Err(e) => fail!("{e} at byte {pos}"),
        }
    }
    if pos != bytes.len() || updates != 2 {
        fail!("consumed {pos}/{} bytes, {updates} updates", bytes.len());
    }
    Outcome::Pass(format!(
        "{width}x{height}, {} bytes, 2 updates match the captured hash in {:.3} ms",
        bytes.len(),
        start.elapsed().as_secs_f64() * 1000.0
    ))
}

fn formats() -> [(&'static str, PixelFormat); 4] {
    let rgb565 = PixelFormat {
        bits_per_pixel: 16,
        depth: 16,
        big_endian: false,
        red_max: 31,
        green_max: 63,
        blue_max: 31,
        red_shift: 11,
        green_shift: 5,
        blue_shift: 0,
    };
    let bgrx_be = PixelFormat {
        big_endian: true,
        red_shift: 16,
        green_shift: 8,
        blue_shift: 0,
        ..PixelFormat::RGBX
    };
    [
        ("rgbx32le", PixelFormat::RGBX),
        ("bgrx32be", bgrx_be),
        ("rgb565le", rgb565),
        (
            "rgb565be",
            PixelFormat {
                big_endian: true,
                ..rgb565
            },
        ),
    ]
}

/// Every encoding the server emits, in every pixel-format shape the codec
/// accepts, through a codec-built client.
fn encodings_and_formats() -> Outcome {
    let mut done = vec![];
    for (name, format) in formats() {
        for (enc_name, enc) in [("raw", RAW), ("hextile", HEXTILE), ("zrle", ZRLE)] {
            let mut scene = Scene::new(200, 150, 8);
            scene.seed = enc as u32 ^ u32::from(format.bits_per_pixel);
            let run = match session::run(&scene, Security::None, Limits::default(), vec![], |s| {
                session::codec_client(s, format, vec![enc], Limits::default())
            }) {
                Ok(run) => run,
                Err(e) => return io(e),
            };
            let seen = match run.seen {
                Ok(seen) => seen,
                Err(e) => fail!("{enc_name}/{name}: {e}; server: {:?}", run.served.error),
            };
            if let Err(e) = check(&scene, &seen, format) {
                fail!("{enc_name}/{name}: {e}");
            }
            done.push(format!("{enc_name}/{name} {}B", seen.bytes));
        }
    }
    Outcome::Pass(format!("exact: {}", done.join(", ")))
}

fn input_events() -> Outcome {
    let scene = Scene::new(320, 200, 4);
    let limits = Limits::default();
    let inputs = [
        ClientMessage::Key {
            down: true,
            keysym: 0xff0d,
        },
        ClientMessage::Key {
            down: false,
            keysym: 0xff0d,
        },
        ClientMessage::Pointer {
            buttons: 1,
            x: 10,
            y: 20,
        },
        ClientMessage::Pointer {
            buttons: 0,
            x: 60000,
            y: 60000,
        },
        ClientMessage::CutText(b"to the server".to_vec()),
    ];
    let extras = vec![
        ServerEvent::Bell.encode_control(limits).unwrap(),
        ServerEvent::CutText(b"to the client".to_vec())
            .encode_control(limits)
            .unwrap(),
    ];
    let run = match session::run(&scene, Security::None, limits, extras, |s| {
        session::client(s, None, limits, &inputs, None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    let seen = match run.seen {
        Ok(seen) => seen,
        Err(e) => fail!("client: {e}"),
    };
    let want = vec![
        S::Key {
            down: true,
            keysym: 0xff0d,
        },
        S::Key {
            down: false,
            keysym: 0xff0d,
        },
        S::Pointer {
            buttons: 1,
            x: 10,
            y: 20,
        },
        // Clamped to the screen.
        S::Pointer {
            buttons: 0,
            x: 319,
            y: 199,
        },
        S::CutText(b"to the server".to_vec()),
    ];
    if run.served.input != want {
        fail!("server saw {:?}", run.served.input);
    }
    if seen.bells != 1 || seen.cut != [b"to the client".to_vec()] {
        fail!("client saw {} bells, cut text {:?}", seen.bells, seen.cut);
    }
    if let Err(e) = check(&scene, &seen, PixelFormat::RGBX) {
        fail!("{e}");
    }
    Outcome::Pass("keys, pointer (clamped), cut text both ways and bell delivered".into())
}

fn resize() -> Outcome {
    let mut scene = Scene::new(320, 240, 20);
    scene.resize = Some((8, 500, 300));
    let run = match session::run(&scene, Security::None, Limits::default(), vec![], |s| {
        session::client(s, None, Limits::default(), &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    let seen = match run.seen {
        Ok(seen) => seen,
        Err(e) => fail!("client: {e}; server: {:?}", run.served.error),
    };
    if seen.resized != 1 {
        fail!("{} resizes seen, expected 1", seen.resized);
    }
    if let Err(e) = check(&scene, &seen, PixelFormat::RGBX) {
        fail!("{e}");
    }
    Outcome::Pass("320x240 → 500x300 by DesktopSize mid-stream, framebuffer exact".into())
}

fn fragmentation() -> Outcome {
    let mut scene = Scene::new(160, 120, 10);
    scene.chunk = Some(1);
    let run = match session::run(&scene, Security::None, Limits::default(), vec![], |s| {
        session::client(s, None, Limits::default(), &[], Some(1))
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    let seen = match run.seen {
        Ok(seen) => seen,
        Err(e) => fail!("client: {e}; server: {:?}", run.served.error),
    };
    if let Err(e) = check(&scene, &seen, PixelFormat::RGBX) {
        fail!("{e}");
    }
    Outcome::Pass(format!(
        "every message written one byte at a time both ways; {} bytes, exact",
        seen.bytes
    ))
}

/// A valid server handshake for a 64×48 screen, as `Client` expects it.
fn handshake_prefix() -> Vec<u8> {
    let mut b = VERSION.to_vec();
    b.extend([1, 1]);
    b.extend([0, 0, 0, 0]);
    b.extend(
        ServerInit {
            width: 64,
            height: 48,
            format: PixelFormat::RGBX,
            name: b"x".to_vec(),
        }
        .encode(Limits::default())
        .unwrap(),
    );
    b
}

/// Bytes a hostile server could send: each must end the client with an
/// error (never a panic), and the client must stay failed.
fn hostile_client_bytes() -> Outcome {
    let rect = |x: u16, y: u16, w: u16, h: u16, enc: i32| {
        let mut b = vec![];
        for v in [x, y, w, h] {
            b.extend(v.to_be_bytes());
        }
        b.extend(enc.to_be_bytes());
        b
    };
    let update = |count: u16, rects: Vec<u8>| {
        let mut b = handshake_prefix();
        b.extend([0, 0]);
        b.extend(count.to_be_bytes());
        b.extend(rects);
        b
    };
    let mut huge_init = VERSION.to_vec();
    huge_init.extend([1, 1, 0, 0, 0, 0]);
    huge_init.extend(65535u16.to_be_bytes());
    huge_init.extend(65535u16.to_be_bytes());
    huge_init.extend(PixelFormat::RGBX.encode().unwrap());
    huge_init.extend(0u32.to_be_bytes());
    let mut huge_cut = handshake_prefix();
    huge_cut.extend([3, 0, 0, 0]);
    huge_cut.extend(u32::MAX.to_be_bytes());
    let mut zrle_len = rect(0, 0, 8, 8, ZRLE);
    zrle_len.extend(u32::MAX.to_be_bytes());
    let mut bad_zlib = rect(0, 0, 8, 8, ZRLE);
    bad_zlib.extend(8u32.to_be_bytes());
    bad_zlib.extend([0xde, 0xad, 0xbe, 0xef, 0, 1, 2, 3]);
    let cases: Vec<(&str, Vec<u8>, Option<&str>)> = vec![
        ("version 3.3", b"RFB 003.003\n".to_vec(), Some("Invalid")),
        ("65535x65535 ServerInit", huge_init, Some("Limit")),
        ("4 GiB server cut text", huge_cut, Some("Limit")),
        ("rectangle off the screen", update(1, [rect(60, 40, 8, 8, RAW), vec![0; 256]].concat()), None),
        ("65535 rectangles", update(65535, [rect(0, 0, 1, 1, RAW), vec![0; 4]].concat().repeat(4097)), Some("Limit")),
        ("unknown encoding", update(1, rect(0, 0, 1, 1, 0x7777)), None),
        ("4 GiB ZRLE length", update(1, zrle_len), Some("Limit")),
        ("corrupt zlib", update(1, bad_zlib), None),
        ("copyrect from off the screen", {
            let mut r = rect(0, 0, 8, 8, COPY_RECT);
            r.extend(1000u16.to_be_bytes());
            r.extend(1000u16.to_be_bytes());
            update(1, r)
        }, None),
        ("unknown message type", {
            let mut b = handshake_prefix();
            b.push(0x7f);
            b
        }, None),
    ];
    let mut kinds = vec![];
    for (name, bytes, want) in cases {
        let mut c = Client::new(None, Limits::default());
        let mut failed = None;
        // One byte at a time: the error must come from content, not framing.
        for b in bytes.chunks(1) {
            if let Err(e) = c.receive(b) {
                failed = Some(e);
                break;
            }
        }
        let Some(e) = failed else {
            fail!("{name}: accepted");
        };
        if let Some(want) = want
            && !e.to_string().starts_with(want)
        {
            fail!("{name}: {e}, expected {want}");
        }
        if c.receive(&[0]).is_ok() {
            fail!("{name}: client usable after {e}");
        }
        kinds.push(format!("{name}: {e}"));
    }
    Outcome::Pass(kinds.join("; "))
}

/// Bytes a hostile client could send to the server, over TCP.
fn hostile_server_bytes() -> Outcome {
    let after_init = |extra: &[u8]| {
        let mut b = VERSION.to_vec();
        b.push(1);
        b.push(1);
        b.extend_from_slice(extra);
        b
    };
    let mut set_encodings = vec![2, 0];
    set_encodings.extend(65535u16.to_be_bytes());
    let mut huge_cut = vec![6, 0, 0, 0];
    huge_cut.extend(u32::MAX.to_be_bytes());
    let mut off_screen = vec![3, 0];
    for v in [0u16, 0, 5000, 5000] {
        off_screen.extend(v.to_be_bytes());
    }
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("version 3.3", b"RFB 003.003\n".to_vec(), "requires RFB 3.8"),
        ("security type 9", {
            let mut b = VERSION.to_vec();
            b.push(9);
            b
        }, "Authentication"),
        ("shared flag 7", {
            let mut b = VERSION.to_vec();
            b.extend([1, 7]);
            b
        }, "shared flag"),
        ("65535 encodings", after_init(&set_encodings), "Limit"),
        ("4 GiB client cut text", after_init(&huge_cut), "Limit"),
        ("update request off the screen", after_init(&off_screen), "bounds"),
        ("unknown message type", after_init(&[0x7f]), "Unsupported"),
        ("key down = 2", after_init(&[4, 2, 0, 0, 0, 0, 0, 1]), "boolean"),
    ];
    let mut seen = vec![];
    for (name, bytes, want) in cases {
        let scene = Scene::new(64, 48, 0);
        let (addr, handle) =
            match session::listen(scene, Security::None, Limits::default(), vec![]) {
                Ok(v) => v,
                Err(e) => return io(e),
            };
        let mut s = match TcpStream::connect(addr) {
            Ok(s) => s,
            Err(e) => return io(e),
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = s.write_all(&bytes);
        // The server must close on its own; read until it does.
        let mut sink = [0u8; 4096];
        let start = Instant::now();
        loop {
            match s.read(&mut sink) {
                Ok(0) | Err(_) => break,
                Ok(_) if start.elapsed() > Duration::from_secs(10) => break,
                Ok(_) => {}
            }
        }
        let closed = start.elapsed() < Duration::from_secs(10);
        drop(s);
        let served = match handle.join() {
            Ok(served) => served,
            Err(_) => fail!("{name}: server thread panicked"),
        };
        let Some(error) = served.error else {
            fail!("{name}: the server accepted it");
        };
        if !error.contains(want) {
            fail!("{name}: {error}, expected {want}");
        }
        if !closed {
            fail!("{name}: the server kept the connection open after {error}");
        }
        seen.push(format!("{name}: {error}"));
    }
    Outcome::Pass(seen.join("; "))
}

/// Deterministic xorshift, so a failure names a reproducible seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut b = base.to_vec();
    for _ in 0..1 + rng.below(8) {
        match rng.below(4) {
            0 => {
                let i = rng.below(b.len());
                b[i] = rng.next() as u8;
            }
            1 => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            2 => b.truncate(rng.below(b.len()) + 1),
            _ => {
                let i = rng.below(b.len());
                let n = rng.below(16);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                b.splice(i..i, junk);
            }
        }
    }
    b
}

/// The recorded streams, mutated: decoding may fail, it must never panic
/// or run away. Time-bounded; every case is reproducible from its seed.
fn mutated_fixtures(budget: Duration) -> Outcome {
    let start = Instant::now();
    let (mut cases, mut errors) = (0u64, 0u64);
    let mut rng = Rng(0x5eed_5eed_5eed_5eed);
    while start.elapsed() < budget && cases < 200_000 {
        let seed = rng.next();
        let (base, w, h) = if seed & 1 == 0 {
            (QEMU, 720, 400)
        } else {
            (TIGERVNC, 73, 69)
        };
        let mut local = Rng(seed | 1);
        let bytes = mutate(&mut local, base);
        let result = std::panic::catch_unwind(|| {
            let limits = Limits::default();
            let mut d = ServerDecoder::new(PixelFormat::RGBX, limits).unwrap();
            let mut fb = Framebuffer::new(w, h, limits).unwrap();
            let mut pos = 0;
            loop {
                match d.next(&bytes[pos..]) {
                    Ok((ServerEvent::Rectangle(r), n)) => {
                        pos += n;
                        if fb.apply(r).is_err() {
                            return true;
                        }
                    }
                    Ok((_, n)) => pos += n,
                    Err(Error::Incomplete) => return false,
                    Err(_) => return true,
                }
            }
        });
        match result {
            Ok(err) => errors += u64::from(err),
            Err(_) => fail!("panic on mutated fixture, seed {seed:#x}"),
        }
        cases += 1;
    }
    Outcome::Pass(format!(
        "{cases} mutated streams in {:.0} s: {errors} rejected, the rest decoded or incomplete; no panic",
        start.elapsed().as_secs_f64()
    ))
}

fn small_limits() -> Outcome {
    let scene = Scene::new(640, 480, 2);
    let small = Limits {
        max_pixels: 100_000,
        ..Limits::default()
    };
    let run = match session::run(&scene, Security::None, Limits::default(), vec![], |s| {
        session::client(s, None, small, &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    match run.seen {
        Err(e) if e.starts_with("Limit") => {
            Outcome::Pass("a 640x480 server refused by a 100,000-pixel client: Limit".into())
        }
        Err(e) => Outcome::Fail(format!("expected Limit, got {e}")),
        Ok(_) => Outcome::Fail("a 307,200-pixel framebuffer passed a 100,000-pixel limit".into()),
    }
}

/// Sessions at once, sized from this pod's CPUs and memory.
pub fn sessions_for(cap: &env::Capacity, per_cpu: usize, bytes_each: u64) -> usize {
    let by_cpu = cap.cpus * per_cpu;
    let by_memory = (cap.memory / 4 / bytes_each.max(1)) as usize;
    by_cpu.min(by_memory).clamp(1, 256)
}

/// 1280×720: two framebuffers, the encoder's and the socket buffers.
pub const SESSION_BYTES: u64 = 24 << 20;

/// Run `n` sessions at once, each checked; returns per-session ms/frame.
pub fn parallel(n: usize, frames: u32, seed: u32) -> Result<Vec<f64>, String> {
    let handles: Vec<_> = (0..n)
        .map(|i| {
            std::thread::spawn(move || -> Result<f64, String> {
                let mut scene = Scene::new(1280, 720, frames);
                scene.seed = seed.wrapping_add(i as u32);
                let run = session::run(&scene, Security::None, Limits::default(), vec![], |s| {
                    session::client(s, None, Limits::default(), &[], None)
                })
                .map_err(|e| format!("session {i}: loopback: {e}"))?;
                let seen = run
                    .seen
                    .map_err(|e| format!("session {i}: {e}; server: {:?}", run.served.error))?;
                check(&scene, &seen, PixelFormat::RGBX).map_err(|e| format!("session {i}: {e}"))?;
                Ok(per_frame(run.elapsed, run.served.frames))
            })
        })
        .collect();
    let mut out = vec![];
    for h in handles {
        out.push(h.join().map_err(|_| "session thread panicked".to_string())??);
    }
    Ok(out)
}

fn concurrent_sessions() -> Outcome {
    let cap = env::capacity();
    let n = sessions_for(&cap, 2, SESSION_BYTES);
    match parallel(n, 20, 100) {
        Ok(ms) => Outcome::Pass(format!(
            "{n} sessions at once (from {} CPUs, {} MiB), 1280x720, 20 frames each, all exact; median {:.3} ms/frame",
            cap.cpus,
            cap.memory >> 20,
            median(ms)
        )),
        Err(e) => Outcome::Fail(e),
    }
}

pub fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// DESIGN phase 2's measurement, in the pod: a 640×480 window moving on a
/// 1080p screen, ZRLE, fps and bytes/s. Machine-dependent, so reported,
/// not gated; the gate is that every frame arrives exact.
fn moving_window_1080p() -> Outcome {
    let mut scene = Scene::new(1920, 1080, 300);
    scene.window = (640, 480);
    let run = match session::run(&scene, Security::None, Limits::default(), vec![], |s| {
        session::client(s, None, Limits::default(), &[], None)
    }) {
        Ok(run) => run,
        Err(e) => return io(e),
    };
    let seen = match run.seen {
        Ok(seen) => seen,
        Err(e) => fail!("client: {e}; server: {:?}", run.served.error),
    };
    if let Err(e) = check(&scene, &seen, PixelFormat::RGBX) {
        fail!("{e}");
    }
    let secs = run.elapsed.as_secs_f64();
    Outcome::Pass(format!(
        "1920x1080, 640x480 window, {} frames exact in {secs:.2} s: {:.0} fps, {:.1} MB/s, {} bytes/frame (ZRLE, one server + one client on {} CPUs)",
        run.served.frames,
        f64::from(run.served.frames) / secs,
        seen.bytes as f64 / secs / 1e6,
        seen.bytes / u64::from(run.served.frames.max(1)),
        env::capacity().cpus
    ))
}

/// Read one full frame from a real RFB 3.8 server, shared, sending no
/// input. Only when STORMRFB_TARGET names one.
fn real_server(env: &Env) -> Outcome {
    let Some(target) = &env.target else {
        return Outcome::Skip(
            "STORMRFB_TARGET not set: no real server named (the real-guest path through stormconsole is stormrfb#4)"
                .into(),
        );
    };
    let mut s = match TcpStream::connect(target) {
        Ok(s) => s,
        Err(e) => return Outcome::Infra(format!("{target}: {e}")),
    };
    let _ = s.set_nodelay(true);
    let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
    let mut c = Client::new(env.password.clone(), Limits::default());
    let mut buf = vec![0; 64 * 1024];
    let (mut bytes, mut rects, mut name) = (0u64, 0u64, vec![]);
    let start = Instant::now();
    loop {
        let n = match s.read(&mut buf) {
            Ok(0) => fail!("{target} closed after {bytes} bytes"),
            Ok(n) => n,
            Err(e) => fail!("{target}: {e} after {bytes} bytes"),
        };
        bytes += n as u64;
        let events = match c.receive(&buf[..n]) {
            Ok(e) => e,
            Err(e) => fail!("{target}: {e}"),
        };
        let mut finished = false;
        for e in events {
            match e {
                C::Send(b) => {
                    // After the first update's rectangles, the client's next
                    // send is its incremental request: the frame is complete.
                    if rects > 0 {
                        finished = true;
                    } else if let Err(e) = s.write_all(&b) {
                        fail!("{target}: {e}");
                    }
                }
                C::Ready { name: n } => name = n,
                C::Damage(_) => rects += 1,
                _ => {}
            }
        }
        if finished {
            break;
        }
    }
    let fb = c.framebuffer().expect("ready before damage");
    Outcome::Pass(format!(
        "{target} \"{}\": {}x{} first frame, {rects} rectangles, {bytes} bytes in {:.0} ms",
        String::from_utf8_lossy(&name),
        fb.width(),
        fb.height(),
        start.elapsed().as_secs_f64() * 1000.0
    ))
}
