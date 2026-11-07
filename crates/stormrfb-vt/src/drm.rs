//! KMS output through the kernel's DRM ioctls only: find a connected
//! connector and its preferred mode, make two dumb XRGB8888 buffers, set the
//! CRTC, and present each frame by page flip. A driver that cannot flip
//! (simpledrm, the X9 blades' firmware framebuffer) gets the frame copied into
//! the scanout buffer and marked dirty instead.
//!
//! Adapted from stormpanel's `src/drm.rs` (glennswest/stormpanel, MIT), which
//! draws tty8 the same way; here a frame can carry an overlay (the cursor).

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::time::Duration;

use crate::canvas::Canvas;

const fn iowr(nr: u32, size: usize) -> libc::c_ulong {
    ((3u32 << 30) | ((size as u32) << 16) | ((b'd' as u32) << 8) | nr) as libc::c_ulong
}
const fn io_(nr: u32) -> libc::c_ulong {
    (((b'd' as u32) << 8) | nr) as libc::c_ulong
}

#[repr(C)]
#[derive(Default)]
struct CardRes {
    fb_id_ptr: u64,
    crtc_id_ptr: u64,
    connector_id_ptr: u64,
    encoder_id_ptr: u64,
    count_fbs: u32,
    count_crtcs: u32,
    count_connectors: u32,
    count_encoders: u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ModeInfo {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    pub name: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
struct GetConnector {
    encoders_ptr: u64,
    modes_ptr: u64,
    props_ptr: u64,
    prop_values_ptr: u64,
    count_modes: u32,
    count_props: u32,
    count_encoders: u32,
    encoder_id: u32,
    connector_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: u32,
    mm_width: u32,
    mm_height: u32,
    subpixel: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Default)]
struct GetEncoder {
    encoder_id: u32,
    encoder_type: u32,
    crtc_id: u32,
    possible_crtcs: u32,
    possible_clones: u32,
}

#[repr(C)]
#[derive(Default)]
struct Crtc {
    set_connectors_ptr: u64,
    count_connectors: u32,
    crtc_id: u32,
    fb_id: u32,
    x: u32,
    y: u32,
    gamma_size: u32,
    mode_valid: u32,
    mode: ModeInfo,
}

#[repr(C)]
#[derive(Default)]
struct CreateDumb {
    height: u32,
    width: u32,
    bpp: u32,
    flags: u32,
    handle: u32,
    pitch: u32,
    size: u64,
}

#[repr(C)]
#[derive(Default)]
struct MapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

#[repr(C)]
#[derive(Default)]
struct FbCmd {
    fb_id: u32,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u32,
    depth: u32,
    handle: u32,
}

#[repr(C)]
#[derive(Default)]
struct PageFlip {
    crtc_id: u32,
    fb_id: u32,
    flags: u32,
    reserved: u32,
    user_data: u64,
}

#[repr(C)]
#[derive(Default)]
struct DirtyFb {
    fb_id: u32,
    flags: u32,
    color: u32,
    num_clips: u32,
    clips_ptr: u64,
}

const GETRESOURCES: libc::c_ulong = iowr(0xA0, std::mem::size_of::<CardRes>());
const GETCRTC: libc::c_ulong = iowr(0xA1, std::mem::size_of::<Crtc>());
const SETCRTC: libc::c_ulong = iowr(0xA2, std::mem::size_of::<Crtc>());
const GETENCODER: libc::c_ulong = iowr(0xA6, std::mem::size_of::<GetEncoder>());
const GETCONNECTOR: libc::c_ulong = iowr(0xA7, std::mem::size_of::<GetConnector>());
const ADDFB: libc::c_ulong = iowr(0xAE, std::mem::size_of::<FbCmd>());
const RMFB: libc::c_ulong = iowr(0xAF, std::mem::size_of::<u32>());
const PAGE_FLIP: libc::c_ulong = iowr(0xB0, std::mem::size_of::<PageFlip>());
const DIRTYFB: libc::c_ulong = iowr(0xB1, std::mem::size_of::<DirtyFb>());
const CREATE_DUMB: libc::c_ulong = iowr(0xB2, std::mem::size_of::<CreateDumb>());
const MAP_DUMB: libc::c_ulong = iowr(0xB3, std::mem::size_of::<MapDumb>());
const DESTROY_DUMB: libc::c_ulong = iowr(0xB4, std::mem::size_of::<u32>());
const SET_MASTER: libc::c_ulong = io_(0x1e);
const DROP_MASTER: libc::c_ulong = io_(0x1f);

const MODE_TYPE_PREFERRED: u32 = 1 << 3;
const PAGE_FLIP_EVENT: u32 = 0x01;
const CONNECTED: u32 = 1;

fn ioctl<T>(fd: i32, req: libc::c_ulong, arg: &mut T) -> io::Result<()> {
    loop {
        let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) | Some(libc::EAGAIN) => continue,
            _ => return Err(e),
        }
    }
}

