//! stormrfb-vt — a VM's display full-screen on the VT it runs on.
//!
//!   stormrfb-vt --vm default/desk         /run/stormvm/default/desk/vnc.sock
//!   stormrfb-vt --socket PATH             any RFB server on a Unix socket
//!   stormrfb-vt --connect HOST:PORT       or on TCP
//!
//! Options: --run-dir DIR (default /run/stormvm), --password-file FILE,
//! --device /dev/dri/cardN, --switch alt|ctrl-alt, --no-resize.
//! stormpump runs it as a `domain task` with `tty tty5`.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use stormrfb::Limits;
use stormrfb_vt::canvas::Canvas;
use stormrfb_vt::evdev::Devices;
use stormrfb_vt::{Action, Options, SwitchKeys, Viewer, drm, vt};

const USAGE: &str = "usage: stormrfb-vt (--vm NS/NAME | --socket PATH | --connect HOST:PORT)
                   [--run-dir DIR] [--password-file FILE] [--device /dev/dri/cardN]
                   [--switch alt|ctrl-alt] [--no-resize]
       stormrfb-vt --version";

enum Target {
    Unix(String),
    Tcp(String),
}

struct Args {
    target: Target,
    label: String,
    password: Option<Vec<u8>>,
    device: Option<String>,
    opts: Options,
}

