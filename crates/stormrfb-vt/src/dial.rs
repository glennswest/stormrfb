//! Connecting to the RFB server without stalling the console (stormrfb#36,
//! d250b8cc030c / 71f1468e10d4).
//!
//! Address resolution and `connect` block, and a `--connect` host that
//! drops SYNs or a resolver that stalls would hold them for minutes. While
//! the viewer holds its VT in VT_PROCESS mode, a switch away waits for it to
//! acknowledge the release, so a blocked connect is a frozen console. A
//! [`Dial`] runs the connect on a thread of its own, with a deadline, and
//! the console loop polls it between its waits, still answering VT switches
//! and SIGTERM. The thread is started after the switch and stop signals are
//! blocked, so it inherits that mask and they always reach the main loop's
//! `ppoll`. A dial given up on is left to end by itself; its stream, if it
//! ever connects, is dropped.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

/// A connect running on its own thread, with a deadline.
pub struct Dial<T> {
    rx: mpsc::Receiver<io::Result<T>>,
    deadline: Instant,
    timeout: Duration,
}

impl<T: Send + 'static> Dial<T> {
    /// Starts `f` on a thread; [`Dial::poll`] answers by `timeout` from now.
    pub fn start(
        timeout: Duration,
        f: impl FnOnce() -> io::Result<T> + Send + 'static,
    ) -> io::Result<Dial<T>> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("dial".into())
            .spawn(move || {
                let _ = tx.send(f());
            })?;
        Ok(Dial {
            rx,
            deadline: Instant::now() + timeout,
            timeout,
        })
    }

    /// `None` while it is still connecting; the result once it is done, or
    /// a `TimedOut` error once the deadline has passed.
    pub fn poll(&self, now: Instant) -> Option<io::Result<T>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) if now >= self.deadline => Some(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no connection in {} s", self.timeout.as_secs_f32()),
            ))),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("the connecting thread ended")))
            }
        }
    }

    /// Time left until the deadline.
    pub fn left(&self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }
}

/// Resolves `addr` (`HOST:PORT`) and connects to each address in turn,
/// giving each `each`.
pub fn tcp(addr: &str, each: Duration) -> io::Result<TcpStream> {
    let mut last = io::Error::new(
        io::ErrorKind::NotFound,
        format!("{addr}: no address to connect to"),
    );
    for a in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&a, each) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}
