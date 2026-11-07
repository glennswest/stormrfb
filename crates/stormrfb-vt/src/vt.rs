//! The viewer's virtual terminal. stormpump starts it with `tty tty5`: the
//! VT is fds 0–2 and the controlling terminal. The viewer puts the VT in
//! graphics mode (the kernel stops drawing text on it), in VT_PROCESS mode
//! (switching away sends SIGUSR1: release, drop DRM master and input, then
//! allow the switch; switching back SIGUSR2: acquire, take them back and ask
//! for a whole frame), and its keyboard in K_OFF: keys reach the guest
//! through evdev only, so nothing piles up on the tty, Ctrl+Alt+Del goes to
//! the guest and not the node, and the viewer does Alt+Fn itself with
//! VT_ACTIVATE. On exit the VT goes back to text, its keyboard mode and
//! auto switching.
//!
//! Adapted from stormpanel's `src/vt.rs` (glennswest/stormpanel, MIT).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

const KDSETMODE: libc::c_ulong = 0x4B3A;
const KD_TEXT: libc::c_ulong = 0x00;
const KD_GRAPHICS: libc::c_ulong = 0x01;
const VT_GETMODE: libc::c_ulong = 0x5601;
const VT_SETMODE: libc::c_ulong = 0x5602;
const VT_GETSTATE: libc::c_ulong = 0x5603;
const VT_RELDISP: libc::c_ulong = 0x5605;
const VT_ACTIVATE: libc::c_ulong = 0x5606;
const KDGKBMODE: libc::c_ulong = 0x4B44;
const KDSKBMODE: libc::c_ulong = 0x4B45;
const K_OFF: libc::c_ulong = 0x04;
const VT_AUTO: i8 = 0;
const VT_PROCESS: i8 = 1;
const VT_ACKACQ: libc::c_ulong = 2;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct VtMode {
    mode: i8,
    waitv: i8,
    relsig: i16,
    acqsig: i16,
    frsig: i16,
}

#[repr(C)]
#[derive(Default)]
struct VtStat {
    v_active: u16,
    v_signal: u16,
    v_state: u16,
}

pub static RELEASE: AtomicBool = AtomicBool::new(false);
pub static ACQUIRE: AtomicBool = AtomicBool::new(false);
pub static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    match sig {
        libc::SIGUSR1 => RELEASE.store(true, Ordering::SeqCst),
        libc::SIGUSR2 => ACQUIRE.store(true, Ordering::SeqCst),
        _ => QUIT.store(true, Ordering::SeqCst),
    }
}

/// Handlers without SA_RESTART, so a sleeping main loop wakes on a switch.
pub fn install_signals() {
    for s in [
        libc::SIGUSR1,
        libc::SIGUSR2,
        libc::SIGTERM,
        libc::SIGINT,
        libc::SIGHUP,
    ] {
        let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
        sa.sa_sigaction = on_signal as *const () as usize;
        unsafe {
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(s, &sa, std::ptr::null_mut());
        }
    }
}

/// Blocks the switch and stop signals, and returns the mask to wait with
/// (`ppoll`), which lets them in: a signal then only ever arrives while the
/// loop waits, never between its check of the flags and the wait.
pub fn block_signals() -> libc::sigset_t {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        let mut old: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGHUP,
        ] {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old);
        for s in [
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGHUP,
        ] {
            libc::sigdelset(&mut old, s);
        }
        old
    }
}

pub struct Vt {
    fd: i32,
    /// This VT's number (8 for tty8).
    pub num: u16,
    saved: Option<VtMode>,
    saved_kb: Option<libc::c_ulong>,
}

impl Vt {
    /// The VT on `fd` (normally 0, which stormpump made the terminal).
    pub fn on_fd(fd: i32) -> io::Result<Vt> {
        let num = vt_number(fd)?;
        Ok(Vt {
            fd,
            num,
            saved: None,
            saved_kb: None,
        })
    }

    /// Graphics mode and process-controlled switching.
    pub fn take(&mut self) -> io::Result<()> {
        let mut old = VtMode::default();
        if unsafe { libc::ioctl(self.fd, VT_GETMODE as _, &mut old) } == 0 {
            self.saved = Some(old);
        }
        let mut m = VtMode {
            mode: VT_PROCESS,
            waitv: 0,
            relsig: libc::SIGUSR1 as i16,
            acqsig: libc::SIGUSR2 as i16,
            frsig: 0,
        };
        if unsafe { libc::ioctl(self.fd, VT_SETMODE as _, &mut m) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::ioctl(self.fd, KDSETMODE as _, KD_GRAPHICS) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut kb: libc::c_int = 0;
        if unsafe { libc::ioctl(self.fd, KDGKBMODE as _, &mut kb) } == 0 {
            self.saved_kb = Some(kb as libc::c_ulong);
        }
        if unsafe { libc::ioctl(self.fd, KDSKBMODE as _, K_OFF) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Asks the kernel to switch the console to VT `n` (Alt+Fn here).
    pub fn activate(&self, n: u16) {
        if n != self.num {
            unsafe { libc::ioctl(self.fd, VT_ACTIVATE as _, libc::c_ulong::from(n)) };
        }
    }

    /// Whether this VT is the one on screen.
    pub fn active(&self) -> bool {
        let mut s = VtStat::default();
        unsafe { libc::ioctl(self.fd, VT_GETSTATE as _, &mut s) == 0 && s.v_active == self.num }
    }

    /// Lets a pending switch away go ahead (after dropping the display).
    pub fn release(&self) {
        unsafe { libc::ioctl(self.fd, VT_RELDISP as _, 1 as libc::c_ulong) };
    }

    /// Acknowledges that the VT is ours again.
    pub fn ack_acquire(&self) {
        unsafe { libc::ioctl(self.fd, VT_RELDISP as _, VT_ACKACQ) };
    }

    /// Text mode and automatic switching again.
    pub fn restore(&self) {
        unsafe {
            libc::ioctl(self.fd, KDSETMODE as _, KD_TEXT);
            if let Some(kb) = self.saved_kb {
                libc::ioctl(self.fd, KDSKBMODE as _, kb);
            }
            let mut m = self.saved.unwrap_or(VtMode {
                mode: VT_AUTO,
                ..Default::default()
            });
            if m.mode == VT_PROCESS {
                m = VtMode {
                    mode: VT_AUTO,
                    ..Default::default()
                };
            }
            libc::ioctl(self.fd, VT_SETMODE as _, &mut m);
        }
    }
}

impl Drop for Vt {
    fn drop(&mut self) {
        self.restore();
    }
}

/// The VT number of a terminal fd: a tty's device is major 4, minors 1–63.
fn vt_number(fd: i32) -> io::Result<u16> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let (maj, min) = (libc::major(st.st_rdev), libc::minor(st.st_rdev));
    if maj != 4 || !(1..=63).contains(&min) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "fd {fd} is not a virtual terminal (device {maj}:{min}); run on a VT, e.g. stormpump's `tty tty5`"
            ),
        ));
    }
    Ok(min as u16)
}
