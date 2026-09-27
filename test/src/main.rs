//! stormrfb-test: stormrfb's test container, per stormcentral
//! `docs/test-standard.md` (#8). Run as `/test short|medium|long`.
//!
//! stormrfb is a library with nothing of its own on a node: it runs inside
//! stormconsole's browser package and stormrdp. So the suites run the
//! commit's own `stormrfb-server` and `stormrfb-client` against each other
//! over real TCP on the pod's loopback, replay the recorded QEMU and TigerVNC
//! fixtures, and, only when `STORMRFB_TARGET` names one, read a real RFB
//! server. They create nothing in the cluster and use no API.
//!
//! stdout is one JSON object per test and a final summary. Exit 0 all
//! passed, 1 a test failed, 2 the suite could not run.
mod env;
mod long;
mod report;
mod session;
mod suites;

use std::process::ExitCode;

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    let Some(env) = env::Env::read(arg.as_deref()) else {
        eprintln!("usage: /test short|medium|long");
        return ExitCode::from(2);
    };
    let mut report = report::Report::new();
    if let Err(e) = std::net::TcpListener::bind("127.0.0.1:0") {
        report.run("loopback", || {
            report::Outcome::Infra(format!("cannot bind 127.0.0.1: {e}"))
        });
        return report.finish();
    }
    match env.suite {
        env::Suite::Short => suites::short(&env, &mut report),
        env::Suite::Medium => suites::medium(&env, &mut report),
        env::Suite::Long => long::run(&env, &mut report),
    }
    report.finish()
}
