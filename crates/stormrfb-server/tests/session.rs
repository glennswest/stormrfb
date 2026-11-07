use stormrfb::*;
use stormrfb_client::{Client, Event as C};
use stormrfb_server::{Event as S, Security, Server};
fn exchange(c: &mut Client, s: &mut Server, initial: Vec<u8>) {
    let mut queue = std::collections::VecDeque::from([initial]);
    for _ in 0..100 {
        let Some(bytes) = queue.pop_front() else {
            return;
        };
        for byte in bytes {
            for e in c.receive(&[byte]).unwrap() {
                if let C::Send(b) = e {
                    for byte in b {
                        for e in s.receive(&[byte]).unwrap() {
                            if let S::Send(b) = e {
                                queue.push_back(b);
                            }
                        }
                    }
                }
            }
        }
    }
    panic!("handshake did not converge");
}
fn server(security: Security) -> Server {
    Server::new(
        ServerInit {
            width: 4,
            height: 3,
            format: PixelFormat::RGBX,
            name: b"test".to_vec(),
        },
        security,
        Limits::default(),
    )
    .unwrap()
}
#[test]
fn both_security_modes_and_incremental_damage() {
    for auth in [false, true] {
        let mut s = server(if auth {
            Security::Vnc {
                password: b"test".to_vec(),
                challenge: [42; 16],
            }
        } else {
            Security::None
        });
        let mut c = Client::new(
            if auth { Some(b"test".to_vec()) } else { None },
            Limits::default(),
        );
        exchange(&mut c, &mut s, VERSION.to_vec());
        assert_eq!(c.framebuffer().unwrap().width(), 4);
        let b = s.update().unwrap().unwrap();
        exchange(&mut c, &mut s, b);
        assert!(s.update().unwrap().is_none());
        s.damage(
            Rect {
                x: 2,
                y: 1,
                width: 1,
                height: 1,
            },
            &[[9, 8, 7, 255]],
        )
        .unwrap();
        let b = s.update().unwrap().unwrap();
        exchange(&mut c, &mut s, b);
        assert_eq!(&c.framebuffer().unwrap().rgba()[24..28], &[9, 8, 7, 255]);
        assert!(s.update().unwrap().is_none());
        let b = c
            .send(ClientMessage::Key {
                down: true,
                keysym: 0xff0d,
            })
            .unwrap();
        assert_eq!(
            s.receive(&b).unwrap(),
            vec![S::Key {
                down: true,
                keysym: 0xff0d
            }]
        );
    }
}
#[test]
fn auth_failure_has_wire_result_and_is_terminal() {
    let mut s = server(Security::Vnc {
        password: b"test".to_vec(),
        challenge: [42; 16],
    });
    s.receive(VERSION).unwrap();
    s.receive(&[2]).unwrap();
    let events = s.receive(&[0; 16]).unwrap();
    let S::Send(b) = &events[0] else { panic!() };
    assert_eq!(&b[..4], &[0, 0, 0, 1]);
    assert_eq!(b.len(), 29);
    assert!(s.receive(&[1]).is_err());
}
#[test]
fn full_request_subregion_and_outside_damage_retained() {
    let mut s = server(Security::None);
    s.receive(VERSION).unwrap();
    s.receive(&[1]).unwrap();
    s.receive(&[1]).unwrap();
    let request = |incremental, rect| {
        ClientMessage::UpdateRequest { incremental, rect }
            .encode(Limits::default())
            .unwrap()
    };
    let region = Rect {
        x: 1,
        y: 1,
        width: 1,
        height: 1,
    };
    s.receive(&request(false, region)).unwrap();
    let b = s.update().unwrap().unwrap();
    let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    d.next(&b).unwrap();
    let (ServerEvent::Rectangle(Rectangle::Pixels { rect, .. }), _) = d.next(&b[4..]).unwrap()
    else {
        panic!()
    };
    assert_eq!(rect, region);
    s.receive(&request(
        true,
        Rect {
            width: 4,
            height: 3,
            ..Rect::default()
        },
    ))
    .unwrap();
    assert!(s.update().unwrap().is_some());
}
#[test]
fn resize_waits_for_request_and_client_requests_new_full_frame() {
    let mut s = server(Security::None);
    let mut c = Client::new(None, Limits::default());
    exchange(&mut c, &mut s, VERSION.to_vec());
    let b = s.update().unwrap().unwrap();
    exchange(&mut c, &mut s, b);
    s.resize(2, 2).unwrap();
    let b = s.update().unwrap().unwrap();
    exchange(&mut c, &mut s, b);
    assert_eq!(c.framebuffer().unwrap().width(), 2);
    s.damage(
        Rect {
            width: 2,
            height: 2,
            ..Rect::default()
        },
        &[[1, 2, 3, 255]; 4],
    )
    .unwrap();
    let b = s.update().unwrap().unwrap();
    exchange(&mut c, &mut s, b);
    assert_eq!(&c.framebuffer().unwrap().rgba()[..4], &[1, 2, 3, 255]);
}
#[test]
fn resize_accepts_request_using_old_dimensions() {
    let mut s = server(Security::None);
    s.receive(VERSION).unwrap();
    s.receive(&[1]).unwrap();
    s.receive(&[1]).unwrap();
    s.receive(
        &ClientMessage::SetEncodings(vec![RAW, DESKTOP_SIZE])
            .encode(Limits::default())
            .unwrap(),
    )
    .unwrap();
    s.resize(1, 1).unwrap();
    s.receive(
        &ClientMessage::UpdateRequest {
            incremental: false,
            rect: Rect {
                width: 4,
                height: 3,
                ..Rect::default()
            },
        }
        .encode(Limits::default())
        .unwrap(),
    )
    .unwrap();
    assert!(s.update().unwrap().is_some());
}
#[test]
fn qemu_extended_keys_are_acknowledged_then_sent() {
    let mut s = server(Security::None);
    let mut c = Client::new(None, Limits::default());
    exchange(&mut c, &mut s, VERSION.to_vec());
    // Before the acknowledgement: refused, and key_event falls back to Key.
    assert!(!c.extended_keys());
    let qemu = ClientMessage::QemuKey {
        down: true,
        keysym: 0,
        keycode: 0x1e,
    };
    assert_eq!(
        c.send(qemu.clone()),
        Err(Error::Unsupported(QEMU_EXTENDED_KEY))
    );
    let b = c.key_event(true, 0x61, Some(0x1e)).unwrap();
    assert_eq!(
        s.receive(&b).unwrap(),
        vec![S::Key {
            down: true,
            keysym: 0x61
        }]
    );
    assert_eq!(
        c.key_event(true, 0, Some(0x1e)),
        Err(Error::Unsupported(QEMU_EXTENDED_KEY))
    );
    // The first update carries the acknowledgement; no damage event for it.
    let b = s.update().unwrap().unwrap();
    exchange(&mut c, &mut s, b);
    assert!(c.extended_keys());
    assert!(s.update().unwrap().is_none());
    // Now a scancode alone goes through, and the server reports it.
    let b = c.key_event(false, 0, qemu_keycode(0x1d, true)).unwrap();
    assert_eq!(b.len(), 12);
    assert_eq!(
        s.receive(&b).unwrap(),
        vec![S::QemuKey {
            down: false,
            keysym: 0,
            keycode: 0x9d
        }]
    );
    // Without a keycode it is still a plain Key.
    let b = c.key_event(true, 0xff0d, None).unwrap();
    assert_eq!(
        s.receive(&b).unwrap(),
        vec![S::Key {
            down: true,
            keysym: 0xff0d
        }]
    );
}
#[test]
fn acknowledgement_answers_an_incremental_request_without_damage() {
    let mut s = server(Security::None);
    let mut c = Client::new(None, Limits::default());
    exchange(&mut c, &mut s, VERSION.to_vec());
    let b = s.update().unwrap().unwrap();
    exchange(&mut c, &mut s, b);
    // The client re-advertises (as after a reconnect of its encoder); an
    // already acknowledged session is not acknowledged twice.
    s.receive(
        &ClientMessage::SetEncodings(ENCODINGS.to_vec())
            .encode(Limits::default())
            .unwrap(),
    )
    .unwrap();
    assert!(s.update().unwrap().is_none());
    // A session that first asked without -258 and has drawn everything,
    // then advertises it with an incremental request and no damage: the
    // acknowledgement is answered on its own.
    let mut s = server(Security::None);
    let enc = |v: Vec<i32>| {
        ClientMessage::SetEncodings(v)
            .encode(Limits::default())
            .unwrap()
    };
    let req = |incremental| {
        ClientMessage::UpdateRequest {
            incremental,
            rect: Rect {
                width: 4,
                height: 3,
                ..Rect::default()
            },
        }
        .encode(Limits::default())
        .unwrap()
    };
    for b in [
        VERSION.to_vec(),
        vec![1],
        vec![1],
        enc(vec![RAW]),
        req(false),
    ] {
        s.receive(&b).unwrap();
    }
    assert!(s.update().unwrap().is_some());
    s.receive(&req(true)).unwrap();
    assert!(s.update().unwrap().is_none());
    s.receive(&enc(vec![RAW, QEMU_EXTENDED_KEY])).unwrap();
    let b = s.update().unwrap().expect("the acknowledgement");
    let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    let mut events = vec![];
    let mut pos = 0;
    while let Ok((e, n)) = d.next(&b[pos..]) {
        pos += n;
        events.push(e);
    }
    assert_eq!(
        events,
        vec![
            ServerEvent::UpdateStart,
            ServerEvent::Rectangle(Rectangle::QemuExtendedKey),
            ServerEvent::UpdateEnd
        ]
    );
    assert!(s.update().unwrap().is_none());
}
/// A ready session whose client has had its first update, so it knows
/// whether the server takes -308.
fn ready() -> (Client, Server) {
    let mut s = server(Security::None);
    let mut c = Client::new(None, Limits::default());
    exchange(&mut c, &mut s, VERSION.to_vec());
    assert!(!c.desktop_resize());
    assert_eq!(
        c.request_resize(8, 6),
        Err(Error::Unsupported(EXTENDED_DESKTOP_SIZE))
    );
    s.damage(
        Rect {
            width: 4,
            height: 3,
            ..Rect::default()
        },
        &[[9, 8, 7, 255]; 12],
    )
    .unwrap();
    let b = s.update().unwrap().unwrap();
    // The layout comes with the pixels and does not clear or resize them.
    let events = c.receive(&b).unwrap();
    assert!(!events.iter().any(|e| matches!(e, C::Resized { .. })));
    assert_eq!(&c.framebuffer().unwrap().rgba()[..4], &[9, 8, 7, 255]);
    assert!(c.desktop_resize());
    assert_eq!(c.screens(), &[Screen::whole(0, 4, 3)]);
    for e in events {
        if let C::Send(b) = e {
            s.receive(&b).unwrap();
        }
    }
    (c, s)
}
#[test]
fn extended_desktop_size_request_accepted() {
    let (mut c, mut s) = ready();
    assert_eq!(c.resize_status(), None);
    let b = c.request_resize(8, 6).unwrap();
    assert_eq!(
        s.receive(&b).unwrap(),
        vec![S::SetDesktopSize {
            width: 8,
            height: 6,
            screens: vec![Screen::whole(0, 8, 6)],
        }]
    );
    s.accept_resize().unwrap();
    assert!(s.accept_resize().is_err(), "answered already");
    let b = s.update().unwrap().unwrap();
    let events = c.receive(&b).unwrap();
    assert!(events.contains(&C::Resized {
        width: 8,
        height: 6
    }));
    assert_eq!(c.resize_status(), Some(RESIZE_OK));
    assert_eq!(c.framebuffer().unwrap().width(), 8);
    // The client then asks for the whole new framebuffer, and gets it.
    for e in events {
        if let C::Send(b) = e {
            s.receive(&b).unwrap();
        }
    }
    s.damage(
        Rect {
            x: 7,
            y: 5,
            width: 1,
            height: 1,
        },
        &[[1, 2, 3, 255]],
    )
    .unwrap();
    let b = s.update().unwrap().unwrap();
    exchange(&mut c, &mut s, b);
    assert_eq!(
        &c.framebuffer().unwrap().rgba()[(5 * 8 + 7) * 4..],
        &[1, 2, 3, 255]
    );
}
#[test]
fn extended_desktop_size_request_refused() {
    let (mut c, mut s) = ready();
    assert!(s.refuse_resize(RESIZE_PROHIBITED).is_err(), "nothing asked");
    s.receive(&c.request_resize(8, 6).unwrap()).unwrap();
    assert!(s.refuse_resize(RESIZE_OK).is_err());
    s.refuse_resize(RESIZE_PROHIBITED).unwrap();
    let b = s.update().unwrap().expect("the refusal answers on its own");
    let events = c.receive(&b).unwrap();
    assert!(!events.iter().any(|e| matches!(e, C::Resized { .. })));
    assert_eq!(c.resize_status(), Some(RESIZE_PROHIBITED));
    assert_eq!(c.framebuffer().unwrap().width(), 4);
    assert_eq!(&c.framebuffer().unwrap().rgba()[..4], &[9, 8, 7, 255]);
}
#[test]
fn extended_desktop_size_bad_requests_are_refused_without_an_event() {
    let small = Limits {
        max_pixels: 100,
        ..Limits::default()
    };
    for (width, height, screens, status) in [
        // A screen outside the framebuffer.
        (4, 3, vec![Screen::whole(0, 8, 6)], RESIZE_INVALID_LAYOUT),
        (4, 3, vec![], RESIZE_INVALID_LAYOUT),
        // Larger than the session's limits.
        (
            20,
            20,
            vec![Screen::whole(0, 20, 20)],
            RESIZE_OUT_OF_RESOURCES,
        ),
    ] {
        let mut s = Server::new(
            ServerInit {
                width: 4,
                height: 3,
                format: PixelFormat::RGBX,
                name: b"test".to_vec(),
            },
            Security::None,
            small,
        )
        .unwrap();
        let mut c = Client::new(None, small);
        exchange(&mut c, &mut s, VERSION.to_vec());
        let b = s.update().unwrap().unwrap();
        exchange(&mut c, &mut s, b);
        let m = c
            .send(ClientMessage::SetDesktopSize {
                width,
                height,
                screens,
            })
            .unwrap();
        assert_eq!(s.receive(&m).unwrap(), vec![]);
        assert!(s.accept_resize().is_err());
        let b = s.update().unwrap().unwrap();
        exchange(&mut c, &mut s, b);
        assert_eq!(c.resize_status(), Some(status));
        assert_eq!(c.framebuffer().unwrap().width(), 4);
    }
}
#[test]
fn server_resize_is_sent_as_extended_desktop_size() {
    let (mut c, mut s) = ready();
    s.resize(6, 2).unwrap();
    let b = s.update().unwrap().unwrap();
    // One -308 rectangle, reason 0, alone in the update, and (continuous
    // updates being on) the fence after it: 248, padding, flags, 4 bytes.
    assert_eq!(b.len(), 4 + 12 + 4 + 16 + 13);
    assert_eq!(b[36], 248);
    assert_eq!(&b[..4], &[0, 0, 0, 1]);
    assert_eq!(&b[4..8], &[0, 0, 0, 0], "reason 0, status 0");
    assert_eq!(&b[12..16], &EXTENDED_DESKTOP_SIZE.to_be_bytes());
    let events = c.receive(&b).unwrap();
    assert!(events.contains(&C::Resized {
        width: 6,
        height: 2
    }));
    assert_eq!(c.screens(), &[Screen::whole(0, 6, 2)]);
    assert_eq!(c.resize_status(), None, "not an answer to the client");
}
#[test]
fn set_desktop_size_without_negotiation_ends_the_session() {
    let mut s = server(Security::None);
    for b in [VERSION.to_vec(), vec![1], vec![1]] {
        s.receive(&b).unwrap();
    }
    let m = ClientMessage::SetDesktopSize {
        width: 8,
        height: 6,
        screens: vec![Screen::whole(0, 8, 6)],
    }
    .encode(Limits::default())
    .unwrap();
    assert_eq!(
        s.receive(&m),
        Err(Error::Unsupported(EXTENDED_DESKTOP_SIZE))
    );
}
#[test]
fn paused_client_asks_for_nothing_and_resumes_with_a_full_frame() {
    let (mut c, mut s) = ready();
    // Our server offers ContinuousUpdates and Fence, so the client is on.
    assert!(c.continuous() && s.continuous());
    let before = c.updates();
    // Pausing stops continuous updates; the server says so, and nothing
    // follows: no request, no re-enable.
    let stop = c.pause().unwrap();
    assert!(c.paused() && !c.continuous());
    let events = s.receive(&stop).unwrap();
    assert!(!s.continuous());
    let [S::Send(end)] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert_eq!(c.receive(end).unwrap(), vec![]);
    // Damage while paused is not sent.
    s.damage(
        Rect {
            x: 3,
            y: 2,
            width: 1,
            height: 1,
        },
        &[[4, 5, 6, 255]],
    )
    .unwrap();
    assert!(s.update().unwrap().is_none());
    // Resuming asks for the whole framebuffer and turns them back on.
    let b = c.resume().unwrap();
    assert!(!c.paused() && c.continuous());
    assert_eq!(s.receive(&b).unwrap(), vec![]);
    assert!(s.continuous());
    let b = s.update().unwrap().unwrap();
    let events = c.receive(&b).unwrap();
    assert!(events.contains(&C::Damage(Rect {
        width: 4,
        height: 3,
        ..Rect::default()
    })));
    assert_eq!(&c.framebuffer().unwrap().rgba()[44..48], &[4, 5, 6, 255]);
    assert_eq!(c.updates(), before + 1);
    // The update carried a fence, which the client answers.
    let answers: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            C::Send(b) => Some(b.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(answers.len(), 1);
    assert!(s.in_flight() > 0);
    s.receive(&answers[0]).unwrap();
    assert_eq!(s.in_flight(), 0);
}
#[test]
fn continuous_updates_are_pushed_and_held_by_the_fence_window() {
    let (mut c, mut s) = ready();
    assert!(c.continuous() && s.continuous());
    s.set_window(1);
    let px = |x| {
        (
            Rect {
                x,
                y: 0,
                width: 1,
                height: 1,
            },
            [[x as u8, 1, 2, 255]],
        )
    };
    // No request needed: damage is pushed, with a fence after it.
    let (r, p) = px(0);
    s.damage(r, &p).unwrap();
    let first = s.update().unwrap().expect("pushed");
    assert!(s.in_flight() > 0);
    // Unanswered, a 1-byte window holds the next one back.
    let (r, p) = px(1);
    s.damage(r, &p).unwrap();
    assert!(s.update().unwrap().is_none(), "the window is full");
    // The client draws the first and answers its fence; the next goes.
    let mut answers = vec![];
    for e in c.receive(&first).unwrap() {
        if let C::Send(b) = e {
            answers.push(b);
        }
    }
    assert_eq!(answers.len(), 1, "a fence answer, and no update request");
    s.receive(&answers[0]).unwrap();
    assert_eq!(s.in_flight(), 0);
    let b = s.update().unwrap().expect("the window opened");
    c.receive(&b).unwrap();
    assert_eq!(&c.framebuffer().unwrap().rgba()[4..8], &[1, 1, 2, 255]);
    // Incremental requests are moot; a non-incremental one is answered.
    let whole = Rect {
        width: 4,
        height: 3,
        ..Rect::default()
    };
    let req = |incremental| {
        ClientMessage::UpdateRequest {
            incremental,
            rect: whole,
        }
        .encode(Limits::default())
        .unwrap()
    };
    s.set_window(DEFAULT_WINDOW);
    s.receive(&req(true)).unwrap();
    assert!(s.update().unwrap().is_none());
    s.receive(&req(false)).unwrap();
    assert!(s.update().unwrap().is_some());
    // The client's own fence request is answered as it was asked, without
    // the request bit and flags it does not know.
    let ask = ClientMessage::Fence {
        flags: FENCE_REQUEST | FENCE_SYNC_NEXT | 1 << 20,
        payload: b"ping".to_vec(),
    }
    .encode(Limits::default())
    .unwrap();
    let events = s.receive(&ask).unwrap();
    let [S::Send(b)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    assert_eq!(
        d.next(b).unwrap().0,
        ServerEvent::Fence {
            flags: FENCE_SYNC_NEXT,
            payload: b"ping".to_vec()
        }
    );
}
#[test]
fn continuous_updates_and_fences_need_their_encodings() {
    let mut s = server(Security::None);
    for b in [VERSION.to_vec(), vec![1], vec![1]] {
        s.receive(&b).unwrap();
    }
    // Without -313/-312 advertised, nothing is announced and either
    // message ends the session.
    let events = s
        .receive(
            &ClientMessage::SetEncodings(vec![RAW])
                .encode(Limits::default())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(events, vec![]);
    let enable = ClientMessage::EnableContinuousUpdates {
        enable: true,
        rect: Rect::default(),
    }
    .encode(Limits::default())
    .unwrap();
    assert_eq!(
        s.receive(&enable),
        Err(Error::Unsupported(CONTINUOUS_UPDATES))
    );
    let mut s = server(Security::None);
    for b in [VERSION.to_vec(), vec![1], vec![1]] {
        s.receive(&b).unwrap();
    }
    let fence = ClientMessage::Fence {
        flags: FENCE_REQUEST,
        payload: vec![],
    }
    .encode(Limits::default())
    .unwrap();
    assert_eq!(s.receive(&fence), Err(Error::Unsupported(FENCE)));
    // Advertised: a fence request and EndOfContinuousUpdates, once each.
    let mut s = server(Security::None);
    for b in [VERSION.to_vec(), vec![1], vec![1]] {
        s.receive(&b).unwrap();
    }
    let enc = ClientMessage::SetEncodings(vec![RAW, FENCE, CONTINUOUS_UPDATES])
        .encode(Limits::default())
        .unwrap();
    let events = s.receive(&enc).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1], S::Send(vec![150]));
    assert_eq!(s.receive(&enc).unwrap(), vec![]);
    // Out of bounds, refused.
    let far = ClientMessage::EnableContinuousUpdates {
        enable: true,
        rect: Rect {
            x: 3,
            y: 0,
            width: 2,
            height: 1,
        },
    }
    .encode(Limits::default())
    .unwrap();
    assert!(s.receive(&far).is_err());
}
