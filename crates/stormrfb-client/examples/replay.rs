use stormrfb::*;
use stormrfb_client::Framebuffer;
fn replay() -> Result<()> {
    let b = include_bytes!("../../../fixtures/qemu-zrle.rfb");
    let limits = Limits::default();
    let mut d = ServerDecoder::new(PixelFormat::RGBX, limits)?;
    let mut f = Framebuffer::new(720, 400, limits)?;
    let mut pos = 0;
    loop {
        match d.next(&b[pos..]) {
            Ok((e, n)) => {
                pos += n;
                if let ServerEvent::Rectangle(r) = e {
                    f.apply(r)?;
                }
            }
            Err(Error::Incomplete) => break,
            Err(e) => return Err(e),
        }
    }
    std::hint::black_box(f);
    Ok(())
}
fn main() -> Result<()> {
    for _ in 0..20 {
        replay()?;
    }
    let start = std::time::Instant::now();
    for _ in 0..200 {
        replay()?;
    }
    println!(
        "QEMU 720x400, 400 frames: {:.3} ms/frame, 902 bytes/frame (decode + framebuffer, including per-session allocation)",
        start.elapsed().as_secs_f64() * 1000.0 / 400.0
    );
    Ok(())
}
