#![no_main]
use libfuzzer_sys::fuzz_target;
use stormrfb::*;
fuzz_target!(|data: &[u8]| {
    let limits=Limits { max_pixels:4096,max_bytes:65536,max_text:4096,max_rectangles:32 };
    let _=ClientMessage::decode(data,limits);
    let _=ServerInit::decode(data,limits);
    let _=PixelFormat::decode(data);
    let mut handshake=ClientHandshake::new(Some(b"fuzz".to_vec()),true,limits);
    let mut pos=0;
    for _ in 0..8 { match handshake.step(&data[pos..]) { Ok((_,n))=>pos+=n,Err(_)=>break } }
    let mut decoder=ServerDecoder::new(PixelFormat::RGBX,limits).unwrap();
    let mut pos=0;
    for _ in 0..128 { match decoder.next(&data[pos..]) { Ok((_,n))=>pos+=n,Err(_)=>break } }
});
