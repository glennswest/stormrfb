//! The viewer against stormrfb's own server, in process: a known frame on
//! the screen, a keystroke and a click round-tripped, a VT release and
//! acquire with no stale frame, the switch keys, and a resize to the screen
//! (stormrfb#25's "done when").

use std::collections::VecDeque;

use stormrfb::*;
use stormrfb_server::{Event as S, Security, Server};
use stormrfb_vt::fit::xrgb;
use stormrfb_vt::{Action, Input, Options, SwitchKeys, Viewer};

const GW: u16 = 8;
const GH: u16 = 6;
/// The screen: twice the guest, so every guest pixel is 2x2 on it.
const DW: usize = 16;
const DH: usize = 12;

fn server() -> Server {
    Server::new(
        ServerInit {
            width: GW,
            height: GH,
            format: PixelFormat::RGBX,
            name: b"default/desk".to_vec(),
        },
        Security::None,
        Limits::default(),
    )
    .unwrap()
}

fn viewer(opts: Options) -> Viewer {
    Viewer::new(DW, DH, None, Limits::default(), opts)
}

fn no_resize() -> Options {
    Options {
        resize: false,
        ..Options::default()
    }
}

/// What the server heard (other than handshake bytes) and the viewer's VT
/// switches.
#[derive(Default)]
struct Heard {
    server: Vec<S>,
    switches: Vec<u16>,
}

/// Runs both ends until neither has anything to say.
fn deliver(v: &mut Viewer, s: &mut Server, actions: Vec<Action>, heard: &mut Heard) {
    let mut to_viewer = VecDeque::new();
    let mut to_server: VecDeque<Action> = actions.into();
    for _ in 0..1000 {
        while let Some(a) = to_server.pop_front() {
            match a {
                Action::Send(b) => {
                    for e in s.receive(&b).unwrap() {
                        match e {
                            S::Send(b) => to_viewer.push_back(b),
                            e => heard.server.push(e),
                        }
                    }
                }
                Action::Switch(n) => heard.switches.push(n),
            }
        }
        if let Some(b) = s.update().unwrap() {
            to_viewer.push_back(b);
        }
        let Some(b) = to_viewer.pop_front() else {
            return;
        };
        // In odd-sized pieces, as a socket would deliver them.
        for piece in b.chunks(7) {
            to_server.extend(v.receive(piece).unwrap());
        }
    }
    panic!("the session did not settle");
}

fn pump(v: &mut Viewer, s: &mut Server, actions: Vec<Action>) -> Heard {
    let mut heard = Heard::default();
    deliver(v, s, actions, &mut heard);
    heard
}

fn input(v: &mut Viewer, s: &mut Server, events: &[Input]) -> Heard {
    let mut actions = vec![];
    for &e in events {
        actions.extend(v.input(e));
    }
    pump(v, s, actions)
}

/// A guest picture where every pixel differs from its neighbours.
fn picture(seed: u8) -> Vec<[u8; 4]> {
    (0..GH)
        .flat_map(|y| {
            (0..GW).map(move |x| {
                [
                    (x as u8) * 30 + seed,
                    (y as u8) * 40,
                    seed.wrapping_mul(3),
                    255,
                ]
            })
        })
        .collect()
}

fn whole() -> Rect {
    Rect {
        width: GW,
        height: GH,
        ..Rect::default()
    }
}

/// The screen shows `pic`, each guest pixel as a 2x2 block.
fn assert_shows(v: &Viewer, pic: &[[u8; 4]]) {
    let frame = v.frame().expect("a picture on the screen");
    assert_eq!((frame.width, frame.height), (DW, DH));
    for y in 0..DH {
        for x in 0..DW {
            let want = xrgb(&pic[(y / 2) * usize::from(GW) + x / 2]);
            assert_eq!(frame.pixels[y * DW + x], want, "screen ({x}, {y})");
        }
    }
}

/// A connected session showing `picture(1)`.
fn connected(opts: Options) -> (Viewer, Server) {
    let mut v = viewer(opts);
    let mut s = server();
    s.damage(whole(), &picture(1)).unwrap();
    let first = v.receive(s.greeting()).unwrap();
    let heard = pump(&mut v, &mut s, first);
    assert!(heard.server.iter().any(|e| matches!(e, S::Ready { .. })));
    assert!(v.ready());
    (v, s)
}

