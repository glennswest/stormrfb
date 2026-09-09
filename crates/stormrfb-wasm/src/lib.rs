//! Browser ABI. Framebuffer storage stays in WASM; JS rebuilds views after growth.
#![forbid(unsafe_code)]
use js_sys::{Array, Uint8Array};
use stormrfb::{ClientMessage, Limits};
use stormrfb_client::{Client, Event, keysym, pointer_buttons};
use wasm_bindgen::prelude::*;
fn error(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}
fn event(kind: &str) -> Array {
    let a = Array::new();
    a.push(&kind.into());
    a
}
#[wasm_bindgen]
pub struct BrowserClient {
    client: Client,
}
#[wasm_bindgen]
impl BrowserClient {
    #[wasm_bindgen(constructor)]
    pub fn new(password: Option<String>) -> Self {
        Self {
            client: Client::new(password.map(String::into_bytes), Limits::default()),
        }
    }
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Array, JsValue> {
        let out = Array::new();
        for e in self.client.receive(bytes).map_err(error)? {
            let a = match e {
                Event::Send(b) => {
                    let a = event("send");
                    a.push(&Uint8Array::from(b.as_slice()));
                    a
                }
                Event::Damage(r) => {
                    let a = event("damage");
                    for v in [r.x, r.y, r.width, r.height] {
                        a.push(&v.into());
                    }
                    a
                }
                Event::Resized { width, height } => {
                    let a = event("resize");
                    a.push(&width.into());
                    a.push(&height.into());
                    a
                }
                Event::Ready { name } => {
                    let a = event("ready");
                    a.push(&String::from_utf8_lossy(&name).as_ref().into());
                    a
                }
                Event::Bell => event("bell"),
                Event::CutText(t) => {
                    let a = event("clipboard");
                    a.push(&t.iter().map(|b| char::from(*b)).collect::<String>().into());
                    a
                }
                Event::Cursor(c) => {
                    let a = event("cursor");
                    for v in [c.x, c.y, c.width, c.height] {
                        a.push(&v.into());
                    }
                    a.push(&Uint8Array::from(c.rgba.as_slice()));
                    a
                }
            };
            out.push(&a);
        }
        Ok(out)
    }
    pub fn width(&self) -> u16 {
        self.client.framebuffer().map_or(0, |f| f.width())
    }
    pub fn height(&self) -> u16 {
        self.client.framebuffer().map_or(0, |f| f.height())
    }
    pub fn framebuffer_ptr(&self) -> usize {
        self.client
            .framebuffer()
            .map_or(0, |f| f.rgba().as_ptr() as usize)
    }
    pub fn key(&self, key: &str, down: bool) -> Result<Uint8Array, JsValue> {
        let sym = keysym(key).ok_or_else(|| error("unmapped key"))?;
        self.key_sym(sym, down)
    }
    pub fn key_sym(&self, keysym: u32, down: bool) -> Result<Uint8Array, JsValue> {
        self.message(ClientMessage::Key { down, keysym })
    }
    pub fn pointer(&self, buttons: u16, x: u16, y: u16, wheel: i8) -> Result<Uint8Array, JsValue> {
        let buttons = pointer_buttons(buttons)
            | if wheel < 0 {
                8
            } else if wheel > 0 {
                16
            } else {
                0
            };
        self.message(ClientMessage::Pointer {
            buttons,
            x: x.min(self.width().saturating_sub(1)),
            y: y.min(self.height().saturating_sub(1)),
        })
    }
    pub fn clipboard(&self, text: &str) -> Result<Uint8Array, JsValue> {
        let bytes = text
            .chars()
            .map(|c| u8::try_from(c as u32).map_err(|_| error("RFB clipboard requires Latin-1")))
            .collect::<Result<Vec<_>, _>>()?;
        self.message(ClientMessage::CutText(bytes))
    }
}
impl BrowserClient {
    fn message(&self, m: ClientMessage) -> Result<Uint8Array, JsValue> {
        let b = self.client.send(m).map_err(error)?;
        Ok(Uint8Array::from(b.as_slice()))
    }
}
