use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use ksni::menu::{MenuItem, StandardItem};
use ksni::{Handle, Tray, TrayMethods};

/// Shared state between the tray and the recording subprocess.
struct RecordingState {
    /// True while a recording subprocess is running.
    is_recording: AtomicBool,
    /// When the current recording started.
    started_at: std::sync::Mutex<Option<Instant>>,
    /// Path to the stop signal file — writing to this stops the recording.
    stop_file: std::sync::Mutex<Option<std::path::PathBuf>>,
}

/// Embedded PNG icon, decoded to ARGB32 network byte order for ksni.
fn tray_icon() -> ksni::Icon {
    static ICON_PNG: &[u8] = include_bytes!("../../../assets/rugif.png");

    let img = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .expect("failed to decode embedded icon")
        .to_rgba8();

    let width = img.width() as i32;
    let height = img.height() as i32;

    let mut argb = Vec::with_capacity((width * height * 4) as usize);
    for pixel in img.pixels() {
        let [r, g, b, a] = pixel.0;
        argb.extend_from_slice(&[a, r, g, b]);
    }

    ksni::Icon {
        width,
        height,
        data: argb,
    }
}

struct RugifTray {
    quit_flag: Arc<AtomicBool>,
    recording: Arc<RecordingState>,
}

impl Tray for RugifTray {
    fn id(&self) -> String {
        "rugif".into()
    }

    fn title(&self) -> String {
        if self.recording.is_recording.load(Ordering::Relaxed) {
            let elapsed = self
                .recording
                .started_at
                .lock()
                .unwrap()
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);
            format!("rugif - Recording {:02}:{:02}", elapsed / 60, elapsed % 60)
        } else {
            "rugif".into()
        }
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![tray_icon()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        if self.recording.is_recording.load(Ordering::Relaxed) {
            ksni::ToolTip {
                title: "rugif - Recording...".into(),
                description: "Click to stop recording".into(),
                ..Default::default()
            }
        } else {
            ksni::ToolTip {
                title: "rugif - GIF Recorder".into(),
                description: "Right-click for options".into(),
                ..Default::default()
            }
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click: stop recording if active, otherwise start one.
        if self.recording.is_recording.load(Ordering::Relaxed) {
            stop_recording(&self.recording);
        } else {
            start_recording(&self.recording);
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let quit_flag = self.quit_flag.clone();

        if self.recording.is_recording.load(Ordering::Relaxed) {
            let elapsed = self
                .recording
                .started_at
                .lock()
                .unwrap()
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);

            vec![
                StandardItem {
                    label: format!(
                        "Stop Recording ({:02}:{:02})",
                        elapsed / 60,
                        elapsed % 60
                    ),
                    activate: Box::new(|tray: &mut Self| {
                        stop_recording(&tray.recording);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        } else {
            vec![
                StandardItem {
                    label: "Record GIF".into(),
                    activate: Box::new(|tray: &mut Self| {
                        start_recording(&tray.recording);
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Settings".into(),
                    activate: Box::new(|_| spawn_rugif(&["--settings"])),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(move |_| {
                        quit_flag.store(true, Ordering::Relaxed);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }
}

fn start_recording(state: &Arc<RecordingState>) {
    if state.is_recording.load(Ordering::Relaxed) {
        return;
    }

    let stop_file = std::env::temp_dir().join(format!(
        "rugif_tray_stop_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&stop_file);

    *state.stop_file.lock().unwrap() = Some(stop_file.clone());
    *state.started_at.lock().unwrap() = Some(Instant::now());
    state.is_recording.store(true, Ordering::Relaxed);

    // Pass the stop file path so the recording subprocess watches it.
    let stop_path_str = stop_file.to_string_lossy().to_string();
    spawn_rugif(&["--stop-file", &stop_path_str]);
}

fn stop_recording(state: &Arc<RecordingState>) {
    if let Some(ref stop_file) = *state.stop_file.lock().unwrap() {
        let _ = std::fs::write(stop_file, "stop");
    }
    state.is_recording.store(false, Ordering::Relaxed);
    *state.started_at.lock().unwrap() = None;
}

/// Spawn `rugif` as a subprocess with the given args.
fn spawn_rugif(args: &[&str]) {
    let exe = std::env::current_exe().unwrap_or_else(|_| "rugif".into());
    match std::process::Command::new(&exe).args(args).spawn() {
        Ok(_) => tracing::debug!("spawned: {} {}", exe.display(), args.join(" ")),
        Err(e) => tracing::error!("failed to spawn rugif: {e}"),
    }
}

/// Run rugif in system tray mode. Blocks until the user quits.
pub fn run_tray() -> Result<(), Box<dyn std::error::Error>> {
    let quit_flag = Arc::new(AtomicBool::new(false));
    let recording = Arc::new(RecordingState {
        is_recording: AtomicBool::new(false),
        started_at: std::sync::Mutex::new(None),
        stop_file: std::sync::Mutex::new(None),
    });

    let tray = RugifTray {
        quit_flag: quit_flag.clone(),
        recording: recording.clone(),
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let handle: Handle<RugifTray> = tray.spawn().await?;
        tracing::info!("tray mode started — right-click the tray icon for options");

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            if quit_flag.load(Ordering::Relaxed) {
                break;
            }

            // Update tray title/menu while recording (shows elapsed time).
            if recording.is_recording.load(Ordering::Relaxed) {
                handle.update(|_| {}).await;
            }
        }

        tracing::info!("tray exiting");
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}
