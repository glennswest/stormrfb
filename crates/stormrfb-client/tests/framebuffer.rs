use stormrfb::*;
use stormrfb_client::*;
#[test]
fn overlap_copy_matches_snapshot_in_all_directions() {
    for (sx, sy, dx, dy) in [(0, 0, 1, 1), (1, 1, 0, 0), (0, 0, 1, 0), (1, 0, 0, 0)] {
        let mut fb = Framebuffer::new(4, 4, Limits::default()).unwrap();
        let pixels = (0..16).map(|i| [i, 0, 0, 255]).collect();
        fb.apply(Rectangle::Pixels {
            rect: fb.rect(),
            pixels,
        })
        .unwrap();
        let before = fb.rgba().to_vec();
        let r = Rect {
            x: dx,
            y: dy,
            width: 3,
            height: 3,
        };
        fb.apply(Rectangle::Copy {
            rect: r,
            source_x: sx,
            source_y: sy,
        })
        .unwrap();
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(
                    fb.rgba()[((dy + y) * 4 + dx + x) as usize * 4],
                    before[((sy + y) * 4 + sx + x) as usize * 4]
                );
            }
        }
    }
}
#[test]
fn resize_cursor_and_invalid_bounds() {
    let mut fb = Framebuffer::new(2, 2, Limits::default()).unwrap();
    assert!(
        fb.apply(Rectangle::Copy {
            rect: fb.rect(),
            source_x: 1,
            source_y: 0
        })
        .is_err()
    );
    let e = fb
        .apply(Rectangle::Cursor {
            hotspot_x: 0,
            hotspot_y: 0,
            width: 2,
            height: 1,
            pixels: vec![[1, 2, 3, 0]; 2],
            mask: vec![128],
        })
        .unwrap();
    let Event::Cursor(c) = e else { panic!() };
    assert_eq!(c.rgba, vec![1, 2, 3, 255, 1, 2, 3, 0]);
    fb.apply(Rectangle::DesktopSize {
        width: 3,
        height: 1,
    })
    .unwrap();
    assert_eq!(fb.rgba().len(), 12);
    assert_eq!(fb.rgba()[3], 255);
}
#[test]
fn input_translation() {
    assert_eq!(keysym("Enter"), Some(0xff0d));
    assert_eq!(keysym("F12"), Some(0xffc9));
    assert_eq!(keysym("é"), Some(233));
    assert_eq!(keysym("😀"), Some(0x0101f600));
    assert_eq!(keysym("Dead"), None);
    assert_eq!(pointer_buttons(2), 4);
    assert_eq!(pointer_buttons(4), 2);
}