#[test]
fn a_known_frame_is_on_the_screen() {
    let (v, _s) = connected(no_resize());
    assert_shows(&v, &picture(1));
    // The server acknowledged -258, so keys go as scancodes.
    assert!(v.client().extended_keys());
}

#[test]
fn nothing_is_shown_before_every_pixel_has_arrived() {
    let mut v = viewer(no_resize());
    let mut s = server();
    let first = v.receive(s.greeting()).unwrap();
    // Handshake only: no update yet.
    let mut heard = Heard::default();
    let mut q: VecDeque<Action> = first.into();
    while let Some(Action::Send(b)) = q.pop_front() {
        for e in s.receive(&b).unwrap() {
            match e {
                S::Send(b) => q.extend(v.receive(&b).unwrap()),
                e => heard.server.push(e),
            }
        }
    }
    assert!(v.ready());
    assert!(v.frame().is_none(), "no frame before the first update");
    pump(&mut v, &mut s, vec![]);
    assert!(v.frame().is_some());
}

#[test]
fn a_keystroke_round_trips_as_a_scancode_with_its_keysym() {
    let (mut v, mut s) = connected(no_resize());
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 30, value: 1 },
            Input::Sync,
            Input::Key { code: 30, value: 0 },
            Input::Sync,
        ],
    );
    assert_eq!(
        heard.server,
        vec![
            S::QemuKey {
                down: true,
                keysym: u32::from(b'a'),
                keycode: 0x1e
            },
            S::QemuKey {
                down: false,
                keysym: u32::from(b'a'),
                keycode: 0x1e
            },
        ]
    );
    // Shifted, and an E0-prefixed key.
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 42, value: 1 },
            Input::Key { code: 30, value: 1 },
            Input::Key { code: 30, value: 0 },
            Input::Key { code: 42, value: 0 },
            Input::Key {
                code: 103,
                value: 1,
            },
            Input::Key {
                code: 103,
                value: 0,
            },
        ],
    );
    let keys: Vec<(bool, u32, u32)> = heard
        .server
        .iter()
        .map(|e| match e {
            S::QemuKey {
                down,
                keysym,
                keycode,
            } => (*down, *keysym, *keycode),
            e => panic!("{e:?}"),
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            (true, 0xffe1, 0x2a),
            (true, u32::from(b'A'), 0x1e),
            (false, u32::from(b'A'), 0x1e),
            (false, 0xffe1, 0x2a),
            (true, 0xff52, 0xc8),
            (false, 0xff52, 0xc8),
        ]
    );
}

#[test]
fn a_click_round_trips_at_the_guest_position() {
    let (mut v, mut s) = connected(no_resize());
    // An absolute pointer three quarters across and half way down the
    // 16x12 screen is guest pixel (6, 3).
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::AbsX(0.75),
            Input::AbsY(0.5),
            Input::Sync,
            Input::Key {
                code: 0x110,
                value: 1,
            },
            Input::Sync,
            Input::Key {
                code: 0x110,
                value: 0,
            },
            Input::Sync,
        ],
    );
    let p = |buttons| S::Pointer {
        buttons,
        x: 6,
        y: 3,
    };
    assert_eq!(heard.server, vec![p(0), p(1), p(0)]);
    // A relative mouse moves from there, clamped to the picture.
    let heard = input(
        &mut v,
        &mut s,
        &[Input::RelX(-2), Input::RelY(100), Input::Sync],
    );
    assert_eq!(
        heard.server,
        vec![S::Pointer {
            buttons: 0,
            x: 5,
            y: 5
        }]
    );
    // A wheel notch is a press and release of button 4.
    let heard = input(&mut v, &mut s, &[Input::Wheel(1)]);
    assert_eq!(
        heard.server,
        vec![
            S::Pointer {
                buttons: 8,
                x: 5,
                y: 5
            },
            S::Pointer {
                buttons: 0,
                x: 5,
                y: 5
            }
        ]
    );
}

