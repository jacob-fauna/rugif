use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use error_stack::{Result, ResultExt};
use rugif_core::{CaptureFrame, Region};
use x11rb::connection::Connection;
use x11rb::protocol::shm;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, Screen};
use x11rb::rust_connection::RustConnection;

use crate::{CaptureError, CaptureReceiver, ScreenCapture};

/// Check if X11 SHM extension is available.
fn shm_available(conn: &RustConnection) -> bool {
    shm::query_version(conn)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some()
}

/// Shared memory segment wrapper for X11 SHM capture.
struct ShmSegment {
    conn: Arc<RustConnection>,
    addr: *mut u8,
    size: usize,
    seg_id: u32,
}

unsafe impl Send for ShmSegment {}

impl ShmSegment {
    fn new(conn: &Arc<RustConnection>, size: usize) -> Result<Self, CaptureError> {
        unsafe {
            let shmid = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o600);
            if shmid < 0 {
                return Err(error_stack::report!(CaptureError::Screenshot(
                    "shmget failed".into()
                )));
            }

            let addr = libc::shmat(shmid, std::ptr::null(), 0) as *mut u8;
            if addr == libc::MAP_FAILED as *mut u8 {
                libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut());
                return Err(error_stack::report!(CaptureError::Screenshot(
                    "shmat failed".into()
                )));
            }

            let seg_id = conn
                .generate_id()
                .change_context(CaptureError::Screenshot("generate_id failed".into()))?;

            shm::attach(&**conn, seg_id, shmid as u32, false)
                .change_context(CaptureError::Screenshot("shm attach failed".into()))?
                .check()
                .change_context(CaptureError::Screenshot("shm attach check failed".into()))?;

            // Mark for removal once all processes detach
            libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut());

            Ok(Self {
                conn: conn.clone(),
                addr,
                size,
                seg_id,
            })
        }
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.addr, self.size) }
    }
}

impl Drop for ShmSegment {
    fn drop(&mut self) {
        // Detach from X server first
        let _ = shm::detach(&*self.conn, self.seg_id);
        let _ = self.conn.flush();
        unsafe {
            libc::shmdt(self.addr as *const _);
        }
    }
}

/// Grab a region using SHM, returning RGBA data.
fn grab_shm(
    conn: &RustConnection,
    root: u32,
    shm_seg: &ShmSegment,
    region: Region,
    width: u32,
    height: u32,
    depth: u8,
) -> Result<CaptureFrame, CaptureError> {
    shm::get_image(
        conn,
        root,
        region.x as i16,
        region.y as i16,
        width as u16,
        height as u16,
        !0,
        ImageFormat::Z_PIXMAP.into(),
        shm_seg.seg_id,
        0,
    )
    .change_context(CaptureError::Screenshot("shm get_image request failed".into()))?
    .reply()
    .change_context(CaptureError::Screenshot("shm get_image reply failed".into()))?;

    let bpp = if depth >= 24 { 4 } else { depth as u32 / 8 };
    let raw = &shm_seg.as_slice()[..(width * height * bpp) as usize];
    let data = convert_to_rgba(raw, depth);

    Ok(CaptureFrame {
        data,
        width,
        height,
        timestamp: Instant::now(),
    })
}

/// Grab a region using GetImage (no SHM, slower but always works).
fn grab_getimage(
    conn: &RustConnection,
    root: u32,
    region: Region,
    width: u32,
    height: u32,
    depth: u8,
) -> Result<CaptureFrame, CaptureError> {
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            region.x as i16,
            region.y as i16,
            width as u16,
            height as u16,
            !0,
        )
        .change_context(CaptureError::Screenshot("get_image request failed".into()))?
        .reply()
        .change_context(CaptureError::Screenshot("get_image reply failed".into()))?;

    let data = convert_to_rgba(&reply.data, depth);

    Ok(CaptureFrame {
        data,
        width,
        height,
        timestamp: Instant::now(),
    })
}

