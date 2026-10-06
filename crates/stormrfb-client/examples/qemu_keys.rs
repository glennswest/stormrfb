//! Type at a real qemu VNC server by scancode (#1): `qemu_keys <vnc.sock>
//! <scancode|keysym> <text>...`. Each text argument is typed, then Enter;
//! arguments are a few seconds apart. In `scancode` mode every key is a QEMU
//! Extended Key Event with keysym 0, so qemu can only use the keycode, and
//! Enter is keypad Enter (E0 1C) to exercise the 0xE0 prefix. In `keysym`
//! mode the same text goes as plain KeyEvents. US layout. Used by
//! tools/verify-extkey.sh; not part of the library.
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use stormrfb::{Limits, qemu_keycode};
use stormrfb_client::{Client, Event};

const SHIFT: u8 = 0x2a;

/// US layout: the XT make code for a character, and whether it needs Shift.
fn xt(c: char) -> Option<(u8, bool)> {
    const ROW: &[(&str, u8)] = &[
        ("1234567890", 0x02),
        ("qwertyuiop", 0x10),
        ("asdfghjkl", 0x1e),
        ("zxcvbnm", 0x2c),
    ];
    const SHIFTED: &[(char, char)] = &[
        ('!', '1'),
        ('@', '2'),
        ('#', '3'),
        ('$', '4'),
        ('%', '5'),
        ('^', '6'),
        ('&', '7'),
        ('*', '8'),
        ('(', '9'),
        (')', '0'),
        ('_', '-'),
        ('+', '='),
        ('>', '.'),
        ('<', ','),
        ('?', '/'),
        (':', ';'),
        ('"', '\''),
    ];
    if let Some(&(_, base)) = SHIFTED.iter().find(|(s, _)| *s == c) {
        return xt(base).map(|(code, _)| (code, true));
    }
    if c.is_ascii_uppercase() {
        return xt(c.to_ascii_lowercase()).map(|(code, _)| (code, true));
    }
    for (row, first) in ROW {
        if let Some(i) = row.find(c) {
            return Some((first + i as u8, false));
        }
    }
    Some((
        match c {
            '-' => 0x0c,
            '=' => 0x0d,
            ';' => 0x27,
            '\'' => 0x28,
            ',' => 0x33,
            '.' => 0x34,
            '/' => 0x35,
            ' ' => 0x39,
            _ => return None,
        },
        false,
    ))
}

struct Session {
    stream: UnixStream,
    client: Client,
}
impl Session {
    /// Read and answer whatever the server sent, for about `ms`.
    fn pump(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut buf = [0; 65536];
        while Instant::now() < end {
            match self.stream.read(&mut buf) {
                Ok(0) => panic!("server closed the connection"),
                Ok(n) => {
                    for e in self.client.receive(&buf[..n]).expect("RFB") {
                        if let Event::Send(b) = e {
                            self.stream.write_all(&b).unwrap();
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => panic!("{e}"),
            }
        }
    }
    fn key(&mut self, down: bool, keysym: u32, keycode: Option<u32>) {
        let b = self.client.key_event(down, keysym, keycode).expect("key");
        self.stream.write_all(&b).unwrap();
        self.pump(15);
    }
    fn tap(&mut self, keysym: u32, keycode: Option<u32>, shift: bool) {
        let shift_code = keycode.map(|_| qemu_keycode(SHIFT, false).unwrap());
        if shift {
            self.key(true, if keycode.is_some() { 0 } else { 0xffe1 }, shift_code);
        }
        self.key(true, keysym, keycode);
        self.key(false, keysym, keycode);
        if shift {
            self.key(false, if keycode.is_some() { 0 } else { 0xffe1 }, shift_code);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, path, mode, lines @ ..] = args.as_slice() else {
        eprintln!("usage: qemu_keys <vnc.sock> <scancode|keysym> <text>...");
        std::process::exit(2);
    };
    let scancodes = match mode.as_str() {
        "scancode" => true,
        "keysym" => false,
        _ => panic!("mode is scancode or keysym"),
    };
    let stream = UnixStream::connect(path).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_millis(10)))
        .unwrap();
    let mut s = Session {
        stream,
        client: Client::new(None, Limits::default()),
    };
    let start = Instant::now();
    while !(s.client.framebuffer().is_some() && s.client.extended_keys())
        && start.elapsed() < Duration::from_secs(10)
    {
        s.pump(50);
    }
    println!("extended_keys {}", s.client.extended_keys());
    if scancodes && !s.client.extended_keys() {
        std::process::exit(1);
    }
    for line in lines {
        for c in line.chars() {
            let (code, shift) = xt(c).unwrap_or_else(|| panic!("no US key for {c:?}"));
            if scancodes {
                s.tap(0, qemu_keycode(code, false), shift);
            } else {
                // Latin-1 keysyms are the characters; qemu adds Shift itself.
                s.tap(u32::from(c), None, false);
            }
        }
        if scancodes {
            s.tap(0, qemu_keycode(0x1c, true), false);
        } else {
            s.tap(0xff0d, None, false);
        }
        println!("typed {line:?}");
        s.pump(3000);
    }
}
