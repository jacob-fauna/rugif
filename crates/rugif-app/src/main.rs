mod tray;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::Parser;
use rugif_core::config::Settings;
use rugif_core::{DisplayServer, RecordingConfig, Region};

#[derive(Parser, Debug)]
#[command(name = "rugif", about = "Record screen regions as high-quality GIFs")]
struct Args {
    /// Output file path (overrides settings)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Frames per second (overrides settings)
    #[arg(long)]
    fps: Option<u8>,

    /// GIF quality 1-100 (overrides settings)
    #[arg(long)]
    quality: Option<u8>,

    /// Maximum recording duration in seconds (overrides settings)
    #[arg(long)]
    max_duration: Option<u32>,

    /// Record a fixed region (x,y,width,height) — skips the selection UI
    #[arg(long, value_parser = parse_region)]
    region: Option<Region>,

    /// Run in system tray mode (background daemon)
    #[arg(long)]
    tray: bool,

    /// Open the settings window
    #[arg(long)]
    settings: bool,
}

fn parse_region(s: &str) -> Result<Region, String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err("expected format: x,y,width,height".into());
    }
    Ok(Region {
        x: parts[0].parse().map_err(|e| format!("bad x: {e}"))?,
        y: parts[1].parse().map_err(|e| format!("bad y: {e}"))?,
        width: parts[2].parse().map_err(|e| format!("bad width: {e}"))?,
        height: parts[3].parse().map_err(|e| format!("bad height: {e}"))?,
    })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    if args.tray {
        if let Err(e) = tray::run_tray() {
            tracing::error!("tray error: {e:?}");
            std::process::exit(1);
        }
        return;
    }

    if args.settings {
        let settings = rugif_core::config::Settings::load();
        match rugif_ui::settings::show_settings(settings) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("settings error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Direct recording mode.
    let display_server = match DisplayServer::detect() {
        Some(ds) => {
            tracing::info!("detected display server: {ds:?}");
            ds
        }
        None => {
            tracing::error!("no display server detected (need WAYLAND_DISPLAY or DISPLAY)");
            std::process::exit(1);
        }
    };

    let settings = Settings::load();
    let config = build_config(&args, &settings);

    if let Err(e) = record(display_server, config, args.region) {
        tracing::error!("fatal: {e:?}");
        std::process::exit(1);
    }
}

fn build_config(args: &Args, settings: &Settings) -> RecordingConfig {
    let fps = args.fps.unwrap_or(settings.recording.fps);
    let quality = args.quality.unwrap_or(settings.recording.quality);
    let max_duration_secs = args.max_duration.unwrap_or(settings.recording.max_duration_secs);

    let output_path = args.output.clone().unwrap_or_else(|| {
        let dir = &settings.recording.save_directory;
        let timestamp = chrono_timestamp();
        dir.join(format!("rugif_{timestamp}.gif"))
    });

    RecordingConfig {
        fps,
        quality,
        output_path,
        max_duration_secs,
    }
}

fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

pub fn record(
    display_server: DisplayServer,
    config: RecordingConfig,
    cli_region: Option<Region>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure save directory exists.
    if let Some(parent) = config.output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut capture = rugif_capture::create_capture(display_server)?;

    // Determine the recording region.
    let region = if let Some(r) = cli_region {
        tracing::info!("using CLI region: {}x{}+{}+{}", r.width, r.height, r.x, r.y);
        r
    } else {
        let screenshot = capture.screenshot_full()?;
        tracing::info!(
            "screenshot: {}x{} ({} bytes)",
            screenshot.width,
            screenshot.height,
            screenshot.data.len()
        );

        match rugif_ui::selection::select_region(
            screenshot.data,
            screenshot.width,
            screenshot.height,
        )? {
            rugif_ui::selection::SelectionResult::Selected(r) => r,
            rugif_ui::selection::SelectionResult::Cancelled => {
                tracing::info!("selection cancelled");
                return Ok(());
            }
        }
    };

    tracing::info!(
        "recording region: {}x{}+{}+{}",
        region.width,
        region.height,
        region.x,
        region.y
    );

    // Start capturing frames.
    let receiver = capture.start_capture(region, config.fps)?;
    tracing::info!(
        "recording started (max {}s, {} fps)",
        config.max_duration_secs,
        config.fps
    );

    let encode_fps = config.fps;
    let encode_quality = config.quality;
    let output_path = config.output_path.clone();
    let width = region.width;
    let height = region.height;

    // Run encoder on a separate thread.
    let encode_handle = thread::spawn(move || {
        rugif_encode::encode_gif(receiver, &output_path, encode_fps, encode_quality, width, height)
    });

    // Shared stop flag.
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Auto-stop timer.
    let max_dur = Duration::from_secs(config.max_duration_secs as u64);
    let timer_stop = stop_flag.clone();
    thread::spawn(move || {
        thread::sleep(max_dur);
        timer_stop.store(true, Ordering::Relaxed);
    });

    // Show recording controls (blocks until user clicks Stop or timer fires).
    let controls_stop = stop_flag.clone();
    rugif_ui::controls::show_recording_controls(controls_stop, region.x, region.y)?;

    // Stop capture — this drops the sender, signaling the encoder to finalize.
    tracing::info!("stopping capture...");
    capture.stop_capture()?;
    tracing::info!("capture stopped, waiting for GIF encoding to finish...");

    // Wait for encoder to finish (gifski may take a while for high-quality encoding).
    encode_handle
        .join()
        .map_err(|_| "encoder thread panicked")??;

    tracing::info!("GIF saved to {}", config.output_path.display());

    // Show a notification window so the user knows where the file was saved.
    rugif_ui::notification::show_save_notification(&config.output_path);

    Ok(())
}
