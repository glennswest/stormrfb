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
        ClientMessage::Pointer {
            buttons: 5,
            x: 42,
            y: 51,
        },
        ClientMessage::CutText(b"clipboard\n".to_vec()),
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
