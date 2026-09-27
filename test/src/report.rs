//! The standard's output: one JSON object per test on stdout, a summary
//! last, the same lines in `/results/results.jsonl`, and the exit code.
use std::fs::File;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::process::ExitCode;
use std::time::Instant;

pub enum Outcome {
    Pass(String),
    Fail(String),
    Skip(String),
    /// The test could not run: the suite exits 2, never a pass.
    Infra(String),
}

pub struct Report {
    pass: u32,
    fail: u32,
    skip: u32,
    infra: bool,
    results: Option<File>,
}

impl Report {
    pub fn new() -> Self {
        let dir = std::env::var("STORMRFB_RESULTS").unwrap_or_else(|_| "/results".into());
        let results = std::fs::create_dir_all(&dir)
            .ok()
            .and_then(|_| File::create(format!("{dir}/results.jsonl")).ok());
        Self {
            pass: 0,
            fail: 0,
            skip: 0,
            infra: false,
            results,
        }
    }

    /// Run one test, timing it. A panic is that test's failure, not the suite's end.
    pub fn run(&mut self, name: &str, test: impl FnOnce() -> Outcome) -> bool {
        let start = Instant::now();
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(test)).unwrap_or_else(|p| {
            let why = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            Outcome::Fail(format!("panicked: {why}"))
        });
        self.record(name, start.elapsed().as_millis(), outcome)
    }

    /// Record a test that was timed by its caller.
    pub fn record(&mut self, name: &str, ms: u128, outcome: Outcome) -> bool {
        let (status, detail, passed) = match outcome {
            Outcome::Pass(d) => {
                self.pass += 1;
                ("pass", d, true)
            }
            Outcome::Fail(d) => {
                self.fail += 1;
                ("fail", d, false)
            }
            Outcome::Skip(d) => {
                self.skip += 1;
                ("skip", d, false)
            }
            Outcome::Infra(d) => {
                self.fail += 1;
                self.infra = true;
                ("fail", format!("could not run: {d}"), false)
            }
        };
        self.emit(&format!(
            "{{\"test\":{},\"status\":\"{status}\",\"ms\":{ms},\"detail\":{}}}",
            json(name),
            json(&detail)
        ));
        passed
    }

    /// A line for `/results` only (trend data), not a test.
    pub fn artifact(&mut self, name: &str, line: &str) {
        let dir = std::env::var("STORMRFB_RESULTS").unwrap_or_else(|_| "/results".into());
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{dir}/{name}"))
        {
            let _ = writeln!(f, "{line}");
        }
    }

    fn emit(&mut self, line: &str) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
        if let Some(f) = &mut self.results {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn finish(mut self) -> ExitCode {
        let summary = format!(
            "{{\"summary\":{{\"pass\":{},\"fail\":{},\"skip\":{}}}}}",
            self.pass, self.fail, self.skip
        );
        self.emit(&summary);
        ExitCode::from(if self.infra {
            2
        } else if self.fail > 0 {
            1
        } else {
            0
        })
    }
}

pub fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
