#![no_main]
use libfuzzer_sys::fuzz_target;
use stormrfb::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > 16384 {
        return;
    }
    let limits = Limits {
        max_pixels: 4096,
        max_bytes: 65536,
        max_text: 4096,
        max_rectangles: 32,
    };
    let _ = ClientMessage::decode(data, limits);
    let _ = ServerInit::decode(data, limits);
    let _ = PixelFormat::decode(data);
    let mut handshake = ClientHandshake::new(Some(b"fuzz".to_vec()), true, limits);
    let mut pos = 0;
    for _ in 0..8 {
        match handshake.step(&data[pos..]) {
            Ok((_, n)) => pos += n,
            Err(_) => break,
        }
    }
    let mut decoder = ServerDecoder::new(PixelFormat::RGBX, limits).unwrap();
    let mut pos = 0;
    for _ in 0..128 {
        match decoder.next(&data[pos..]) {
            Ok((_, n)) => pos += n,
            Err(_) => break,
        }
    }
    // Wrap mutated tile data in valid framing/compression to reach deep decoders.
    if data.len() >= 2 {
        let width = u16::from(data[0] % 64) + 1;
        let height = u16::from(data[1] % 64) + 1;
        for encoding in [HEXTILE, ZRLE] {
            let mut frame = vec![0, 0, 0, 1, 0, 0, 0, 0];
            frame.extend(width.to_be_bytes());
            frame.extend(height.to_be_bytes());
            frame.extend(encoding.to_be_bytes());
            if encoding == ZRLE {
                let mut compressed = Vec::with_capacity(data.len() * 2 + 128);
                let mut c = flate2::Compress::new(flate2::Compression::fast(), true);
                c.compress_vec(&data[2..], &mut compressed, flate2::FlushCompress::Sync)
                    .unwrap();
                frame.extend((compressed.len() as u32).to_be_bytes());
                frame.extend(compressed);
            } else {
                frame.extend(&data[2..]);
            }
            let mut d = ServerDecoder::new(PixelFormat::RGBX, limits).unwrap();
            d.next(&frame).unwrap();
            let _ = d.next(&frame[4..]);
        }
    }
});
