//! What the runner hands the container, and what the pod may use.
//!
//! Capacity is read from the pod's own cgroup and `/proc`, never assumed:
//! the fleet is mixed hardware.
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    Short,
    Medium,
    Long,
}

pub struct Env {
    pub suite: Suite,
    /// `STORM_TIMEOUT`: the whole run's budget.
    pub timeout: Duration,
    /// `STORMRFB_TARGET=host:port`: a real RFB 3.8 server to read. Optional.
    pub target: Option<String>,
    /// `STORMRFB_PASSWORD`: its VNC password, if it has one.
    pub password: Option<Vec<u8>>,
}

impl Env {
    pub fn read(arg: Option<&str>) -> Option<Self> {
        let name = arg
            .map(str::to_string)
            .or_else(|| std::env::var("STORM_SUITE").ok())?;
        let suite = match name.as_str() {
            "short" => Suite::Short,
            "medium" => Suite::Medium,
            "long" => Suite::Long,
            _ => return None,
        };
        let default = match suite {
            Suite::Short => 120,
            Suite::Medium => 1800,
            Suite::Long => 8 * 3600,
        };
        let seconds = std::env::var("STORM_TIMEOUT")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default);
        Some(Self {
            suite,
            timeout: Duration::from_secs(seconds),
            target: std::env::var("STORMRFB_TARGET")
                .ok()
                .filter(|v| !v.is_empty()),
            password: std::env::var("STORMRFB_PASSWORD")
                .ok()
                .filter(|v| !v.is_empty())
                .map(String::into_bytes),
        })
    }
}

pub struct Capacity {
    pub cpus: usize,
    /// Bytes this pod may use: the smaller of its cgroup limit and what the
    /// node has available.
    pub memory: u64,
}

pub fn capacity() -> Capacity {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let memory = [cgroup_memory(), mem_available()]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(1 << 30);
    Capacity { cpus, memory }
}

fn cgroup_memory() -> Option<u64> {
    for path in [
        "/sys/fs/cgroup/memory.max",
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
    ] {
        if let Ok(v) = std::fs::read_to_string(path) {
            // cgroup v1 reports "no limit" as a number near u64::MAX.
            return v.trim().parse().ok().filter(|&n: &u64| n < 1 << 60);
        }
    }
    None
}

fn mem_available() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// What the process holds: what a drained wave must give back.
#[derive(Debug, Clone, Copy, Default)]
pub struct Residue {
    pub rss_kb: u64,
    pub threads: u64,
    pub fds: u64,
}

pub fn residue() -> Residue {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    Residue {
        rss_kb: field("VmRSS:"),
        threads: field("Threads:"),
        fds: std::fs::read_dir("/proc/self/fd").map_or(0, |d| d.count() as u64),
    }
}
