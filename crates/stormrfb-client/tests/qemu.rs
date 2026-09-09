use stormrfb::*;
use stormrfb_client::Framebuffer;
#[test]
fn qemu_zrle_matches_independent_qmp_screendump() {
    check(
        include_bytes!("../../../fixtures/qemu-zrle.rfb"),
        720,
        400,
        0x06133f0cfae0b305,
    );
}
#[test]
fn tigervnc_zrle_matches_independent_xgetimage() {
    check(
        include_bytes!("../../../fixtures/tigervnc-zrle.rfb"),
        73,
        69,
        0x091a3de23b92f334,
    );
}
fn check(bytes: &[u8], width: u16, height: u16, expected: u64) {
    let mut decoder = ServerDecoder::new(PixelFormat::RGBX, Limits::default()).unwrap();
    let mut fb = Framebuffer::new(width, height, Limits::default()).unwrap();
    let mut pos = 0;
    let mut updates = 0;
    loop {
        match decoder.next(&bytes[pos..]) {
            Ok((e, n)) => {
                pos += n;
                match e {
                    ServerEvent::Rectangle(r) => {
                        fb.apply(r).unwrap();
                    }
                    ServerEvent::UpdateEnd => {
                        updates += 1;
                        let hash = fb.rgba().iter().fold(0xcbf29ce484222325u64, |h, b| {
                            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
                        });
                        assert_eq!(hash, expected);
                    }
                    _ => {}
                }
            }
            Err(Error::Incomplete) => break,
            Err(e) => panic!("{e} at {pos}"),
        }
    }
    assert_eq!(pos, bytes.len());
    assert_eq!(updates, 2);
}