fn parse_args() -> Result<Args, String> {
    let mut vm = None;
    let mut socket = None;
    let mut connect = None;
    let mut run_dir = "/run/stormvm".to_string();
    let mut password = None;
    let mut device = None;
    let mut opts = Options::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--vm" => vm = Some(val("--vm")?),
            "--socket" => socket = Some(val("--socket")?),
            "--connect" => connect = Some(val("--connect")?),
            "--run-dir" => run_dir = val("--run-dir")?,
            "--device" => device = Some(val("--device")?),
            "--password-file" => {
                let f = val("--password-file")?;
                let p = std::fs::read(&f).map_err(|e| format!("{f}: {e}"))?;
                password = Some(p.trim_ascii_end().to_vec());
            }
            "--switch" => {
                opts.switch = match val("--switch")?.as_str() {
                    "alt" => SwitchKeys::Alt,
                    "ctrl-alt" => SwitchKeys::CtrlAlt,
                    other => return Err(format!("--switch is alt or ctrl-alt, not {other:?}")),
                }
            }
            "--no-resize" => opts.resize = false,
            "--version" | "-V" => {
                println!("stormrfb-vt {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    let (target, label) = match (vm, socket, connect) {
        (Some(vm), None, None) => {
            if vm.split('/').count() != 2 || vm.contains("..") {
                return Err(format!("--vm is NAMESPACE/NAME, not {vm:?}"));
            }
            (Target::Unix(format!("{run_dir}/{vm}/vnc.sock")), vm)
        }
        (None, Some(s), None) => (Target::Unix(s.clone()), s),
        (None, None, Some(c)) => (Target::Tcp(c.clone()), c),
        _ => return Err(format!("one of --vm, --socket or --connect\n{USAGE}")),
    };
    Ok(Args {
        target,
        label,
        password,
        device,
        opts,
    })
}

/// One line to the kernel log (tty7 and serial carry it), or stderr where
/// /dev/kmsg is not writable. Our own stderr is the VT, in graphics mode.
fn log(msg: &str) {
    let line = format!("stormrfb-vt: {msg}\n");
    match std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        Ok(mut k) => {
            let _ = k.write_all(format!("<6>{line}").as_bytes());
        }
        Err(_) => eprint!("{line}"),
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("stormrfb-vt: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args) {
        log(&e);
        eprintln!("stormrfb-vt: {e}");
        std::process::exit(1);
    }
}

trait Stream: Read + Write + AsRawFd {}
impl Stream for UnixStream {}
impl Stream for TcpStream {}

fn connect(t: &Target) -> io::Result<Box<dyn Stream>> {
    let s: Box<dyn Stream> = match t {
        Target::Unix(p) => {
            let s = UnixStream::connect(p)?;
            s.set_write_timeout(Some(Duration::from_secs(5)))?;
            Box::new(s)
        }
        Target::Tcp(a) => {
            let s = TcpStream::connect(a)?;
            s.set_nodelay(true)?;
            s.set_write_timeout(Some(Duration::from_secs(5)))?;
            Box::new(s)
        }
    };
    Ok(s)
}

/// The console: the VT, the screen and the input devices, and whether the
/// VT is the one on screen.
struct Console {
    term: vt::Vt,
    disp: drm::Display,
    input: Devices,
    shown: bool,
    need_modeset: bool,
    wait_mask: libc::sigset_t,
    status: Canvas,
    last_err: String,
    last_present: Instant,
    pending: bool,
}

/// What a session's end says on the screen.
struct Ended {
    why: String,
    was_ready: bool,
}

impl Console {
    /// Handles a switch signal. With a session, its keys are let go of on
    /// release and a whole frame asked for on acquire.
    fn switches(&mut self, mut viewer: Option<&mut Viewer>, out: &mut Vec<Action>) {
        if vt::RELEASE.swap(false, Ordering::SeqCst) {
            if let Some(v) = viewer.as_deref_mut() {
                out.extend(v.release());
            }
            self.input.close();
            self.shown = false;
            let _ = self.disp.drop_master();
            self.term.release();
        }
        if vt::ACQUIRE.swap(false, Ordering::SeqCst) {
            self.term.ack_acquire();
            self.shown = true;
            self.need_modeset = true;
            self.pending = true;
            self.input.scan();
            if let Some(v) = viewer {
                out.extend(v.acquire());
            }
        }
    }

    /// Puts the picture (or, without one, the status screen) on the screen
    /// if it changed, at most once a frame.
    fn present(&mut self, viewer: Option<&Viewer>, force: bool) {
        if !self.shown || !(self.pending || force) {
            return;
        }
        if !force && self.last_present.elapsed() < Duration::from_millis(16) {
            return;
        }
        if self.need_modeset {
            let _ = self.disp.set_master();
            match self.disp.modeset() {
                Ok(()) => self.need_modeset = false,
                Err(e) => note(&mut self.last_err, &format!("modeset: {e}")),
            }
        }
        let r = match viewer.and_then(|v| v.frame().map(|f| (v, f))) {
            Some((v, frame)) => self
                .disp
                .present(frame, &|buf: &mut [u32], stride: usize| {
                    v.draw_cursor(buf, stride)
                }),
            None => self
                .disp
                .present(&self.status, &|_: &mut [u32], _: usize| {}),
        };
        match r {
            Ok(()) => self.last_err.clear(),
            Err(e) => {
                note(&mut self.last_err, &format!("present: {e}"));
                self.need_modeset = true;
            }
        }
        self.pending = false;
        self.last_present = Instant::now();
    }

    fn set_status(&mut self, lines: &[&str]) {
        self.status.status(lines);
        self.pending = true;
    }

    /// Waits for `fds` (or the timeout, or a signal). Returns which are
    /// readable.
    fn wait(&self, fds: &[i32], timeout: Duration) -> Vec<bool> {
        let mut p: Vec<libc::pollfd> = fds
            .iter()
            .map(|&fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let ts = libc::timespec {
            tv_sec: timeout.as_secs() as _,
            tv_nsec: timeout.subsec_nanos() as _,
        };
        let n = unsafe { libc::ppoll(p.as_mut_ptr(), p.len() as _, &ts, &self.wait_mask) };
        if n <= 0 {
            return vec![false; fds.len()];
        }
        p.iter()
            .map(|p| p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
            .collect()
    }
}

fn run(args: &Args) -> Result<(), String> {
    vt::install_signals();
    let wait_mask = vt::block_signals();
    let mut term = vt::Vt::on_fd(0).map_err(|e| e.to_string())?;
    let disp = drm::Display::open(args.device.as_deref()).map_err(|e| format!("display: {e}"))?;
    term.take().map_err(|e| format!("tty{}: {e}", term.num))?;
    log(&format!(
        "v{} on tty{}, {}x{}@{}, showing {}",
        env!("CARGO_PKG_VERSION"),
        term.num,
        disp.width,
        disp.height,
        disp.mode.vrefresh,
        args.label
    ));
    let (w, h) = (disp.width, disp.height);
    let mut con = Console {
        shown: term.active(),
        term,
        disp,
        input: Devices::default(),
        need_modeset: true,
        wait_mask,
        status: Canvas::new(w, h),
        last_err: String::new(),
        last_present: Instant::now(),
        pending: true,
    };
    if con.shown {
        con.input.scan();
    } else {
        let _ = con.disp.drop_master();
    }
    let mut backoff = Duration::from_secs(1);
    while !vt::QUIT.load(Ordering::SeqCst) {
        con.set_status(&[&args.label, "connecting"]);
        con.present(None, true);
        let ended = match connect(&args.target) {
            Ok(s) => session(&mut con, args, s),
            Err(e) => Ended {
                why: format!("not running ({e})"),
                was_ready: false,
            },
        };
        if vt::QUIT.load(Ordering::SeqCst) {
            break;
        }
        if ended.was_ready {
            backoff = Duration::from_secs(1);
        }
        log(&format!("{}: {}; retrying in {} s", args.label, ended.why, backoff.as_secs()));
        // Wait out the backoff, still switching VTs and counting down.
        let until = Instant::now() + backoff;
        while !vt::QUIT.load(Ordering::SeqCst) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let line = format!("retrying in {} s", left.as_secs() + 1);
            con.set_status(&[&args.label, &ended.why, &line]);
            con.present(None, false);
            let mut sink = vec![];
            con.switches(None, &mut sink);
            con.present(None, false);
            con.wait(&[], left.min(Duration::from_millis(500)));
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
    log("stopping");
    let _ = con.disp.drop_master();
    con.disp.restore();
    con.term.restore();
    Ok(())
}

/// One RFB session, until the server goes away or the VT is told to stop.
fn session(con: &mut Console, args: &Args, mut s: Box<dyn Stream>) -> Ended {
    let mut v = Viewer::new(
        con.status.width,
        con.status.height,
        args.password.clone(),
        Limits::default(),
        args.opts,
    );
    if !con.shown {
        v.release();
    }
    con.set_status(&[&args.label, "waiting for the first frame"]);
    let mut buf = vec![0u8; 256 * 1024];
    let mut events = vec![];
    let mut last_scan = Instant::now();
    loop {
        if vt::QUIT.load(Ordering::SeqCst) {
            return Ended {
                why: "stopping".into(),
                was_ready: v.ready(),
            };
        }
        let mut out = vec![];
        con.switches(Some(&mut v), &mut out);
        if let Err(e) = act(con, &mut *s, out) {
            return Ended {
                why: format!("write: {e}"),
                was_ready: v.ready(),
            };
        }
        if v.take_changed() {
            con.pending = true;
        }
        con.present(Some(&v), false);
        if con.shown && last_scan.elapsed() > Duration::from_secs(2) {
            con.input.scan();
            last_scan = Instant::now();
        }
        let mut fds = vec![s.as_raw_fd()];
        if con.shown {
            fds.extend(con.input.fds());
        }
        let timeout = if con.pending {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(500)
        };
        let ready = con.wait(&fds, timeout);
        if ready[0] {
            let n = match s.read(&mut buf) {
                Ok(0) => {
                    return Ended {
                        why: "the VM closed the session".into(),
                        was_ready: v.ready(),
                    };
                }
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => 0,
                Err(e) => {
                    return Ended {
                        why: format!("read: {e}"),
                        was_ready: v.ready(),
                    };
                }
            };
            let out = match v.receive(&buf[..n]) {
                Ok(out) => out,
                Err(e) => {
                    return Ended {
                        why: format!("protocol: {e}"),
                        was_ready: v.ready(),
                    };
                }
            };
            if let Err(e) = act(con, &mut *s, out) {
                return Ended {
                    why: format!("write: {e}"),
                    was_ready: v.ready(),
                };
            }
        }
        if con.shown {
            // Backwards: a device that is gone is dropped from the list.
            for i in (0..ready.len().saturating_sub(1)).rev() {
                if ready[i + 1] && i < con.input.len() {
                    con.input.read(i, &mut events);
                }
            }
            let mut out = vec![];
            for e in events.drain(..) {
                out.extend(v.input(e));
            }
            if let Err(e) = act(con, &mut *s, out) {
                return Ended {
                    why: format!("write: {e}"),
                    was_ready: v.ready(),
                };
            }
        }
    }
}

fn act(con: &Console, s: &mut dyn Stream, out: Vec<Action>) -> io::Result<()> {
    for a in out {
        match a {
            Action::Send(b) => s.write_all(&b)?,
            Action::Switch(n) => con.term.activate(n),
        }
    }
    Ok(())
}

/// Logs an error once, not every frame.
fn note(last: &mut String, msg: &str) {
    if last != msg {
        log(msg);
        *last = msg.to_string();
    }
}
