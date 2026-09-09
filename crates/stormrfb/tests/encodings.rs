use stormrfb::*;
fn decode_all(d: &mut ServerDecoder, b: &[u8]) -> Vec<ServerEvent> {
    let mut pos = 0;
    let mut events = vec![];
    loop {
        match d.next(&b[pos..]) {
            Ok((e, n)) => {
                pos += n;
                events.push(e);
            }
            Err(Error::Incomplete) => break,
            Err(e) => panic!("{e:?} at {pos}"),
        }
    }
    assert_eq!(pos, b.len());
    events
}
#[test]
fn encodings_match_raw_across_tile_edges_and_stream_updates() {
    let rect = Rect {
        x: 3,
        y: 4,
        width: 67,
        height: 65,
    };
    let pixels = (0..67 * 65)
        .map(|i| [(i % 251) as u8, (i % 79) as u8, (i % 13) as u8, 255])
        .collect();
    let rectangle = Rectangle::Pixels { rect, pixels };
    for encoding in [RAW, HEXTILE, ZRLE] {
        let mut e = ServerEncoder::new(Limits::default());
        let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
        for _ in 0..3 {
            let b = e
                .update(
                    std::slice::from_ref(&rectangle),
                    PixelFormat::RGBX,
                    encoding,
                )
                .unwrap();
            assert_eq!(
                decode_all(&mut d, &b),
                vec![
                    ServerEvent::UpdateStart,
                    ServerEvent::Rectangle(rectangle.clone()),
                    ServerEvent::UpdateEnd
                ]
            );
        }
    }
}
#[test]
fn rectangle_fragmentation() {
    for encoding in [RAW, HEXTILE, ZRLE] {
        let rectangle = Rectangle::Pixels {
            rect: Rect {
                width: 3,
                height: 2,
                ..Rect::default()
            },
            pixels: vec![[5, 6, 7, 255]; 6],
        };
        let b = ServerEncoder::new(Limits::default())
            .update(
                std::slice::from_ref(&rectangle),
                PixelFormat::RGBX,
                encoding,
            )
            .unwrap();
        let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
        assert_eq!(d.next(&b).unwrap(), (ServerEvent::UpdateStart, 4));
        for end in 4..b.len() {
            assert_eq!(d.next(&b[4..end]), Err(Error::Incomplete));
        }
        assert_eq!(
            d.next(&b[4..]).unwrap(),
            (ServerEvent::Rectangle(rectangle), b.len() - 4)
        );
        assert_eq!(d.next(&[]).unwrap(), (ServerEvent::UpdateEnd, 0));
    }
}
#[test]
fn pseudo_and_control_roundtrips() {
    let rects = vec![
        Rectangle::Copy {
            rect: Rect {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            },
            source_x: 5,
            source_y: 6,
        },
        Rectangle::DesktopSize {
            width: 800,
            height: 600,
        },
        Rectangle::Cursor {
            hotspot_x: 0,
            hotspot_y: 0,
            width: 1,
            height: 1,
            pixels: vec![[1, 2, 3, 255]],
            mask: vec![128],
        },
    ];
    let b = ServerEncoder::new(Limits::default())
        .update(&rects, PixelFormat::RGBX, RAW)
        .unwrap();
    let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    let events = decode_all(&mut d, &b);
    assert_eq!(
        &events[1..4],
        rects
            .into_iter()
            .map(ServerEvent::Rectangle)
            .collect::<Vec<_>>()
    );
    for event in [
        ServerEvent::Bell,
        ServerEvent::CutText(vec![0, 255, 13]),
        ServerEvent::ColourMap {
            first: 3,
            colors: vec![[1, 2, 65535]],
        },
    ] {
        let b = event.encode_control(Limits::default()).unwrap();
        assert_eq!(decode_all(&mut d, &b), vec![event]);
    }
}
#[test]
fn last_rect_ends_unknown_count() {
    let mut b = vec![0, 0, 255, 255];
    b.extend([0; 8]);
    b.extend(LAST_RECT.to_be_bytes());
    b.push(2);
    let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    assert_eq!(
        decode_all(&mut d, &b),
        vec![
            ServerEvent::UpdateStart,
            ServerEvent::UpdateEnd,
            ServerEvent::Bell
        ]
    );
}
#[test]
fn unknown_count_still_obeys_rectangle_budget() {
    let limits = Limits {
        max_rectangles: 1,
        ..Limits::default()
    };
    let mut d = ServerDecoder::new(PixelFormat::RGBX, limits).unwrap();
    d.next(&[0, 0, 255, 255]).unwrap();
    let mut r = vec![0; 8];
    r.extend(COPY_RECT.to_be_bytes());
    r.extend([0; 4]);
    d.next(&r).unwrap();
    assert_eq!(d.next(&r), Err(Error::Limit));
    assert!(d.next(&[]).is_err());
}
#[test]
fn inflated_data_is_bounded_and_failure_is_terminal() {
    let r = Rectangle::Pixels {
        rect: Rect {
            width: 64,
            height: 64,
            ..Rect::default()
        },
        pixels: vec![[0, 0, 0, 255]; 4096],
    };
    let mut b = ServerEncoder::new(Limits::default())
        .update(&[r], PixelFormat::RGBX, ZRLE)
        .unwrap();
    // Claim one pixel while retaining a compressed full tile.
    b[8..12].copy_from_slice(&[0, 1, 0, 1]);
    let mut d = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    d.next(&b).unwrap();
    assert_eq!(d.next(&b[4..]), Err(Error::Limit));
    assert!(d.next(&[]).is_err());
}
