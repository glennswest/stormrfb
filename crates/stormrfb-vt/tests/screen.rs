//! `screen::Presenter` and `screen::shutdown` against a fake screen
//! (stormrfb#36): a failed modeset or present is retried until a picture is
//! shown, a reacquire's modeset scans out the status screen and never the
//! guest frame from before, and shutdown restores the CRTC while master,
//! only when this VT is on screen.

use std::io;
use std::time::{Duration, Instant};

use stormrfb_vt::canvas::Canvas;
use stormrfb_vt::screen::{self, FRAME, Overlay, Presenter, RETRY_MAX, RETRY_MIN, Screen};

/// Records the calls; the scanned-out picture is the canvas's first pixel.
#[derive(Default)]
struct Fake {
    calls: Vec<&'static str>,
    /// Failures still to give, per call.
    fail_modeset: u32,
    fail_present: u32,
    fail_restore: bool,
    /// The first pixel of what is on the CRTC, set only by a good call.
    scanned: Option<u32>,
}

fn eio() -> io::Error {
    io::Error::from_raw_os_error(libc::EIO)
}

impl Screen for Fake {
    fn set_master(&mut self) -> io::Result<()> {
        self.calls.push("set_master");
        Ok(())
    }
    fn drop_master(&mut self) -> io::Result<()> {
        self.calls.push("drop_master");
        Ok(())
    }
    fn modeset(&mut self, c: &Canvas, _: Overlay) -> io::Result<()> {
        self.calls.push("modeset");
        if self.fail_modeset > 0 {
            self.fail_modeset -= 1;
            return Err(eio());
        }
        self.scanned = Some(c.pixels[0]);
        Ok(())
    }
    fn present(&mut self, c: &Canvas, _: Overlay) -> io::Result<()> {
        self.calls.push("present");
        if self.fail_present > 0 {
            self.fail_present -= 1;
            return Err(eio());
        }
        self.scanned = Some(c.pixels[0]);
        Ok(())
    }
    fn restore(&mut self) -> io::Result<()> {
        self.calls.push("restore");
        if self.fail_restore {
            return Err(eio());
        }
        Ok(())
    }
}

fn canvas(c: u32) -> Canvas {
    let mut k = Canvas::new(4, 4);
    k.clear(c);
    k
}

fn none(_: &mut [u32], _: usize) {}

#[test]
fn the_first_picture_is_a_modeset_of_that_picture() {
    let mut s = Fake::default();
    let mut p = Presenter::new();
    let t = Instant::now();
    assert_eq!(p.show(&mut s, t, false, &canvas(7), &none), Ok(true));
    assert_eq!(s.calls, ["set_master", "modeset"]);
    assert_eq!(s.scanned, Some(7));
    assert!(!p.pending());
    // Nothing changed: nothing to do, and the loop may idle.
    assert_eq!(
        p.show(&mut s, t + FRAME, false, &canvas(7), &none),
        Ok(false)
    );
    assert_eq!(p.wait(t + FRAME), screen::IDLE);
}

#[test]
fn a_change_is_presented_at_most_once_a_frame() {
    let mut s = Fake::default();
    let mut p = Presenter::new();
    let t = Instant::now();
    p.show(&mut s, t, false, &canvas(1), &none).unwrap();
    p.changed();
    assert_eq!(
        p.show(&mut s, t + FRAME / 2, false, &canvas(2), &none),
        Ok(false)
    );
    assert_eq!(p.wait(t + FRAME / 2), FRAME - FRAME / 2);
    assert_eq!(
        p.show(&mut s, t + FRAME, false, &canvas(2), &none),
        Ok(true)
    );
    assert_eq!(s.calls.last(), Some(&"present"));
    assert_eq!(s.scanned, Some(2));
}

#[test]
fn a_failed_modeset_keeps_the_picture_waiting_and_is_retried() {
    // 1831ded79907: a static guest frame (no further change) after a failed
    // modeset on acquire must still reach the screen.
    let mut s = Fake {
        fail_modeset: 3,
        ..Fake::default()
    };
    let mut p = Presenter::new();
    let t = Instant::now();
    let frame = canvas(9);
    assert!(p.show(&mut s, t, false, &frame, &none).is_err());
    assert!(p.pending() && p.needs_modeset());
    // Not before the retry delay: no busy loop on a failing display.
    assert_eq!(p.wait(t), RETRY_MIN);
    assert_eq!(
        p.show(&mut s, t + RETRY_MIN / 2, false, &frame, &none),
        Ok(false)
    );
    // The delay doubles: 100 ms, then 200 ms, then 400 ms.
    let t1 = t + RETRY_MIN;
    assert!(p.show(&mut s, t1, false, &frame, &none).is_err());
    assert_eq!(p.wait(t1), RETRY_MIN * 2);
    let t2 = t1 + RETRY_MIN * 2;
    assert!(p.show(&mut s, t2, false, &frame, &none).is_err());
    assert_eq!(p.wait(t2), RETRY_MIN * 4);
    assert_eq!(s.scanned, None);
    // No change was reported meanwhile; the retry alone shows it.
    let t3 = t2 + RETRY_MIN * 4;
    assert_eq!(p.show(&mut s, t3, false, &frame, &none), Ok(true));
    assert_eq!(s.scanned, Some(9));
    assert!(!p.pending() && !p.needs_modeset());
    assert_eq!(s.calls.iter().filter(|c| **c == "modeset").count(), 4);
}

