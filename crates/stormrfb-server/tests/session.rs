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