/// Convert X11 pixel data to RGBA based on depth.
/// X11 ZPixmap at depth 24/32 is typically BGRX/BGRA (little-endian u32).
fn convert_to_rgba(raw: &[u8], depth: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(raw.len());

    if depth >= 24 {
        // 4 bytes per pixel: B, G, R, X/A (little-endian BGRX or BGRA)
        for pixel in raw.chunks_exact(4) {
            data.push(pixel[2]); // R
            data.push(pixel[1]); // G
            data.push(pixel[0]); // B
            data.push(255);      // A (force opaque — X padding byte is often 0)
        }
    } else {
        // Rare: 16-bit or other depths — just fill white as fallback
        tracing::warn!("unsupported X11 depth {depth}, frames may look wrong");
        for pixel in raw.chunks_exact(2) {
            let val = u16::from_le_bytes([pixel[0], pixel[1]]);
            let r = ((val >> 11) & 0x1F) as u8 * 8;
            let g = ((val >> 5) & 0x3F) as u8 * 4;
            let b = (val & 0x1F) as u8 * 8;
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }

    data
}

/// X11 screen capture backend using SHM for fast frame grabs,
/// with GetImage fallback.
pub struct X11Capture {
    conn: Arc<RustConnection>,
    screen: Screen,
    use_shm: bool,
    stop_flag: Option<Arc<AtomicBool>>,
    capture_thread: Option<thread::JoinHandle<()>>,
}

impl X11Capture {
    pub fn new() -> Result<Self, CaptureError> {
        let (conn, screen_num) =
            RustConnection::connect(None).change_context(CaptureError::Connection)?;
        let screen = conn.setup().roots[screen_num].clone();
        let use_shm = shm_available(&conn);

        tracing::info!(
            "X11 capture: {}x{}, depth={}, shm={}",
            screen.width_in_pixels,
            screen.height_in_pixels,
            screen.root_depth,
            use_shm
        );

        Ok(Self {
            conn: Arc::new(conn),
            screen,
            use_shm,
            stop_flag: None,
            capture_thread: None,
        })
    }

    fn grab_region(&self, region: Region) -> Result<CaptureFrame, CaptureError> {
        let width = region.width.min(self.screen.width_in_pixels as u32);
        let height = region.height.min(self.screen.height_in_pixels as u32);

        if self.use_shm {
            let size = (width * height * 4) as usize;
            let shm = ShmSegment::new(&self.conn, size)?;
            grab_shm(
                &self.conn,
                self.screen.root,
                &shm,
                region,
                width,
                height,
                self.screen.root_depth,
            )
        } else {
            grab_getimage(
                &self.conn,
                self.screen.root,
                region,
                width,
                height,
                self.screen.root_depth,
            )
        }
    }
}

impl ScreenCapture for X11Capture {
    fn screenshot_full(&mut self) -> Result<CaptureFrame, CaptureError> {
        let region = Region {
            x: 0,
            y: 0,
            width: self.screen.width_in_pixels as u32,
            height: self.screen.height_in_pixels as u32,
        };
        self.grab_region(region)
    }

    fn start_capture(
        &mut self,
        region: Region,
        fps: u8,
    ) -> Result<CaptureReceiver, CaptureError> {
        if self.stop_flag.is_some() {
            return Err(error_stack::report!(CaptureError::AlreadyCapturing));
        }

        let (tx, rx) = mpsc::sync_channel::<CaptureFrame>(30);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let conn = self.conn.clone();
        let screen = self.screen.clone();
        let use_shm = self.use_shm;

        let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);

        let handle = thread::spawn(move || {
            let width = region.width.min(screen.width_in_pixels as u32);
            let height = region.height.min(screen.height_in_pixels as u32);
            let depth = screen.root_depth;

            // For SHM mode, allocate once and reuse
            let shm = if use_shm {
                let size = (width * height * 4) as usize;
                match ShmSegment::new(&conn, size) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        tracing::warn!("SHM alloc failed, falling back to GetImage: {e}");
                        None
                    }
                }
            } else {
                None
            };

            while !stop_clone.load(Ordering::Relaxed) {
                let frame_start = Instant::now();

                let result = if let Some(ref shm) = shm {
                    grab_shm(&conn, screen.root, shm, region, width, height, depth)
                } else {
                    grab_getimage(&conn, screen.root, region, width, height, depth)
                };

                match result {
                    Ok(frame) => {
                        if tx.send(frame).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("frame capture failed: {e}");
                    }
                }

                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    thread::sleep(frame_interval - elapsed);
                }
            }
        });

        self.stop_flag = Some(stop_flag);
        self.capture_thread = Some(handle);

        Ok(rx)
    }

    fn stop_capture(&mut self) -> Result<(), CaptureError> {
        let stop_flag = self
            .stop_flag
            .take()
            .ok_or_else(|| error_stack::report!(CaptureError::NotCapturing))?;
        stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.capture_thread.take() {
            handle.join().ok();
        }

        Ok(())
    }
}
