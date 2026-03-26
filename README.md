# rugif

A fast, self-contained GIF screen recorder for Linux with a snipping-tool-style interface. Select a region, record, and get a high-quality GIF — no ffmpeg required.

![icon](assets/rugif.png)

## Features

- **Snipping tool UI** — fullscreen overlay with click-and-drag region selection
- **High-quality GIFs** — powered by [gifski](https://github.com/ImageOptim/gifski) with temporal dithering and thousands of colors per frame
- **System tray** — runs in the background, record anytime from the tray menu
- **Wayland & X11** — native support for both display servers
- **Self-contained** — no runtime dependencies on ffmpeg or other tools
- **Settings UI** — configure FPS, quality, save location, shortcuts, and autostart
- **Start on login** — optional systemd service or XDG autostart

## Install

### One-liner

```bash
curl -sSf https://raw.githubusercontent.com/jacob-fauna/rugif/master/install.sh | bash
```

This will check dependencies, build from source, install the binary, set up the desktop entry and icon, and optionally enable start-on-login.

### Manual

**Prerequisites:**

```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# System libraries
sudo apt install pkg-config libpipewire-0.3-dev libclang-dev
```

**Build and install:**

```bash
git clone https://github.com/jacob-fauna/rugif.git
cd rugif

# Wayland (recommended — also supports XWayland)
cargo install --path crates/rugif-app --features wayland

# X11 only (no pipewire dependency)
cargo install --path crates/rugif-app
```

**Desktop integration (optional):**

```bash
# Application icon
mkdir -p ~/.local/share/icons/hicolor/128x128/apps
cp assets/rugif.png ~/.local/share/icons/hicolor/128x128/apps/rugif.png

# Desktop entry (shows in app launcher search)
cp rugif.desktop ~/.local/share/applications/
sed -i "s|Exec=rugif|Exec=$(which rugif)|g" ~/.local/share/applications/rugif.desktop

# Start tray on login via systemd
mkdir -p ~/.config/systemd/user
cp rugif.service ~/.config/systemd/user/
sed -i "s|ExecStart=rugif|ExecStart=$(which rugif)|g" ~/.config/systemd/user/rugif.service
systemctl --user daemon-reload
systemctl --user enable rugif --now
```

### Uninstall

```bash
curl -sSf https://raw.githubusercontent.com/jacob-fauna/rugif/master/uninstall.sh | bash
```

Or manually:

```bash
systemctl --user disable rugif --now
cargo uninstall rugif
rm -f ~/.local/share/applications/rugif.desktop
rm -f ~/.local/share/icons/hicolor/128x128/apps/rugif.png
rm -f ~/.config/systemd/user/rugif.service
rm -f ~/.config/autostart/rugif.desktop
rm -rf ~/.config/rugif/
```

## Usage

### Record a GIF

```bash
rugif
```

1. A screen picker dialog appears (Wayland) or a fullscreen overlay (X11)
2. Click and drag to select the region to record
3. A small controls window appears — click **Stop** or press **Escape** when done
4. The GIF is encoded and saved

### System tray mode

```bash
rugif --tray
```

Right-click the tray icon for:
- **Record GIF** — start a recording
- **Settings** — open the settings window
- **Quit** — exit the tray

### CLI options

```
rugif [OPTIONS]

Options:
  -o, --output <PATH>              Output file path
      --fps <FPS>                  Frames per second (default: 15)
      --quality <1-100>            GIF quality (default: 90)
      --max-duration <SECONDS>     Max recording duration (default: 30)
      --region <x,y,width,height>  Record a fixed region (skips selection UI)
      --tray                       Run in system tray mode
  -h, --help                       Show help
```

### Examples

```bash
# Record with custom settings
rugif --fps 20 --quality 95 -o ~/Videos/demo.gif

# Record a specific region without the selection UI
rugif --region 100,200,800,600 -o capture.gif

# Quick 5-second recording
rugif --max-duration 5
```

## Settings

Settings are stored in `~/.config/rugif/settings.toml` and can be edited via the tray's **Settings** menu or directly:

```toml
[recording]
fps = 15
quality = 90
max_duration_secs = 30
save_directory = "/home/user/Videos/rugif"

[shortcuts]
record = "Super+Shift+R"
stop = "Super+Shift+S"

[general]
start_on_login = true
start_minimized = true
notify_on_save = true
```

| Setting | Description | Default |
|---------|-------------|---------|
| `fps` | Capture framerate | 15 |
| `quality` | gifski quality (1-100, higher = better + slower) | 90 |
| `max_duration_secs` | Auto-stop after this many seconds | 30 |
| `save_directory` | Where GIFs are saved | `~/Videos/rugif` |
| `start_on_login` | Launch tray on login | false |

## Architecture

rugif is a Cargo workspace with 5 crates:

```
crates/
  rugif-core/      Shared types, config, settings persistence
  rugif-capture/   Screen capture (X11 via x11rb+SHM, Wayland via ashpd+PipeWire)
  rugif-encode/    GIF encoding via gifski
  rugif-ui/        egui windows (region selection, recording controls, settings)
  rugif-app/       Binary, CLI, system tray (ksni)
```

**Capture pipeline:**

```
Screen → PipeWire/X11 SHM → mpsc channel → gifski encoder → .gif file
```

## Requirements

- **Linux** (X11 or Wayland)
- **Rust** 1.70+
- **PipeWire** (Wayland capture; most modern distros ship this)
- **D-Bus** (system tray via StatusNotifierItem protocol)

## License

MIT
