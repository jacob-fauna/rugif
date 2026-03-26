#!/usr/bin/env bash
set -euo pipefail

echo "=== rugif uninstaller ==="
echo

# Stop and disable systemd service
if systemctl --user is-active rugif &>/dev/null 2>&1; then
  systemctl --user disable rugif --now 2>/dev/null || true
  echo "Stopped systemd service."
fi
rm -f ~/.config/systemd/user/rugif.service
systemctl --user daemon-reload 2>/dev/null || true

# Remove binary
if command -v rugif &>/dev/null; then
  cargo uninstall rugif 2>/dev/null || true
  echo "Removed binary."
fi

# Remove desktop entry
rm -f ~/.local/share/applications/rugif.desktop
echo "Removed desktop entry."

# Remove autostart entry
rm -f ~/.config/autostart/rugif.desktop
echo "Removed autostart entry."

echo
read -rp "Remove config (~/.config/rugif/)? [y/N] " answer
if [[ "$answer" =~ ^[Yy] ]]; then
  rm -rf ~/.config/rugif/
  echo "Removed config."
else
  echo "Config kept at ~/.config/rugif/"
fi

echo
echo "=== Done! ==="
