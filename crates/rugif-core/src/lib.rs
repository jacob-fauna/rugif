pub mod config;

use std::path::PathBuf;
use std::time::Instant;

use thiserror::Error;

/// A rectangular screen region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A single captured frame in RGBA format.
#[derive(Debug, Clone)]
pub struct CaptureFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: Instant,
}

/// Recording configuration (runtime, derived from Settings).
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub fps: u8,
    pub quality: u8,
    pub output_path: PathBuf,
    pub max_duration_secs: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        let settings = config::Settings::default();
        Self {
            fps: settings.recording.fps,
            quality: settings.recording.quality,
            output_path: settings.recording.save_directory.join("output.gif"),
            max_duration_secs: settings.recording.max_duration_secs,
        }
    }
}

/// Detected display server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
}

impl DisplayServer {
    /// Auto-detect the active display server from environment variables.
    pub fn detect() -> Option<Self> {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            Some(Self::Wayland)
        } else if std::env::var("DISPLAY").is_ok() {
            Some(Self::X11)
        } else {
            None
        }
    }
}

#[derive(Debug, Error)]
pub enum RugifError {
    #[error("no display server detected")]
    NoDisplayServer,
    #[error("capture error: {0}")]
    Capture(String),
    #[error("encoding error: {0}")]
    Encode(String),
    #[error("ui error: {0}")]
    Ui(String),
}