#[test]
fn the_retry_delay_is_bounded() {
    let mut s = Fake {
        fail_modeset: u32::MAX,
        ..Fake::default()
    };
    let mut p = Presenter::new();
    let mut t = Instant::now();
    let mut w = Duration::ZERO;
    for _ in 0..20 {
        assert!(p.show(&mut s, t, false, &canvas(1), &none).is_err());
        w = p.wait(t);
        assert!(w <= RETRY_MAX, "{w:?}");
        t += w;
    }
    assert_eq!(w, RETRY_MAX);
    assert_eq!(s.calls.iter().filter(|c| **c == "modeset").count(), 20);
}

#[test]
fn a_failed_present_is_retried_as_a_modeset() {
    let mut s = Fake::default();
    let mut p = Presenter::new();
    let t = Instant::now();
    p.show(&mut s, t, false, &canvas(1), &none).unwrap();
    s.fail_present = 1;
    p.changed();
    let t1 = t + FRAME;
    assert!(p.show(&mut s, t1, false, &canvas(2), &none).is_err());
    assert!(p.pending() && p.needs_modeset());
    let t2 = t1 + RETRY_MIN;
    assert_eq!(p.show(&mut s, t2, false, &canvas(2), &none), Ok(true));
    assert_eq!(s.calls.last(), Some(&"modeset"));
    assert_eq!(s.scanned, Some(2));
}

#[test]
fn reacquire_scans_out_the_status_screen_not_the_old_frame() {
    // c32a95b2eeb1: the modeset on acquire shows what it is given (the
    // status screen while the viewer's fresh-frame gate is shut), and is
    // the only call, so the old guest frame is never scanned out.
    let guest = canvas(0x00ff_0000);
    let status = canvas(0x0000_00ff);
    let mut s = Fake::default();
    let mut p = Presenter::new();
    let t = Instant::now();
    p.show(&mut s, t, false, &guest, &none).unwrap();
    assert_eq!(s.scanned, Some(0x00ff_0000));
    // Released: nothing is shown (the binary does not call show). Back:
    p.acquired();
    s.calls.clear();
    s.scanned = None;
    assert_eq!(p.show(&mut s, t + FRAME, false, &status, &none), Ok(true));
    assert_eq!(s.calls, ["set_master", "modeset"]);
    assert_eq!(s.scanned, Some(0x0000_00ff));
}

#[test]
fn acquire_cancels_a_running_retry_delay() {
    let mut s = Fake {
        fail_modeset: 1,
        ..Fake::default()
    };
    let mut p = Presenter::new();
    let t = Instant::now();
    assert!(p.show(&mut s, t, false, &canvas(1), &none).is_err());
    p.acquired();
    assert_eq!(
        p.show(&mut s, t + FRAME, false, &canvas(1), &none),
        Ok(true)
    );
}

#[test]
fn shutdown_on_screen_restores_while_master_then_drops_it() {
    // cdca2b0ca8e3
    let mut s = Fake::default();
    assert!(screen::shutdown(&mut s, true).is_none());
    assert_eq!(s.calls, ["set_master", "restore", "drop_master"]);
}

#[test]
fn shutdown_off_screen_leaves_the_crtc_alone() {
    let mut s = Fake::default();
    assert!(screen::shutdown(&mut s, false).is_none());
    assert_eq!(s.calls, ["drop_master"]);
}

#[test]
fn shutdown_reports_a_failed_restore_and_still_drops_master() {
    let mut s = Fake {
        fail_restore: true,
        ..Fake::default()
    };
    let e = screen::shutdown(&mut s, true).expect("the restore's error");
    assert_eq!(e.raw_os_error(), Some(libc::EIO));
    assert_eq!(s.calls, ["set_master", "restore", "drop_master"]);
}
