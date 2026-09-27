//! The long suite: the night window as waves (test-standard.md "Overnight
//! soaks"). stormrfb has no pods or VMs of its own, so a wave is concurrent
//! RFB sessions in this pod: ramp to a count sized from the pod's CPUs and
//! memory, hold (every session streams 60 1280×720 frames, each checked
//! exact), drain (join every session and socket), repeat with the size
//! varied, until `STORM_TIMEOUT` nearly runs out.
//!
//! Measured per wave: median ms/frame, and what the drained process still
//! holds (RSS, threads, file descriptors). A wave slower than the first wave
//! of its size, or residue above what the first drain left, fails that wave
//! even when every session passed. Trend lines go to
//! `/results/waves.jsonl`; the last test names the first wave that regressed.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::env::{self, Env, Residue};
use crate::report::{Outcome, Report};
use crate::suites::{SESSION_BYTES, median, parallel, sessions_for};

/// Sessions per CPU, cycled: the waves' varying size.
const SIZES: [usize; 4] = [1, 2, 3, 2];
const FRAMES: u32 = 60;
/// Slower than 1.5× the first wave of the same size, and by more than
/// half a millisecond (below that it is scheduler noise).
const SLOWER: f64 = 1.5;
const NOISE_MS: f64 = 0.5;

pub fn run(env: &Env, r: &mut Report) {
    let cap = env::capacity();
    let start = Instant::now();
    let margin = (env.timeout / 10).min(Duration::from_secs(300));
    let window = env.timeout.saturating_sub(margin);
    let mut firsts: HashMap<usize, f64> = HashMap::new();
    let mut base: Option<Residue> = None;
    let mut first_regressed: Option<String> = None;
    let mut last_wall = Duration::ZERO;
    let mut wave = 0u32;
    loop {
        // Stop when another wave like the last would not fit the window.
        if wave > 0 && start.elapsed() + last_wall * 3 / 2 > window {
            break;
        }
        wave += 1;
        let per_cpu = SIZES[(wave as usize - 1) % SIZES.len()];
        let n = sessions_for(&cap, per_cpu, SESSION_BYTES);
        let began = Instant::now();
        let result = parallel(n, FRAMES, wave * 1000);
        last_wall = began.elapsed();
        let after = env::residue();
        let name = format!("wave-{wave:03}");
        let mut regressed = None;
        let outcome = match result {
            Err(e) => Outcome::Fail(format!("{n} sessions: {e}")),
            Ok(ms) => {
                let m = median(ms);
                let first = *firsts.entry(per_cpu).or_insert(m);
                let base = *base.get_or_insert(after);
                let mut why = vec![];
                if m > first * SLOWER && m - first > NOISE_MS {
                    why.push(format!("{m:.3} ms/frame vs {first:.3} in the first wave of {n}"));
                }
                // The allocator may keep some of a wave; growth past a
                // quarter of the first drain (at least 64 MiB) is a leak.
                let slack = (base.rss_kb / 4).max(64 * 1024);
                if after.rss_kb > base.rss_kb + slack {
                    why.push(format!("RSS {} KiB after drain vs {} KiB", after.rss_kb, base.rss_kb));
                }
                if after.threads > base.threads {
                    why.push(format!("{} threads left vs {}", after.threads, base.threads));
                }
                if after.fds > base.fds {
                    why.push(format!("{} fds left vs {}", after.fds, base.fds));
                }
                let line = format!(
                    "{n} sessions ({per_cpu}/CPU), {FRAMES} frames each, all exact; median {m:.3} ms/frame; wall {:.1} s; after drain RSS {} KiB, {} threads, {} fds",
                    last_wall.as_secs_f64(),
                    after.rss_kb,
                    after.threads,
                    after.fds
                );
                r.artifact(
                    "waves.jsonl",
                    &format!(
                        "{{\"wave\":{wave},\"sessions\":{n},\"per_cpu\":{per_cpu},\"ms_per_frame\":{m:.4},\"wall_ms\":{},\"rss_kb\":{},\"threads\":{},\"fds\":{}}}",
                        last_wall.as_millis(),
                        after.rss_kb,
                        after.threads,
                        after.fds
                    ),
                );
                if why.is_empty() {
                    Outcome::Pass(line)
                } else {
                    let why = why.join("; ");
                    regressed = Some(why.clone());
                    Outcome::Fail(format!("regressed: {why}. {line}"))
                }
            }
        };
        if first_regressed.is_none()
            && let Some(why) = regressed
        {
            first_regressed = Some(format!("{name}: {why}"));
        }
        r.record(&name, last_wall.as_millis(), outcome);
    }
    r.run("waves-trend", || match &first_regressed {
        None => Outcome::Pass(format!(
            "{wave} waves in {:.0} s on {} CPUs / {} MiB; none slower than its first, no residue growth",
            start.elapsed().as_secs_f64(),
            cap.cpus,
            cap.memory >> 20
        )),
        Some(first) => Outcome::Fail(format!("first regression at {first}")),
    });
}
