use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use error_stack::{Result, ResultExt};
use gifski::collector::{ImgVec, RGBA8};
use rugif_core::CaptureFrame;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("failed to initialize encoder: {0}")]
    Init(String),
    #[error("failed to add frame: {0}")]
    AddFrame(String),
    #[error("failed to write GIF: {0}")]
    Write(String),
}

/// Convert raw RGBA u8 bytes to RGBA8 pixel vec.
fn rgba_bytes_to_pixels(data: &[u8]) -> Vec<RGBA8> {
    data.chunks_exact(4)
        .map(|c| RGBA8::new(c[0], c[1], c[2], c[3]))
        .collect()
}

/// Encode captured frames into a GIF file using gifski.
///
/// Consumes frames from `receiver` until the channel is closed (sender dropped),
/// then finalizes the GIF. This function blocks until encoding is complete.
pub fn encode_gif(
    receiver: mpsc::Receiver<CaptureFrame>,
    output_path: &Path,
    fps: u8,
    quality: u8,
    width: u32,
    height: u32,
) -> Result<(), EncodeError> {
    let settings = gifski::Settings {
        width: Some(width),
        height: Some(height),
        quality,
        fast: false,
        repeat: gifski::Repeat::Infinite,
    };

    let (collector, writer) =
        gifski::new(settings).change_context(EncodeError::Init("gifski::new failed".into()))?;

    let output_file = File::create(output_path)
        .change_context(EncodeError::Write("failed to create output file".into()))?;

    // Writer runs on its own thread — it writes the GIF to disk as frames arrive.
    let writer_handle = thread::spawn(move || {
        let mut buf = BufWriter::new(output_file);
        writer
            .write(&mut buf, &mut gifski::progress::NoProgress {})
            .map_err(|e| error_stack::report!(EncodeError::Write(e.to_string())))
    });

    // Feed frames from the capture channel into the collector.
    // Skip the first ~0.5s of frames to avoid window transition artifacts.
    let skip_frames = (fps as usize / 2).max(1);
    let frame_duration = 1.0 / fps as f64;
    let start_time = Instant::now();
    let mut frame_index: usize = 0;
    let mut total_received: usize = 0;

    for frame in receiver {
        total_received += 1;
        if total_received <= skip_frames {
            continue;
        }

        let presentation_timestamp = frame_index as f64 * frame_duration;
        let pixels = rgba_bytes_to_pixels(&frame.data);

        collector
            .add_frame_rgba(
                frame_index,
                ImgVec::new(pixels, frame.width as usize, frame.height as usize),
                presentation_timestamp,
            )
            .change_context(EncodeError::AddFrame(format!("frame {frame_index}")))?;

        frame_index += 1;
    }

    // Drop collector to signal the writer that we're done.
    drop(collector);

    let elapsed = start_time.elapsed();
    tracing::info!(
        "encoded {frame_index} frames in {:.2}s",
        elapsed.as_secs_f64()
    );

    // Wait for writer to finish.
    writer_handle
        .join()
        .map_err(|_| error_stack::report!(EncodeError::Write("writer thread panicked".into())))??;

    Ok(())
}
