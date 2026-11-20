//! `dial` (stormrfb#36, d250b8cc030c / 71f1468e10d4): a stalled connect
//! never holds the caller past its deadline, and the caller keeps control
//! while it runs; a real connect to a local listener comes through.

use std::io;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use stormrfb_vt::dial::{self, Dial};

#[test]
fn a_stalled_connect_gives_up_at_its_deadline() {
    // A resolver or SYN that never answers, as a closure that sleeps.
    let start = Instant::now();
    let d: Dial<()> = Dial::start(Duration::from_millis(200), || {
        std::thread::sleep(Duration::from_secs(30));
        Ok(())
    })
    .unwrap();
    let mut polls = 0;
    let r = loop {
        if let Some(r) = d.poll(Instant::now()) {
            break r;
        }
        // The caller's turn: the console answers VT switches here.
        polls += 1;
        std::thread::sleep(Duration::from_millis(10));
    };
    let took = start.elapsed();
    assert_eq!(r.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(polls >= 5, "the caller got {polls} turns");
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert!(took >= Duration::from_millis(200), "{took:?}");
}

#[test]
fn poll_never_blocks() {
    let d: Dial<()> = Dial::start(Duration::from_secs(60), || {
        std::thread::sleep(Duration::from_secs(30));
        Ok(())
    })
    .unwrap();
    let t = Instant::now();
    for _ in 0..100 {
        assert!(d.poll(Instant::now()).is_none());
    }
    assert!(t.elapsed() < Duration::from_millis(500));
    assert!(d.left(Instant::now()) > Duration::from_secs(50));
}

#[test]
fn a_result_comes_back() {
    let d = Dial::start(Duration::from_secs(5), || Ok(42)).unwrap();
    let r = loop {
        if let Some(r) = d.poll(Instant::now()) {
            break r;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(r.unwrap(), 42);
}

#[test]
fn tcp_connects_to_a_listener() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let d = Dial::start(Duration::from_secs(5), move || {
        dial::tcp(&addr, Duration::from_secs(1))
    })
    .unwrap();
    let s = loop {
        if let Some(r) = d.poll(Instant::now()) {
            break r.unwrap();
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let (_peer, from) = l.accept().unwrap();
    assert_eq!(s.local_addr().unwrap(), from);
}

#[test]
fn tcp_refused_is_an_error_not_a_hang() {
    // Bind then drop: the port is free and nothing listens on it.
    let addr = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    };
    let t = Instant::now();
    let e = dial::tcp(&addr, Duration::from_secs(1)).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::ConnectionRefused);
    assert!(t.elapsed() < Duration::from_secs(1));
}

#[test]
fn tcp_bad_address_is_an_error() {
    assert!(dial::tcp("no-port-here", Duration::from_secs(1)).is_err());
}
