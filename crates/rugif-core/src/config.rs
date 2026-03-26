use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Persistent application settings stored as TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub recording: RecordingSettings,
    pub shortcuts: ShortcutSettings,
    pub general: GeneralSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingSettings {
    /// Frames per second.
    pub fps: u8,
    /// GIF quality (1-100).
    pub quality: u8,
    /// Maximum recording duration in seconds.
    pub max_duration_secs: u32,
    /// Directory to save GIFs.
    pub save_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutSettings {
    /// Keyboard shortcut to start recording (e.g. "Super+Shift+R").
    pub record: String,
    /// Keyboard shortcut to stop recording.
    pub stop: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Start rugif in the system tray on login.
    pub start_on_login: bool,
    /// Start minimized to tray.
    pub start_minimized: bool,
    /// Show a notification when GIF is saved.
    pub notify_on_save: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recording: RecordingSettings::default(),
            shortcuts: ShortcutSettings::default(),
            general: GeneralSettings::default(),
        }
    }
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self {
            fps: 15,
            quality: 90,
            max_duration_secs: 30,
            save_directory: default_save_directory(),
        }
    }
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            record: "Super+Shift+R".into(),
            stop: "Super+Shift+S".into(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            start_on_login: false,
            start_minimized: true,
            notify_on_save: true,
        }
    }
}

fn default_save_directory() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.video_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.home_dir().join("Videos"))
                .unwrap_or_else(|| PathBuf::from("."))
        })
        .join("rugif")
}

/// Return the path to the config file (~/.config/rugif/settings.toml).
pub fn config_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "rugif")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".config/rugif"))
        .join("settings.toml")
}

impl Settings {
    /// Load settings from disk, or return defaults if the file doesn't exist.
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match toml::from_str(&contents) {
                    Ok(settings) => return settings,
                    Err(e) => eprintln!("warning: failed to parse {}: {e}", path.display()),
                },
                Err(e) => eprintln!("warning: failed to read {}: {e}", path.display()),
            }
        }
        Self::default()
    }

    /// Save settings to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        Ok(())
    }
}

/// Manage XDG autostart entry for rugif.
pub fn set_autostart(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    let autostart_dir = directories::BaseDirs::new()
        .map(|d| d.config_dir().join("autostart"))
        .unwrap_or_else(|| PathBuf::from(".config/autostart"));

    let desktop_path = autostart_dir.join("rugif.desktop");

    if enabled {
        fs::create_dir_all(&autostart_dir)?;
        let exe = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("rugif"));
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=rugif\n\
             Comment=GIF screen recorder\n\
             Exec={} --tray\n\
             Terminal=false\n\
             StartupNotify=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        fs::write(&desktop_path, contents)?;
    } else if desktop_path.exists() {
        fs::remove_file(&desktop_path)?;
    }

    Ok(())
}
