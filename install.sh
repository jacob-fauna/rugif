#!/usr/bin/env bash
set -euo pipefail

# rugif installer
# Usage: curl -sSf https://raw.githubusercontent.com/USER/rugif/master/install.sh | bash

REPO="jacob-fauna/rugif"
RAW="https://raw.githubusercontent.com/$REPO/master"

echo "=== rugif installer ==="
echo

# --- Check dependencies ---

check_cmd() {
  if ! command -v "$1" &>/dev/null; then
    echo "ERROR: '$1' is required but not found."
    echo "  $2"
    exit 1
  fi
}

check_cmd cargo "Install Rust: https://rustup.rs"
check_cmd pkg-config "Install: sudo apt install pkg-config"

echo "Checking system libraries..."

missing_libs=()

if ! pkg-config --exists libpipewire-0.3 2>/dev/null; then
  missing_libs+=("libpipewire-0.3-dev")
fi

if ! pkg-config --exists libclang 2>/dev/null && ! dpkg -s libclang-dev &>/dev/null 2>&1; then
  missing_libs+=("libclang-dev")
fi

if [ ${#missing_libs[@]} -gt 0 ]; then
  echo
  echo "Missing system libraries: ${missing_libs[*]}"
  echo
  read -rp "Install them with apt? [Y/n] " answer
  if [[ "$answer" =~ ^[Nn] ]]; then
    echo "Please install manually: sudo apt install ${missing_libs[*]}"
    exit 1
  fi
  sudo apt install -y "${missing_libs[@]}"
  echo
fi

# --- Detect display server ---

detect_wayland() {
  [ "${XDG_SESSION_TYPE:-}" = "wayland" ] && return 0
  [ -n "${WAYLAND_DISPLAY:-}" ] && return 0
  # Check if a Wayland compositor is running
  loginctl show-session "$(loginctl | grep "$(whoami)" | awk '{print $1}' | head -1)" -p Type 2>/dev/null | grep -q "wayland" && return 0
  # Check for common Wayland socket
  [ -e "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/wayland-0" ] && return 0
  return 1
}

USE_WAYLAND=false
if detect_wayland; then
  echo "Detected: Wayland"
  USE_WAYLAND=true
elif [ -n "${DISPLAY:-}" ]; then
  echo "Detected: X11"
else
  echo "WARNING: Could not detect display server. Building with X11 support only."
fi

# --- Install binary ---

echo
echo "Building rugif (this may take a few minutes)..."
if [ "$USE_WAYLAND" = true ]; then
  cargo install --git "https://github.com/$REPO" --bin rugif --features wayland
else
  cargo install --git "https://github.com/$REPO" --bin rugif
fi

RUGIF_BIN="$(which rugif)"
echo "Installed: $RUGIF_BIN"

# --- Install icon ---

echo
echo "Installing icon..."
ICON_DIR="$HOME/.local/share/icons/hicolor/128x128/apps"
mkdir -p "$ICON_DIR"
curl -sSf "$RAW/assets/rugif.png" -o "$ICON_DIR/rugif.png"

# --- Desktop entry ---

echo "Installing desktop entry..."
mkdir -p ~/.local/share/applications
curl -sSf "$RAW/rugif.desktop" -o ~/.local/share/applications/rugif.desktop
sed -i "s|Exec=rugif|Exec=$RUGIF_BIN|g" ~/.local/share/applications/rugif.desktop
sed -i "s|Icon=camera-video|Icon=$ICON_DIR/rugif.png|g" ~/.local/share/applications/rugif.desktop

# --- Start on login ---

echo
echo "Setting up rugif tray to start on login..."

STARTED=false

# Method 1: systemd user service (preferred)
if command -v systemctl &>/dev/null && systemctl --user status &>/dev/null 2>&1; then
  mkdir -p ~/.config/systemd/user
  curl -sSf "$RAW/rugif.service" -o ~/.config/systemd/user/rugif.service
  sed -i "s|ExecStart=rugif|ExecStart=$RUGIF_BIN|g" ~/.config/systemd/user/rugif.service

  systemctl --user daemon-reload
  systemctl --user enable rugif --now

  echo "Enabled via systemd user service."
  echo "  Status:  systemctl --user status rugif"
  echo "  Logs:    journalctl --user -u rugif -f"
  STARTED=true
fi

# Method 2: XDG autostart (fallback / additional)
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/rugif.desktop <<AUTOSTART
[Desktop Entry]
Type=Application
Name=rugif
Comment=GIF screen recorder tray
Exec=$RUGIF_BIN --tray
Terminal=false
StartupNotify=false
X-GNOME-Autostart-enabled=true
AUTOSTART
echo "Enabled via XDG autostart (~/.config/autostart/rugif.desktop)."

# Start tray now if systemd didn't already
if [ "$STARTED" = false ]; then
  echo "Starting rugif tray..."
  nohup "$RUGIF_BIN" --tray &>/dev/null &
fi

# --- Done ---

echo
echo "=== Done! ==="
echo
echo "rugif is running in your system tray. Right-click the icon for options."
echo
echo "Usage:"
echo "  rugif              Record a GIF (select region, record, save)"
echo "  rugif --tray       Run in system tray (right-click for menu)"
echo "  rugif --help       Show all options"
echo
echo "Settings: ~/.config/rugif/settings.toml"
echo "Uninstall: curl -sSf $RAW/uninstall.sh | bash"
