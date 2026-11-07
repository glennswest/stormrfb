//! stormrfb-vt: a VM's display full-screen on a Linux virtual terminal
//! (stormrfb#25, stormcos#228). An RFB client, as the browser's is, that
//! draws to KMS instead of a canvas and takes its input from evdev.
//!
//! [`viewer`] is the part with no I/O: RFB bytes and input events in, bytes
//! to send, VT switches and a fitted XRGB picture out. It is what the tests
//! drive against `stormrfb-server`. [`drm`], [`vt`] and [`evdev`] are the
//! kernel interfaces the binary puts around it.

pub mod canvas;
pub mod drm;
pub mod evdev;
pub mod fit;
pub mod font;
pub mod keys;
pub mod viewer;
pub mod vt;

pub use viewer::{Action, Input, Options, SwitchKeys, Viewer};
