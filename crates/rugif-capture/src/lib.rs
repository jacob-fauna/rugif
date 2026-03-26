use std::sync::mpsc;

use error_stack::Result;
use rugif_core::{CaptureFrame, Region};
use thiserror::Error;

#[cfg(feature = "x11")]
pub mod x11;

#[cfg(feature = "wayland")]
pub mod wayland;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("failed to connect to display server")]
    Connection,
    #[error("failed to capture screenshot: {0}")]
    Screenshot(String),
    #[error("failed to start capture: {0}")]
    StartCapture(String),
    #[error("capture already in progress")]
    AlreadyCapturing,
    #[error("no capture in progress")]
    NotCapturing,
}

/// Receiver end of the frame capture channel.
pub type CaptureReceiver = mpsc::Receiver<CaptureFrame>;

/// Screen capture abstraction over display server backends.
pub trait ScreenCapture: Send {
    /// Take a full-screen screenshot (used for the selection overlay background).
    fn screenshot_full(&mut self) -> Result<CaptureFrame, CaptureError>;

    /// Begin continuous capture of the given region at the given FPS.
    /// Returns a receiver that yields captured frames.
    fn start_capture(
        &mut self,
        region: Region,
        fps: u8,
    ) -> Result<CaptureReceiver, CaptureError>;

    /// Stop an in-progress capture.
    fn stop_capture(&mut self) -> Result<(), CaptureError>;
}

/// Create the appropriate capture backend for the current display server.
pub fn create_capture(
    display_server: rugif_core::DisplayServer,
) -> Result<Box<dyn ScreenCapture>, CaptureError> {
    match display_server {
        #[cfg(feature = "x11")]
        rugif_core::DisplayServer::X11 => {
            let backend = x11::X11Capture::new()?;
            Ok(Box::new(backend))
        }
        #[cfg(not(feature = "x11"))]
        rugif_core::DisplayServer::X11 => {
            Err(error_stack::report!(CaptureError::Connection)
                .attach_printable("X11 support not compiled in"))
        }
        #[cfg(feature = "wayland")]
        rugif_core::DisplayServer::Wayland => {
            let backend = wayland::WaylandCapture::new()?;
            Ok(Box::new(backend))
        }
        #[cfg(not(feature = "wayland"))]
        rugif_core::DisplayServer::Wayland => {
            Err(error_stack::report!(CaptureError::Connection)
                .attach_printable("Wayland support not compiled in"))
        }
    }
}