struct Buffer {
    handle: u32,
    fb: u32,
    pitch: usize,
    size: usize,
    map: *mut u8,
}

pub struct Display {
    file: File,
    pub width: usize,
    pub height: usize,
    pub mode: ModeInfo,
    crtc: u32,
    connector: u32,
    bufs: Vec<Buffer>,
    front: usize,
    /// Page flips work on this driver (cleared on the first refusal).
    flips: bool,
    /// A flip has been queued and its event not yet read.
    pending: bool,
    saved: Option<Crtc>,
}

impl Display {
    /// Opens `path` (or the first /dev/dri/card* with a connected screen).
    pub fn open(path: Option<&str>) -> io::Result<Display> {
        let cards: Vec<String> = match path {
            Some(p) => vec![p.to_string()],
            None => {
                let mut v: Vec<String> = std::fs::read_dir("/dev/dri")?
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("card"))
                    .map(|n| format!("/dev/dri/{n}"))
                    .collect();
                v.sort();
                v
            }
        };
        let mut last = io::Error::new(
            io::ErrorKind::NotFound,
            "no /dev/dri/card* with a connected screen",
        );
        for c in cards {
            match Display::open_card(&c) {
                Ok(d) => return Ok(d),
                Err(e) => last = io::Error::new(e.kind(), format!("{c}: {e}")),
            }
        }
        Err(last)
    }

    fn open_card(path: &str) -> io::Result<Display> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags_cloexec()
            .open(path)?;
        let fd = file.as_raw_fd();
        let mut res = CardRes::default();
        ioctl(fd, GETRESOURCES, &mut res)?;
        let mut crtcs = vec![0u32; res.count_crtcs as usize];
        let mut conns = vec![0u32; res.count_connectors as usize];
        let mut encs = vec![0u32; res.count_encoders as usize];
        let mut fbs = vec![0u32; res.count_fbs as usize];
        let mut res2 = CardRes {
            fb_id_ptr: fbs.as_mut_ptr() as u64,
            crtc_id_ptr: crtcs.as_mut_ptr() as u64,
            connector_id_ptr: conns.as_mut_ptr() as u64,
            encoder_id_ptr: encs.as_mut_ptr() as u64,
            count_fbs: fbs.len() as u32,
            count_crtcs: crtcs.len() as u32,
            count_connectors: conns.len() as u32,
            count_encoders: encs.len() as u32,
            ..Default::default()
        };
        ioctl(fd, GETRESOURCES, &mut res2)?;
        for &cid in conns.iter().take(res2.count_connectors as usize) {
            let Some((mode, enc_ids, cur_enc)) = connector(fd, cid)? else {
                continue;
            };
            let Some(crtc) = pick_crtc(fd, cur_enc, &enc_ids, &crtcs) else {
                continue;
            };
            let mut saved = Crtc {
                crtc_id: crtc,
                ..Default::default()
            };
            let saved = ioctl(fd, GETCRTC, &mut saved).ok().map(|_| saved);
            let mut d = Display {
                file,
                width: mode.hdisplay as usize,
                height: mode.vdisplay as usize,
                mode,
                crtc,
                connector: cid,
                bufs: vec![],
                front: 0,
                flips: true,
                pending: false,
                saved,
            };
            for _ in 0..2 {
                let b = d.create_buffer()?;
                d.bufs.push(b);
            }
            return Ok(d);
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no connected connector with a mode",
        ))
    }

    fn create_buffer(&self) -> io::Result<Buffer> {
        let fd = self.file.as_raw_fd();
        let mut c = CreateDumb {
            width: self.width as u32,
            height: self.height as u32,
            bpp: 32,
            ..Default::default()
        };
        ioctl(fd, CREATE_DUMB, &mut c)?;
        let mut fb = FbCmd {
            width: c.width,
            height: c.height,
            pitch: c.pitch,
            bpp: 32,
            depth: 24,
            handle: c.handle,
            ..Default::default()
        };
        ioctl(fd, ADDFB, &mut fb)?;
        let mut m = MapDumb {
            handle: c.handle,
            ..Default::default()
        };
        ioctl(fd, MAP_DUMB, &mut m)?;
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                c.size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                m.offset as libc::off_t,
            )
        };
        if map == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Buffer {
            handle: c.handle,
            fb: fb.fb_id,
            pitch: c.pitch as usize,
            size: c.size as usize,
            map: map as *mut u8,
        })
    }

    pub fn set_master(&self) -> io::Result<()> {
        let mut z = 0u32;
        ioctl(self.file.as_raw_fd(), SET_MASTER, &mut z)
    }

    pub fn drop_master(&self) -> io::Result<()> {
        let mut z = 0u32;
        ioctl(self.file.as_raw_fd(), DROP_MASTER, &mut z)
    }

    /// Shows the front buffer (after acquiring the VT, or at start).
    pub fn modeset(&mut self) -> io::Result<()> {
        self.pending = false;
        let fb = self.bufs[self.front].fb;
        self.set_crtc(fb)
    }

    fn set_crtc(&self, fb: u32) -> io::Result<()> {
        let mut conn = self.connector;
        let mut c = Crtc {
            set_connectors_ptr: &mut conn as *mut u32 as u64,
            count_connectors: 1,
            crtc_id: self.crtc,
            fb_id: fb,
            mode_valid: 1,
            mode: self.mode,
            ..Default::default()
        };
        ioctl(self.file.as_raw_fd(), SETCRTC, &mut c)
    }

    fn copy_into(&self, i: usize, c: &Canvas, overlay: &dyn Fn(&mut [u32], usize)) {
        let b = &self.bufs[i];
        let w = self.width.min(c.width);
        for y in 0..self.height.min(c.height) {
            let src = &c.pixels[y * c.width..y * c.width + w];
            let off = y * b.pitch;
            if off + w * 4 > b.size {
                break;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr() as *const u8, b.map.add(off), w * 4);
            }
        }
        // SAFETY: the mapping is page-aligned, `size` bytes long and ours
        // alone; nothing else touches it while the slice lives.
        let px = unsafe { std::slice::from_raw_parts_mut(b.map as *mut u32, b.size / 4) };
        overlay(px, b.pitch / 4);
    }

    /// Presents a frame: into the back buffer and flipped to, or (no flips)
    /// into the scanout buffer and marked dirty. `overlay` draws on top of
    /// the copied canvas, given the buffer and its stride in pixels.
    pub fn present(&mut self, c: &Canvas, overlay: &dyn Fn(&mut [u32], usize)) -> io::Result<()> {
        let fd = self.file.as_raw_fd();
        if self.pending {
            self.wait_flip(Duration::from_millis(100));
        }
        if self.flips && !self.pending {
            let back = 1 - self.front;
            self.copy_into(back, c, overlay);
            let mut f = PageFlip {
                crtc_id: self.crtc,
                fb_id: self.bufs[back].fb,
                flags: PAGE_FLIP_EVENT,
                ..Default::default()
            };
            match ioctl(fd, PAGE_FLIP, &mut f) {
                Ok(()) => {
                    self.front = back;
                    self.pending = true;
                    return Ok(());
                }
                Err(e) if e.raw_os_error() == Some(libc::EBUSY) => return Ok(()),
                Err(_) => {
                    // simpledrm and friends: no vblank, no flip. Keep the
                    // back buffer's content as the one scanned out.
                    self.flips = false;
                    self.front = back;
                    self.set_crtc(self.bufs[back].fb)?;
                }
            }
        } else if !self.flips {
            self.copy_into(self.front, c, overlay);
        }
        let mut d = DirtyFb {
            fb_id: self.bufs[self.front].fb,
            ..Default::default()
        };
        // ENOSYS: the driver scans out straight from memory; nothing to mark.
        let _ = ioctl(fd, DIRTYFB, &mut d);
        Ok(())
    }

    /// Reads the flip-complete event (or gives up after `t`).
    fn wait_flip(&mut self, t: Duration) {
        let mut p = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut p, 1, t.as_millis() as i32) } > 0 {
            let mut buf = [0u8; 256];
            unsafe {
                libc::read(
                    self.file.as_raw_fd(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
        }
        self.pending = false;
    }

    pub fn flips(&self) -> bool {
        self.flips
    }

    /// Puts back whatever the CRTC showed before (fbcon's buffer).
    pub fn restore(&self) {
        if let Some(s) = &self.saved
            && s.fb_id != 0
            && s.mode_valid != 0
        {
            let _ = self.set_crtc_raw(s);
        }
    }

    fn set_crtc_raw(&self, s: &Crtc) -> io::Result<()> {
        let mut conn = self.connector;
        let mut c = Crtc {
            set_connectors_ptr: &mut conn as *mut u32 as u64,
            count_connectors: 1,
            crtc_id: s.crtc_id,
            fb_id: s.fb_id,
            x: s.x,
            y: s.y,
            mode_valid: 1,
            mode: s.mode,
            ..Default::default()
        };
        ioctl(self.file.as_raw_fd(), SETCRTC, &mut c)
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        let fd = self.file.as_raw_fd();
        for b in &self.bufs {
            unsafe { libc::munmap(b.map as *mut libc::c_void, b.size) };
            let mut fb = b.fb;
            let _ = ioctl(fd, RMFB, &mut fb);
            let mut h = b.handle;
            let _ = ioctl(fd, DESTROY_DUMB, &mut h);
        }
    }
}

/// A connected connector's preferred mode (else its first), its encoders and
/// its current encoder.
fn connector(fd: i32, id: u32) -> io::Result<Option<(ModeInfo, Vec<u32>, u32)>> {
    let mut c = GetConnector {
        connector_id: id,
        ..Default::default()
    };
    ioctl(fd, GETCONNECTOR, &mut c)?;
    if c.connection != CONNECTED || c.count_modes == 0 {
        return Ok(None);
    }
    let mut modes = vec![ModeInfo::default(); c.count_modes as usize];
    let mut encs = vec![0u32; c.count_encoders as usize];
    let mut c2 = GetConnector {
        connector_id: id,
        modes_ptr: modes.as_mut_ptr() as u64,
        count_modes: modes.len() as u32,
        encoders_ptr: encs.as_mut_ptr() as u64,
        count_encoders: encs.len() as u32,
        ..Default::default()
    };
    ioctl(fd, GETCONNECTOR, &mut c2)?;
    let n = (c2.count_modes as usize).min(modes.len());
    let modes = &modes[..n];
    let Some(mode) = modes
        .iter()
        .find(|m| m.type_ & MODE_TYPE_PREFERRED != 0)
        .or(modes.first())
        .copied()
    else {
        return Ok(None);
    };
    encs.truncate((c2.count_encoders as usize).min(encs.len()));
    Ok(Some((mode, encs, c2.encoder_id)))
}

fn pick_crtc(fd: i32, cur_enc: u32, encs: &[u32], crtcs: &[u32]) -> Option<u32> {
    if cur_enc != 0 {
        let mut e = GetEncoder {
            encoder_id: cur_enc,
            ..Default::default()
        };
        if ioctl(fd, GETENCODER, &mut e).is_ok() && e.crtc_id != 0 {
            return Some(e.crtc_id);
        }
    }
    for &id in encs {
        let mut e = GetEncoder {
            encoder_id: id,
            ..Default::default()
        };
        if ioctl(fd, GETENCODER, &mut e).is_err() {
            continue;
        }
        for (i, &c) in crtcs.iter().enumerate() {
            if e.possible_crtcs & (1 << i) != 0 {
                return Some(c);
            }
        }
    }
    None
}

trait CloExec {
    fn custom_flags_cloexec(&mut self) -> &mut Self;
}

impl CloExec for OpenOptions {
    fn custom_flags_cloexec(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.custom_flags(libc::O_CLOEXEC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structs_have_the_kernel_layout() {
        assert_eq!(std::mem::size_of::<CardRes>(), 64);
        assert_eq!(std::mem::size_of::<ModeInfo>(), 68);
        assert_eq!(std::mem::size_of::<GetConnector>(), 80);
        assert_eq!(std::mem::size_of::<GetEncoder>(), 20);
        assert_eq!(std::mem::size_of::<Crtc>(), 104);
        assert_eq!(std::mem::size_of::<CreateDumb>(), 32);
        assert_eq!(std::mem::size_of::<MapDumb>(), 16);
        assert_eq!(std::mem::size_of::<FbCmd>(), 28);
        assert_eq!(std::mem::size_of::<PageFlip>(), 24);
        assert_eq!(std::mem::size_of::<DirtyFb>(), 24);
    }

    #[test]
    fn ioctl_numbers_match_the_uapi_headers() {
        // Values from <drm/drm.h> as compiled on x86_64.
        assert_eq!(GETRESOURCES, 0xC040_64A0);
        assert_eq!(GETCONNECTOR, 0xC050_64A7);
        assert_eq!(SETCRTC, 0xC068_64A2);
        assert_eq!(CREATE_DUMB, 0xC020_64B2);
        assert_eq!(MAP_DUMB, 0xC010_64B3);
        assert_eq!(ADDFB, 0xC01C_64AE);
        assert_eq!(PAGE_FLIP, 0xC018_64B0);
        assert_eq!(DIRTYFB, 0xC018_64B1);
        assert_eq!(SET_MASTER, 0x641E);
    }
}
