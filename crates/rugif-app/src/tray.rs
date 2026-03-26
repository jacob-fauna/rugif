use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ksni::menu::{MenuItem, StandardItem};
use ksni::{Tray, TrayMethods};

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
}

impl Tray for RugifTray {
    fn id(&self) -> String {
        "rugif".into()
    }

    fn title(&self) -> String {
        "rugif".into()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![tray_icon()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "rugif - GIF Recorder".into(),
            description: "Click to record a GIF".into(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let quit_flag = self.quit_flag.clone();
        vec![
            StandardItem {
                label: "Record GIF".into(),
                activate: Box::new(|_| spawn_rugif(&[])),
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

/// Spawn `rugif` as a subprocess with the given args.
/// Each invocation gets a clean process — no winit event loop conflicts.
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

    let tray = RugifTray {
        quit_flag: quit_flag.clone(),
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let _handle = tray.spawn().await?;
        tracing::info!("tray mode started — right-click the tray icon for options");

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if quit_flag.load(Ordering::Relaxed) {
                break;
            }
        }

        tracing::info!("tray exiting");
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    Ok(())
}
