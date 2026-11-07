//! The console's keyboards and pointers, read straight from
//! `/dev/input/event*`. Only devices that are a keyboard (they have letter
//! keys) or a pointer (a button and an X axis) are opened, so the node's
//! power button and lid switch never reach the guest.
//!
//! They are opened when the VT is acquired and closed when it is released,
//! so nothing typed on another VT is ever replayed into the guest, and a
//! device plugged in meanwhile (the SMKVM's virtual keyboard) is found.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::viewer::Input;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_HWHEEL: u16 = 6;
const REL_WHEEL: u16 = 8;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const KEY_A: usize = 30;
const KEY_ENTER: usize = 28;
const KEY_MAX: usize = 0x2ff;

/// `struct input_event` on a 64-bit kernel: a timeval, then type, code, value.
const EVENT_SIZE: usize = 24;

/// `EVIOCGBIT(ev, len)`: the capability bits of event type `ev`.
const fn eviocgbit(ev: u32, len: u32) -> libc::c_ulong {
    ((2u32 << 30) | (len << 16) | ((b'E' as u32) << 8) | (0x20 + ev)) as libc::c_ulong
}

/// `EVIOCGABS(axis)`: `struct input_absinfo` (six i32s) of an axis.
const fn eviocgabs(axis: u32) -> libc::c_ulong {
    ((2u32 << 30) | (24 << 16) | ((b'E' as u32) << 8) | (0x40 + axis)) as libc::c_ulong
}

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

struct Device {
    file: File,
    path: PathBuf,
    /// ABS_X and ABS_Y ranges, for an absolute pointer (a tablet, a KVM).
    abs: [Option<(i32, i32)>; 2],
    /// Bytes of a partly read event.
    partial: Vec<u8>,
}

#[derive(Default)]
pub struct Devices {
    list: Vec<Device>,
}

fn bits(fd: i32, ev: u32, n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n.div_ceil(8)];
    let r = unsafe { libc::ioctl(fd, eviocgbit(ev, b.len() as u32) as _, b.as_mut_ptr()) };
    if r < 0 {
        b.fill(0);
    }
    b
}

fn has(b: &[u8], bit: usize) -> bool {
    b.get(bit / 8).is_some_and(|v| v & (1 << (bit % 8)) != 0)
}

fn range(fd: i32, axis: u32) -> Option<(i32, i32)> {
    let mut a = AbsInfo::default();
    let r = unsafe { libc::ioctl(fd, eviocgabs(axis) as _, &mut a as *mut AbsInfo) };
    (r == 0 && a.maximum > a.minimum).then_some((a.minimum, a.maximum))
}

impl Device {
    /// The device at `path`, if it is a keyboard or a pointer.
    fn open(path: &Path) -> Option<Device> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .ok()?;
        let fd = file.as_raw_fd();
        let types = bits(fd, 0, 32);
        let keys = bits(fd, u32::from(EV_KEY), KEY_MAX + 1);
        let keyboard = has(&types, EV_KEY.into()) && has(&keys, KEY_A) && has(&keys, KEY_ENTER);
        let button = [0x110, 0x14a].iter().any(|&b| has(&keys, b));
        let rel = has(&types, EV_REL.into()) && has(&bits(fd, EV_REL.into(), 16), REL_X.into());
        let abs = has(&types, EV_ABS.into()) && has(&bits(fd, EV_ABS.into(), 64), ABS_X.into());
        if !keyboard && !(button && (rel || abs)) {
            return None;
        }
        let abs = if abs {
            [range(fd, ABS_X.into()), range(fd, ABS_Y.into())]
        } else {
            [None, None]
        };
        Some(Device {
            file,
            path: path.to_path_buf(),
            abs,
            partial: vec![],
        })
    }

    /// Everything readable now. `Err` means the device is gone.
    fn read(&mut self, out: &mut Vec<Input>) -> io::Result<()> {
        let mut buf = [0u8; EVENT_SIZE * 64];
        loop {
            let n = match self.file.read(&mut buf) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            self.partial.extend_from_slice(&buf[..n]);
            let whole = self.partial.len() / EVENT_SIZE * EVENT_SIZE;
            for ev in self.partial[..whole].chunks_exact(EVENT_SIZE) {
                let kind = u16::from_ne_bytes([ev[16], ev[17]]);
                let code = u16::from_ne_bytes([ev[18], ev[19]]);
                let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                out.extend(decode(kind, code, value, &self.abs));
            }
            self.partial.drain(..whole);
        }
    }
}

