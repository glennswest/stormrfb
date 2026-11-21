//! When and how a picture goes on the screen, apart from the DRM ioctls
//! that put it there ([`crate::drm::Display`] is the [`Screen`] the binary
//! uses; the tests use a fake one).
//!
//! - A picture waits (`pending`) until it is shown. A failed modeset or
//!   present keeps it waiting and is retried after a delay that doubles from
//!   [`RETRY_MIN`] to [`RETRY_MAX`], so a transient DRM master or modeset
//!   failure on VT acquire is not left black or stale (stormrfb#36,
//!   1831ded79907).
//! - A modeset scans out a buffer already filled with the picture it is
//!   given: on reacquire that is the status screen, never the guest frame
//!   from before the release (c32a95b2eeb1).
//! - [`shutdown`] restores the saved CRTC while still DRM master, only when
//!   this VT is on screen, and drops master after (cdca2b0ca8e3).

use std::io;
use std::time::{Duration, Instant};

use crate::canvas::Canvas;

/// Draws on top of a copied canvas: the buffer and its stride in pixels.
pub type Overlay<'a> = &'a dyn Fn(&mut [u32], usize);

/// A KMS output, as [`Presenter`] and [`shutdown`] use it.
pub trait Screen {
    fn set_master(&mut self) -> io::Result<()>;
    fn drop_master(&mut self) -> io::Result<()>;
    /// Fills the buffer it is about to scan out with `c` (and `overlay`),
    /// then sets the CRTC to it.
    fn modeset(&mut self, c: &Canvas, overlay: Overlay) -> io::Result<()>;
    /// Shows `c` (and `overlay`) on an already set CRTC.
    fn present(&mut self, c: &Canvas, overlay: Overlay) -> io::Result<()>;
    /// Puts back the CRTC as it was found (needs DRM master). Nothing saved
    /// is not an error.
    fn restore(&mut self) -> io::Result<()>;
}

/// At most one present per this.
pub const FRAME: Duration = Duration::from_millis(16);
/// The first retry after a failed modeset or present.
pub const RETRY_MIN: Duration = Duration::from_millis(100);
/// The longest wait between retries.
pub const RETRY_MAX: Duration = Duration::from_secs(2);
/// How long to wait with nothing to show.
pub const IDLE: Duration = Duration::from_millis(500);

pub struct Presenter {
    pending: bool,
    need_modeset: bool,
    last: Option<Instant>,
    retry: Duration,
    retry_at: Option<Instant>,
}

impl Default for Presenter {
    fn default() -> Self {
        Presenter::new()
    }
}

impl Presenter {
    /// Nothing shown yet: the first picture needs a modeset.
    pub fn new() -> Presenter {
        Presenter {
            pending: true,
            need_modeset: true,
            last: None,
            retry: RETRY_MIN,
            retry_at: None,
        }
    }

    /// The picture changed.
    pub fn changed(&mut self) {
        self.pending = true;
    }

    /// The VT is back on screen: the next picture needs a modeset, now.
    pub fn acquired(&mut self) {
        self.pending = true;
        self.need_modeset = true;
        self.retry = RETRY_MIN;
        self.retry_at = None;
    }

    /// A picture is waiting to be shown.
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// The next show is a modeset.
    pub fn needs_modeset(&self) -> bool {
        self.need_modeset
    }

    /// How long until [`Presenter::show`] has something to do.
    pub fn wait(&self, now: Instant) -> Duration {
        if !self.pending {
            return IDLE;
        }
        let mut at = self.last.map_or(now, |l| l + FRAME);
        if let Some(r) = self.retry_at {
            at = at.max(r);
        }
        at.saturating_duration_since(now)
    }

    /// Shows `c` if a picture is waiting (or `force`), the frame time has
    /// passed (unless `force`) and no retry delay is running. `Ok(true)`:
    /// shown; `Ok(false)`: not due; `Err`: the failure, to be retried.
    pub fn show<S: Screen + ?Sized>(
        &mut self,
        s: &mut S,
        now: Instant,
        force: bool,
        c: &Canvas,
        overlay: Overlay,
    ) -> Result<bool, String> {
        if force {
            self.pending = true;
        }
        if !self.pending {
            return Ok(false);
        }
        if self.retry_at.is_some_and(|r| now < r) {
            return Ok(false);
        }
        if !force
            && self
                .last
                .is_some_and(|l| now.saturating_duration_since(l) < FRAME)
        {
            return Ok(false);
        }
        self.last = Some(now);
        let r = if self.need_modeset {
            // Refused when it is already ours or another holds it; the
            // modeset says which.
            let _ = s.set_master();
            s.modeset(c, overlay).map_err(|e| format!("modeset: {e}"))
        } else {
            s.present(c, overlay).map_err(|e| {
                self.need_modeset = true;
                format!("present: {e}")
            })
        };
        match r {
            Ok(()) => {
                self.pending = false;
                self.need_modeset = false;
                self.retry = RETRY_MIN;
                self.retry_at = None;
                Ok(true)
            }
            Err(e) => {
                self.retry_at = Some(now + self.retry);
                self.retry = (self.retry * 2).min(RETRY_MAX);
                Err(e)
            }
        }
    }
}

/// Gives the display back on exit. With this VT on screen (`shown`), the
/// saved CRTC is restored while DRM master is held, then master is dropped;
/// otherwise another VT owns the display and the CRTC is left alone. Returns
/// the restore's failure, if any.
pub fn shutdown<S: Screen + ?Sized>(s: &mut S, shown: bool) -> Option<io::Error> {
    let mut err = None;
    if shown {
        let _ = s.set_master();
        if let Err(e) = s.restore() {
            err = Some(e);
        }
    }
    let _ = s.drop_master();
    err
}
