//! Development harness: cargo run -p stormrfb-client --example viewer --features native-harness -- HOST:PORT
use minifb::{MouseButton, MouseMode, Window, WindowOptions};
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};
use stormrfb::{ClientMessage, Limits};
use stormrfb_client::{Client, Event};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::args().nth(1).ok_or("usage: viewer HOST:PORT")?;
    let mut socket = TcpStream::connect(address)?;
    socket.set_read_timeout(Some(Duration::from_millis(5)))?;
    let mut client = Client::new(
        std::env::var("VNC_PASSWORD").ok().map(String::into_bytes),
        Limits::default(),
    );
    let mut window: Option<Window> = None;
    let mut screen = Vec::new();
    let mut input = [0; 65536];
    loop {
        match socket.read(&mut input) {
            Ok(0) => break,
            Ok(n) => {
                for event in client.receive(&input[..n])? {
                    if let Event::Send(bytes) = event {
                        socket.write_all(&bytes)?;
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
        if let Some(fb) = client.framebuffer() {
            let w = usize::from(fb.width());
            let h = usize::from(fb.height());
            if window.is_none() {
                window = Some(Window::new(
                    "stormrfb development harness",
                    w,
                    h,
                    WindowOptions {
                        resize: true,
                        ..WindowOptions::default()
                    },
                )?);
            }
            let win = window.as_mut().unwrap();
            if !win.is_open() {
                break;
            }
            screen.clear();
            screen.extend(
                fb.rgba()
                    .chunks_exact(4)
                    .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])),
            );
            win.update_with_buffer(&screen, w, h)?;
            if let Some((x, y)) = win.get_mouse_pos(MouseMode::Clamp) {
                let buttons = u8::from(win.get_mouse_down(MouseButton::Left))
                    | (u8::from(win.get_mouse_down(MouseButton::Middle)) << 1)
                    | (u8::from(win.get_mouse_down(MouseButton::Right)) << 2);
                let (ww, wh) = win.get_size();
                socket.write_all(&client.send(ClientMessage::Pointer {
                    buttons,
                    x: (x * w as f32 / ww as f32) as u16,
                    y: (y * h as f32 / wh as f32) as u16,
                })?)?;
            }
        }
    }
    Ok(())
}