/// One `input_event` as the viewer takes it.
fn decode(kind: u16, code: u16, value: i32, abs: &[Option<(i32, i32)>; 2]) -> Option<Input> {
    let norm = |axis: usize| {
        let (lo, hi) = abs[axis]?;
        Some(f64::from(value - lo) / f64::from(hi - lo))
    };
    match (kind, code) {
        (EV_SYN, SYN_REPORT) => Some(Input::Sync),
        (EV_KEY, _) => Some(Input::Key { code, value }),
        (EV_REL, REL_X) => Some(Input::RelX(value)),
        (EV_REL, REL_Y) => Some(Input::RelY(value)),
        (EV_REL, REL_WHEEL) => Some(Input::Wheel(value)),
        (EV_REL, REL_HWHEEL) => Some(Input::HWheel(value)),
        (EV_ABS, ABS_X) => norm(0).map(Input::AbsX),
        (EV_ABS, ABS_Y) => norm(1).map(Input::AbsY),
        _ => None,
    }
}

impl Devices {
    /// Opens every keyboard and pointer not open yet.
    pub fn scan(&mut self) {
        let Ok(dir) = std::fs::read_dir("/dev/input") else {
            return;
        };
        let mut paths: Vec<PathBuf> = dir
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("event"))
            })
            .collect();
        paths.sort();
        for p in paths {
            if self.list.iter().all(|d| d.path != p)
                && let Some(d) = Device::open(&p)
            {
                self.list.push(d);
            }
        }
    }

    pub fn close(&mut self) {
        self.list.clear();
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The devices' fds, in the order `read` takes indexes.
    pub fn fds(&self) -> Vec<i32> {
        self.list.iter().map(|d| d.file.as_raw_fd()).collect()
    }

    /// Reads device `i`; one that is gone is closed and dropped.
    pub fn read(&mut self, i: usize, out: &mut Vec<Input>) {
        if let Some(d) = self.list.get_mut(i)
            && d.read(out).is_err()
        {
            self.list.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_the_uapi_headers() {
        // <linux/input.h> on x86_64.
        assert_eq!(eviocgbit(0, 4), 0x8004_4520);
        assert_eq!(eviocgbit(1, 96), 0x8060_4521);
        assert_eq!(eviocgabs(0), 0x8018_4540);
        assert_eq!(std::mem::size_of::<AbsInfo>(), 24);
    }

    #[test]
    fn events_decode_with_absolute_axes_normalised() {
        let abs = [Some((0, 32767)), Some((0, 32767))];
        assert_eq!(decode(0, 0, 0, &abs), Some(Input::Sync));
        assert_eq!(
            decode(1, 30, 1, &abs),
            Some(Input::Key { code: 30, value: 1 })
        );
        assert_eq!(decode(2, 0, -3, &abs), Some(Input::RelX(-3)));
        assert_eq!(decode(2, 8, 1, &abs), Some(Input::Wheel(1)));
        assert_eq!(decode(3, 0, 32767, &abs), Some(Input::AbsX(1.0)));
        assert_eq!(decode(3, 1, 0, &abs), Some(Input::AbsY(0.0)));
        // An absolute axis with no range, and events the viewer ignores.
        assert_eq!(decode(3, 0, 5, &[None, None]), None);
        assert_eq!(decode(4, 4, 1, &abs), None);
    }
}
