mod tray;

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

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

    /// Internal: stop file path — recording stops when this file appears
    #[arg(long, hide = true)]
    stop_file: Option<PathBuf>,

    /// Internal: show region selection overlay, write result to file
    #[arg(long, hide = true)]
    select_region: Option<String>,
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

    // Internal: selection overlay subprocess — writes region to a file.
    if let Some(ref result_path) = args.select_region {
        let screenshot_path = format!("{result_path}.screenshot");
        let screenshot_data = std::fs::read(&screenshot_path).unwrap_or_default();
        // First 8 bytes: width (u32 LE) + height (u32 LE), rest is RGBA data
        if screenshot_data.len() > 8 {
            let width = u32::from_le_bytes(screenshot_data[0..4].try_into().unwrap());
            let height = u32::from_le_bytes(screenshot_data[4..8].try_into().unwrap());
            let pixels = screenshot_data[8..].to_vec();

            match rugif_ui::selection::select_region(pixels, width, height) {
                Ok(rugif_ui::selection::SelectionResult::Selected(r)) => {
                    let _ = std::fs::write(
                        result_path,
                        format!("{},{},{},{}", r.x, r.y, r.width, r.height),
                    );
                }
                Ok(rugif_ui::selection::SelectionResult::Cancelled) => {
                    let _ = std::fs::write(result_path, "cancelled");
                }
                Err(e) => {
                    tracing::error!("selection error: {e}");
                    let _ = std::fs::write(result_path, "error");
                }
            }
            let _ = std::fs::remove_file(&screenshot_path);
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

    if let Err(e) = record(display_server, config, args.region, args.stop_file.as_deref(), &settings) {
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

fn copy_to_clipboard(path: &std::path::Path) {
    use std::io::Write;

    let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let file_uri = format!("file://{}\n", abs_path.display());

    // Copy as a file URI (text/uri-list) — this is what file managers use
    // when you Ctrl+C a file. Web apps like GitHub accept it for upload.
    let result = std::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/uri-list")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/uri-list"])
                .stdin(std::process::Stdio::piped())
                .spawn()
        });

    match result {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(file_uri.as_bytes());
                drop(stdin);
            }
            let _ = child.wait();
            tracing::info!("copied to clipboard: {}", abs_path.display());
        }
        Err(_) => {
            tracing::warn!("clipboard: wl-copy and xclip not found, skipping");
        }
    }
}

pub fn record(
    display_server: DisplayServer,
    config: RecordingConfig,
    cli_region: Option<Region>,
    stop_file: Option<&std::path::Path>,
    settings: &Settings,
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

        // Run selection overlay as a subprocess to avoid freezing the
        // compositor (fullscreen egui window + Wayland = "not responding").
        let result_file = std::env::temp_dir().join(format!("rugif_sel_{}", std::process::id()));
        let screenshot_file = format!("{}.screenshot", result_file.display());
        let _ = std::fs::remove_file(&result_file);

        // Write screenshot to temp file: [width:4][height:4][rgba...]
        let mut screenshot_blob = Vec::with_capacity(8 + screenshot.data.len());
        screenshot_blob.extend_from_slice(&screenshot.width.to_le_bytes());
        screenshot_blob.extend_from_slice(&screenshot.height.to_le_bytes());
        screenshot_blob.extend_from_slice(&screenshot.data);
        std::fs::write(&screenshot_file, &screenshot_blob)?;

        let exe = std::env::current_exe().unwrap_or_else(|_| "rugif".into());
        let mut child = std::process::Command::new(&exe)
            .arg("--select-region")
            .arg(result_file.to_str().unwrap())
            .spawn()?;

        // Wait for the selection subprocess to finish.
        child.wait()?;

        // Read the result.
        let result_str = std::fs::read_to_string(&result_file).unwrap_or_default();
        let _ = std::fs::remove_file(&result_file);

        if result_str == "cancelled" || result_str == "error" || result_str.is_empty() {
            tracing::info!("selection cancelled");
            return Ok(());
        }

        // Parse "x,y,width,height"
        let parts: Vec<&str> = result_str.trim().split(',').collect();
        if parts.len() != 4 {
            return Err(format!("invalid selection result: {result_str}").into());
        }
        Region {
            x: parts[0].parse()?,
            y: parts[1].parse()?,
            width: parts[2].parse()?,
            height: parts[3].parse()?,
        }
    };

    tracing::info!(
        "recording region: {}x{}+{}+{}",
        region.width,
        region.height,
        region.x,
        region.y
    );

    // Start capturing frames immediately.
    let receiver = capture.start_capture(region, config.fps)?;
    tracing::info!(
        "recording started (max {}s, {} fps)",
        config.max_duration_secs,
        config.fps
    );

    // Drain frames into an in-memory buffer while waiting for the stop signal.
    // Buffering (rather than encoding live) is what makes the post-capture
    // trim UI possible without a re-decode round-trip.
    let max_dur = Duration::from_secs(config.max_duration_secs as u64);
    let deadline = Instant::now() + max_dur;
    let mut frames: Vec<rugif_core::CaptureFrame> = Vec::new();

    'capture: loop {
        // Drain any frames that have arrived since the last poll.
        loop {
            match receiver.try_recv() {
                Ok(f) => frames.push(f),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'capture,
            }
        }

        if let Some(sf) = stop_file {
            if sf.exists() {
                let _ = std::fs::remove_file(sf);
                break;
            }
        }
        if Instant::now() >= deadline {
            tracing::info!("max duration reached");
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    tracing::info!("stopping capture...");
    capture.stop_capture()?;

    // Drain anything still in flight after stop.
    while let Ok(f) = receiver.try_recv() {
        frames.push(f);
    }
    tracing::info!("captured {} frames", frames.len());

    if frames.is_empty() {
        tracing::warn!("no frames captured — nothing to encode");
        return Ok(());
    }

    // Optional trim step: let the user pick a sub-range to encode.
    let range = if settings.general.show_trim_ui && frames.len() > 1 {
        match rugif_ui::trim::show_trim_dialog(&frames, config.fps) {
            Ok(rugif_ui::trim::TrimResult::Save(r)) => r,
            Ok(rugif_ui::trim::TrimResult::Cancel) => {
                tracing::info!("recording discarded by user");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!("trim UI error: {e} — encoding full recording");
                0..frames.len()
            }
        }
    } else {
        0..frames.len()
    };

    // Drain the selected slice out of `frames` so we don't hold two copies.
    let selected: Vec<rugif_core::CaptureFrame> = frames.drain(range).collect();
    drop(frames);
    tracing::info!("encoding {} selected frames", selected.len());

    // Pipe the selected frames through the existing streaming gifski encoder.
    let (tx, rx) = std::sync::mpsc::sync_channel::<rugif_core::CaptureFrame>(30);
    let encode_fps = config.fps;
    let encode_quality = config.quality;
    let output_path = config.output_path.clone();
    let width = region.width;
    let height = region.height;

    let encode_handle = thread::spawn(move || {
        rugif_encode::encode_gif(rx, &output_path, encode_fps, encode_quality, width, height)
    });

    for frame in selected {
        if tx.send(frame).is_err() {
            break;
        }
    }
    drop(tx);

    encode_handle
        .join()
        .map_err(|_| "encoder thread panicked")??;

    tracing::info!("GIF saved to {}", config.output_path.display());

    // Copy to clipboard if enabled.
    if settings.general.copy_to_clipboard {
        copy_to_clipboard(&config.output_path);
    }

    // Show a notification window so the user knows where the file was saved.
    rugif_ui::notification::show_save_notification(&config.output_path);

    Ok(())
}
