#!/usr/bin/env bash
#
# Dremes uninstaller (Linux).
#
# Removes the Dremes GUI, Engine daemon, systemd unit (if the user enabled it),
# desktop entries, icons, and the per-user configuration/socket directory.
# It REQUIRES root for system-wide removal; run with sudo.
#
#   sudo ./uninstall.sh
#
set -euo pipefail

APP_NAME="dremes"
ENGINE_DIR="/usr/lib/dremes-engine"
BIN_DIR="/usr/bin"
DESKTOP_DIR="/usr/share/applications"
ICON_DIR="/usr/share/icons/hicolor"
DOC_DIR="/usr/share/doc/${APP_NAME}"
UDEV_RULES="/usr/lib/udev/rules.d/99-dremes.rules"

echo "==> Stopping the Dremes Engine (if running) and disabling its unit"
systemctl --user disable --now dremes-engine.service 2>/dev/null || true
pkill -f "/usr/lib/${APP_NAME}-engine/${APP_NAME}-engine" 2>/dev/null || true

echo "==> Removing binaries"
rm -f "${BIN_DIR}/${APP_NAME}"
rm -rf "${ENGINE_DIR}"

echo "==> Removing systemd unit"
rm -f "/etc/systemd/system/${APP_NAME}-engine.service"
systemctl daemon-reload 2>/dev/null || true

echo "==> Removing desktop entry and icons"
rm -f "${DESKTOP_DIR}/${APP_NAME}.desktop"
for s in 16 22 24 32 48 64 128 256 512; do
  rm -f "${ICON_DIR}/${s}x${s}/apps/${APP_NAME}.png" || true
done
rm -f "${ICON_DIR}/scalable/apps/${APP_NAME}.svg" || true

echo "==> Removing docs"
rm -rf "${DOC_DIR}"

echo "==> Removing udev rule (V4L2 device permissions)"
rm -f "${UDEV_RULES}"
udevadm control --reload-rules 2>/dev/null || true

echo "==> Removing per-user config (Dremes will forget settings)"
rm -rf "${HOME}/.config/${APP_NAME}"

echo "Done. Dremes has been removed."