#[test]
fn release_and_acquire_show_no_stale_frame() {
    let (mut v, mut s) = connected(no_resize());
    assert_shows(&v, &picture(1));
    // A key and a button held as the VT goes away.
    input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 29, value: 1 },
            Input::Key {
                code: 0x110,
                value: 1,
            },
            Input::Sync,
        ],
    );
    let released = v.release();
    let heard = pump(&mut v, &mut s, released);
    assert!(
        heard.server.contains(&S::QemuKey {
            down: false,
            keysym: 0xffe3,
            keycode: 0x1d
        }),
        "the held key is let go of: {:?}",
        heard.server
    );
    assert!(
        matches!(
            heard.server.last(),
            Some(S::Pointer { buttons: 0, .. })
        ),
        "the held button is let go of: {:?}",
        heard.server
    );
    // Away: input is not taken, and the guest's screen changes. The request
    // outstanding at release is answered once; nothing is asked after it.
    assert!(v.input(Input::Key { code: 30, value: 1 }).is_empty());
    s.damage(whole(), &picture(2)).unwrap();
    pump(&mut v, &mut s, vec![]);
    s.damage(whole(), &picture(3)).unwrap();
    assert!(
        pump(&mut v, &mut s, vec![]).server.is_empty() && s.update().unwrap().is_none(),
        "paused: no request, so no update"
    );
    // Back: nothing is shown until the whole frame is new, then it is the
    // guest's current one.
    let acquired = v.acquire();
    assert!(v.frame().is_none(), "no picture from before the switch");
    assert!(v.take_changed());
    pump(&mut v, &mut s, acquired);
    assert_shows(&v, &picture(3));
    // And it carries on: incremental updates after it.
    s.damage(whole(), &picture(4)).unwrap();
    pump(&mut v, &mut s, vec![]);
    assert_shows(&v, &picture(4));
    // A key released after acquire that went down before it is not sent.
    assert!(v.input(Input::Key { code: 30, value: 0 }).is_empty());
}

#[test]
fn alt_f_keys_switch_vts_and_the_guest_never_sees_them() {
    let (mut v, mut s) = connected(no_resize());
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 56, value: 1 },
            Input::Key { code: 59, value: 1 },
            Input::Key { code: 59, value: 0 },
            Input::Key { code: 56, value: 0 },
        ],
    );
    assert_eq!(heard.switches, vec![1]);
    let codes: Vec<u32> = heard
        .server
        .iter()
        .filter_map(|e| match e {
            S::QemuKey { keycode, .. } => Some(*keycode),
            _ => None,
        })
        .collect();
    assert_eq!(codes, vec![0x38, 0x38], "only Alt, down and up");
    // Ctrl+Alt+F8 too.
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 29, value: 1 },
            Input::Key { code: 56, value: 1 },
            Input::Key { code: 66, value: 1 },
        ],
    );
    assert_eq!(heard.switches, vec![8]);
}

#[test]
fn ctrl_alt_mode_passes_alt_f4_to_the_guest() {
    let (mut v, mut s) = connected(Options {
        switch: SwitchKeys::CtrlAlt,
        resize: false,
    });
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 56, value: 1 },
            Input::Key { code: 62, value: 1 },
            Input::Key { code: 62, value: 0 },
            Input::Key { code: 56, value: 0 },
        ],
    );
    assert!(heard.switches.is_empty());
    assert_eq!(heard.server.len(), 4);
    let heard = input(
        &mut v,
        &mut s,
        &[
            Input::Key { code: 97, value: 1 },
            Input::Key { code: 100, value: 1 },
            Input::Key { code: 63, value: 1 },
        ],
    );
    assert_eq!(heard.switches, vec![5]);
}

#[test]
fn the_guest_is_asked_for_the_screens_size() {
    let mut v = viewer(Options::default());
    let mut s = server();
    s.damage(whole(), &picture(1)).unwrap();
    let first = v.receive(s.greeting()).unwrap();
    let heard = pump(&mut v, &mut s, first);
    let asked = heard
        .server
        .iter()
        .find_map(|e| match e {
            S::SetDesktopSize { width, height, .. } => Some((*width, *height)),
            _ => None,
        })
        .expect("a SetDesktopSize");
    assert_eq!(asked, (DW as u16, DH as u16));
    // Granted: the guest is now the screen's size, shown 1:1.
    s.accept_resize().unwrap();
    let pic: Vec<[u8; 4]> = (0..DW * DH)
        .map(|i| [i as u8, (i / DW) as u8, 9, 255])
        .collect();
    s.damage(
        Rect {
            width: DW as u16,
            height: DH as u16,
            ..Rect::default()
        },
        &pic,
    )
    .unwrap();
    let heard = pump(&mut v, &mut s, vec![]);
    assert!(
        !heard
            .server
            .iter()
            .any(|e| matches!(e, S::SetDesktopSize { .. })),
        "asked once"
    );
    let frame = v.frame().expect("the resized picture");
    for (i, p) in pic.iter().enumerate() {
        assert_eq!(frame.pixels[i], xrgb(p), "pixel {i}");
    }
}
