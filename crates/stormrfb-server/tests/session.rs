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
