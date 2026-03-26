use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use error_stack::{Result, ResultExt};
use rugif_core::{CaptureFrame, Region};
use x11rb::connection::Connection;
use x11rb::protocol::shm;
use x11rb::protocol::xproto::{ImageFormat, Screen};
use x11rb::rust_connection::RustConnection;

use crate::{CaptureError, CaptureReceiver, ScreenCapture};

/// Shared memory segment wrapper for X11 SHM capture.
struct ShmSegment {
    addr: *mut u8,
    size: usize,
    seg_id: u32,
}

unsafe impl Send for ShmSegment {}

impl ShmSegment {
    fn new(conn: &RustConnection, size: usize) -> Result<Self, CaptureError> {
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
            shm::attach(conn, seg_id, shmid as u32, false)
                .change_context(CaptureError::Screenshot("shm attach failed".into()))?;

            // Mark for removal once all processes detach
            libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut());

            Ok(Self {
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
        unsafe {
            libc::shmdt(self.addr as *const _);
        }
    }
}

/// Grab a region from the screen into an existing SHM segment, returning RGBA data.
fn grab_into_shm(
    conn: &RustConnection,
    root: u32,
    shm_seg: &ShmSegment,
    region: Region,
    width: u32,
    height: u32,
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
    .change_context(CaptureError::Screenshot("get_image request failed".into()))?
    .reply()
    .change_context(CaptureError::Screenshot("get_image reply failed".into()))?;

    // X11 typically gives BGRA; convert to RGBA
    let mut data = shm_seg.as_slice()[..(width * height * 4) as usize].to_vec();
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    Ok(CaptureFrame {
        data,
        width,
        height,
        timestamp: Instant::now(),
    })
}

/// X11 screen capture backend using SHM for fast frame grabs.
pub struct X11Capture {
    conn: Arc<RustConnection>,
    screen: Screen,
    stop_flag: Option<Arc<AtomicBool>>,
    capture_thread: Option<thread::JoinHandle<()>>,
}

impl X11Capture {
    pub fn new() -> Result<Self, CaptureError> {
        let (conn, screen_num) =
            RustConnection::connect(None).change_context(CaptureError::Connection)?;
        let screen = conn.setup().roots[screen_num].clone();

        Ok(Self {
            conn: Arc::new(conn),
            screen,
            stop_flag: None,
            capture_thread: None,
        })
    }
}

impl ScreenCapture for X11Capture {
    fn screenshot_full(&mut self) -> Result<CaptureFrame, CaptureError> {
        let width = self.screen.width_in_pixels as u32;
        let height = self.screen.height_in_pixels as u32;
        let size = (width * height * 4) as usize;
        let region = Region {
            x: 0,
            y: 0,
            width,
            height,
        };

        let shm = ShmSegment::new(&self.conn, size)?;
        grab_into_shm(&self.conn, self.screen.root, &shm, region, width, height)
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
        let root = self.screen.root;
        let screen_w = self.screen.width_in_pixels as u32;
        let screen_h = self.screen.height_in_pixels as u32;

        let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);

        let handle = thread::spawn(move || {
            let width = region.width.min(screen_w);
            let height = region.height.min(screen_h);
            let size = (width * height * 4) as usize;

            // Allocate SHM once, reuse for every frame
            let shm = match ShmSegment::new(&conn, size) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("failed to allocate SHM segment: {e}");
                    return;
                }
            };

            while !stop_clone.load(Ordering::Relaxed) {
                let frame_start = Instant::now();

                match grab_into_shm(&conn, root, &shm, region, width, height) {
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
