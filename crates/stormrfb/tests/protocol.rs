use stormrfb::*;
#[test]
fn client_messages_roundtrip_and_fragment() {
    let messages = vec![
        ClientMessage::SetPixelFormat(PixelFormat::RGBX),
        ClientMessage::SetEncodings(ENCODINGS.to_vec()),
        ClientMessage::UpdateRequest {
            incremental: true,
            rect: Rect {
                x: 1,
                y: 2,
                width: 640,
                height: 480,
            },
        },
        ClientMessage::Key {
            down: true,
            keysym: 0xff0d,
        },
        ClientMessage::QemuKey {
            down: false,
            keysym: 0,
            keycode: 0x9c,
        },
        ClientMessage::Pointer {
            buttons: 5,
            x: 42,
            y: 51,
        },
        ClientMessage::CutText(b"clipboard\n".to_vec()),
        ClientMessage::SetDesktopSize {
            width: 1920,
            height: 1080,
            screens: vec![
                Screen::whole(7, 960, 1080),
                Screen {
                    id: 8,
                    rect: Rect {
                        x: 960,
                        y: 0,
                        width: 960,
                        height: 1080,
                    },
                    flags: 3,
                },
            ],
        },
    ];
    for m in messages {
        let b = m.encode(Limits::default()).unwrap();
        for i in 0..b.len() {
            assert_eq!(
                ClientMessage::decode(&b[..i], Limits::default()),
                Err(Error::Incomplete)
            );
        }
        let mut joined = b.clone();
        joined.push(255);
        assert_eq!(
            ClientMessage::decode(&joined, Limits::default()).unwrap(),
            (m, b.len())
        );
    }
}
#[test]
fn pixel_endianness_and_alpha() {
    for be in [false, true] {
        let f = PixelFormat {
            bits_per_pixel: 16,
            depth: 16,
            big_endian: be,
            red_max: 31,
            green_max: 63,
            blue_max: 31,
            red_shift: 11,
            green_shift: 5,
            blue_shift: 0,
        };
        assert_eq!(PixelFormat::decode(&f.encode().unwrap()).unwrap(), f);
        let mut b = vec![];
        f.write([255, 0, 255, 0], &mut b).unwrap();
        assert_eq!(
            b,
            if be {
                vec![0xf8, 0x1f]
            } else {
                vec![0x1f, 0xf8]
            }
        );
        assert_eq!(f.read(&b).unwrap(), [255, 0, 255, 255]);
        let f = PixelFormat {
            big_endian: be,
            ..PixelFormat::RGBX
        };
        let mut b = vec![];
        f.write([12, 23, 34, 0], &mut b).unwrap();
        assert_eq!(f.read(&b).unwrap(), [12, 23, 34, 255]);
    }
    assert_eq!(
        PixelFormat::RGBX.read(&[1, 2, 3, 0]).unwrap(),
        [1, 2, 3, 255]
    );
    assert!(
        PixelFormat {
            green_shift: 0,
            ..PixelFormat::RGBX
        }
        .validate()
        .is_err()
    );
    assert!(
        PixelFormat {
            red_shift: 255,
            ..PixelFormat::RGBX
        }
        .validate()
        .is_err()
    );
}
#[test]
fn none_handshake_fragmentation() {
    let init = ServerInit {
        width: 640,
        height: 480,
        format: PixelFormat::RGBX,
        name: b"test".to_vec(),
    };
    let mut h = ClientHandshake::new(None, true, Limits::default());
    for (b, event) in [
        (VERSION.to_vec(), HandshakeEvent::Send(VERSION.to_vec())),
        (vec![2, 2, 1], HandshakeEvent::Send(vec![1])),
        (vec![0; 4], HandshakeEvent::Send(vec![1])),
        (
            init.encode(Limits::default()).unwrap(),
            HandshakeEvent::Ready(init),
        ),
    ] {
        for i in 0..b.len() {
            assert_eq!(h.step(&b[..i]), Err(Error::Incomplete));
        }
        assert_eq!(h.step(&b).unwrap(), (event, b.len()));
    }
}
#[test]
fn hostile_lengths_and_terminal_errors() {
    assert_eq!(
        ClientMessage::decode(&[6, 0, 0, 0, 255, 255, 255, 255], Limits::default()),
        Err(Error::Limit)
    );
    assert_eq!(
        ServerInit::decode(&[255; 4], Limits::default()),
        Err(Error::Limit)
    );
    let mut h = ClientHandshake::new(None, true, Limits::default());
    assert!(h.step(b"RFB 003.003\n").is_err());
    assert!(h.step(VERSION).is_err());
}
#[test]
fn vnc_des_known_answer() {
    // Standard DES known-answer key 133457799bbcdff1, bit-reversed for VNC.
    let password = [0xc8, 0x2c, 0xea, 0x9e, 0xd9, 0x3d, 0xfb, 0x8f];
    let block = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    let mut challenge = [0; 16];
    challenge[..8].copy_from_slice(&block);
    challenge[8..].copy_from_slice(&block);
    let expected = [0x85, 0xe8, 0x13, 0x54, 0x0f, 0x0a, 0xb4, 0x05];
    let out = vnc_response(&password, challenge);
    assert_eq!(&out[..8], &expected);
    assert_eq!(&out[8..], &expected);
}
#[test]
fn qemu_extended_key_event_wire_and_keycodes() {
    // RFB community spec / qemu vnc.c: U8 255, U8 0, U16 down, U32 keysym,
    // U32 keycode. 'a' is XT 0x1e.
    let b = ClientMessage::QemuKey {
        down: true,
        keysym: 0x61,
        keycode: qemu_keycode(0x1e, false).unwrap(),
    }
    .encode(Limits::default())
    .unwrap();
    assert_eq!(b, [255, 0, 0, 1, 0, 0, 0, 0x61, 0, 0, 0, 0x1e]);
    // The 0xE0 prefix is bit 7: right Ctrl E0 1D → 0x9d, keypad Enter
    // E0 1C → 0x9c, Up E0 48 → 0xc8; Pause (E1 1D 45) is 0xc6.
    assert_eq!(qemu_keycode(0x1d, true), Some(0x9d));
    assert_eq!(qemu_keycode(0x1c, true), Some(0x9c));
    assert_eq!(qemu_keycode(0x48, true), Some(0xc8));
    assert_eq!(qemu_keycode(0x46, true), Some(0xc6));
    assert_eq!(qemu_keycode(0x01, false), Some(0x01));
    // No key 0, and a break code is not a key.
    assert_eq!(qemu_keycode(0, false), None);
    assert_eq!(qemu_keycode(0x9e, false), None);
    // Other QEMU submessages are refused, and so is a down flag of 2.
    assert_eq!(
        ClientMessage::decode(&[255, 1, 0, 0], Limits::default()),
        Err(Error::Unsupported(255))
    );
    assert_eq!(
        ClientMessage::decode(&[255, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1], Limits::default()),
        Err(Error::Invalid("boolean"))
    );
}
#[test]
fn set_desktop_size_wire_layout() {
    // Message 251, padding, width, height, number-of-screens, padding, then
    // id, x, y, width, height, flags per screen (as noVNC and TigerVNC send).
    let b = ClientMessage::SetDesktopSize {
        width: 640,
        height: 480,
        screens: vec![Screen::whole(1, 640, 480)],
    }
    .encode(Limits::default())
    .unwrap();
    assert_eq!(
        b,
        [
            251, 0, 2, 128, 1, 224, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 2, 128, 1, 224, 0, 0, 0, 0
        ]
    );
    let too_many = ClientMessage::SetDesktopSize {
        width: 640,
        height: 480,
        screens: vec![Screen::whole(1, 640, 480); 256],
    };
    assert_eq!(too_many.encode(Limits::default()), Err(Error::Limit));
}
#[test]
fn valid_layouts() {
    let one = [Screen::whole(0, 640, 480)];
    assert!(valid_layout(640, 480, &one));
    assert!(valid_layout(800, 600, &one));
    assert!(
        !valid_layout(320, 480, &one),
        "screen outside the framebuffer"
    );
    assert!(!valid_layout(640, 480, &[]), "no screens");
    assert!(!valid_layout(0, 480, &one));
    assert!(!valid_layout(640, 480, &[Screen::whole(0, 0, 480)]));
    assert!(
        !valid_layout(640, 480, &[one[0], one[0]]),
        "duplicate screen ids"
    );
}
