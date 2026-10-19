#!/usr/bin/env bash
#
# Build a Dremes .deb package from release artifacts.
#
# The .deb bundles:
#   - the Dremes GUI binary (built by Tauri -> apps/gui/src-tauri/target/release/dremes-gui)
#   - the Dremes Engine daemon (cargo release build -> dremes-engine)
#   - an opt-in systemd unit (NOT enabled by postinst)
#   - a desktop entry, icon, docs, and uninstaller
#
# Prereqs (host): cargo, dpkg-deb, and the Tauri/WebKit toolchain to build the
# GUI (npm i @tauri-apps/cli && npm run build:tauri).
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="${ROOT}/packaging/deb"
VERSION="0.2.0"
ARCH="amd64"

GUI_BIN="${ROOT}/apps/gui/src-tauri/target/release/dremes-gui"
ENGINE_BIN="${ROOT}/target/release/dremes-engine"

echo "==> Building engine (release)"
(cd "${ROOT}" && cargo build --release -p dremes-engine)

if [ ! -f "${ENGINE_BIN}" ]; then
  echo "ERROR: engine binary not found after build: ${ENGINE_BIN}" >&2
  exit 1
fi

# Fresh staging tree; templates are tracked as packaging/deb-*.
rm -rf "${PKG}"
mkdir -p "${PKG}/DEBIAN" "${PKG}/usr/bin"
cp "${ROOT}/packaging/deb-control"   "${PKG}/DEBIAN/control"
cp "${ROOT}/packaging/deb-postinst"  "${PKG}/DEBIAN/postinst"
cp "${ROOT}/packaging/deb-prerm"     "${PKG}/DEBIAN/prerm"
chmod 0755 "${PKG}/DEBIAN/postinst" "${PKG}/DEBIAN/prerm"

if [ ! -f "${GUI_BIN}" ]; then
  echo "WARNING: GUI binary not found at ${GUI_BIN}."
  echo "  Build it with: cd apps/gui && npm i && npm i @tauri-apps/cli && npm run build:tauri"
  echo "  (a placeholder is used so the .deb still assembles; re-run after building.)"
  echo "placholder" > "${PKG}/usr/bin/dremes"
else
  cp "${GUI_BIN}" "${PKG}/usr/bin/dremes"
fi

echo "==> Staging files"
# Engine runtime
mkdir -p "${PKG}/usr/lib/dremes-engine"
cp "${ENGINE_BIN}" "${PKG}/usr/lib/dremes-engine/dremes-engine"
chmod 0755 "${PKG}/usr/lib/dremes-engine/dremes-engine"

# Capture root-repair helper (invoked with pkexec from the Capture tab)
cp "${ROOT}/packaging/fix-capture-root.sh" "${PKG}/usr/lib/dremes-engine/fix-capture-root.sh"
chmod 0755 "${PKG}/usr/lib/dremes-engine/fix-capture-root.sh"

# Login-time portal re-sync helper (user-scoped, no root) so OBS's
# Screen Capture (PipeWire) never disappears after a reboot.
cp "${ROOT}/packaging/portal-sync.sh" "${PKG}/usr/lib/dremes-engine/portal-sync.sh"
chmod 0755 "${PKG}/usr/lib/dremes-engine/portal-sync.sh"

# systemd unit (opt-in)
mkdir -p "${PKG}/usr/lib/systemd/user"
cp "${ROOT}/packaging/dremes-engine.service" "${PKG}/usr/lib/systemd/user/dremes-engine.service"
cp "${ROOT}/packaging/dremes-portal-sync.service" "${PKG}/usr/lib/systemd/user/dremes-portal-sync.service"

# desktop entry
mkdir -p "${PKG}/usr/share/applications"
cp "${ROOT}/packaging/dremes.desktop" "${PKG}/usr/share/applications/dremes.desktop"

# icon
ICON_SRC=""
for cand in "${ROOT}/apps/gui/src-tauri/icons/icon.png" "${ROOT}/apps/gui/src-tauri/icons/128x128@2x.png"; do
  [ -f "$cand" ] && ICON_SRC="$cand" && break
done
if [ -n "$ICON_SRC" ]; then
  mkdir -p "${PKG}/usr/share/icons/hicolor/256x256/apps"
  cp "$ICON_SRC" "${PKG}/usr/share/icons/hicolor/256x256/apps/dremes.png"
fi

# docs
mkdir -p "${PKG}/usr/share/doc/dremes"
cp "${ROOT}/README.md" "${PKG}/usr/share/doc/dremes/README.md"
cp "${ROOT}/LICENSE" "${PKG}/usr/share/doc/dremes/copyright"
cp "${ROOT}/packaging/uninstall.sh" "${PKG}/usr/share/doc/dremes/uninstall.sh"
chmod 0755 "${PKG}/usr/share/doc/dremes/uninstall.sh"

# branding assets (profile photo + red D logo) so the packaged app is
# self-contained and not dependent on files in the developer's home dir.
mkdir -p "${PKG}/usr/share/doc/dremes/assets"
[ -f "${ROOT}/branding/assets/profile-raw.png" ] && cp "${ROOT}/branding/assets/profile-raw.png" "${PKG}/usr/share/doc/dremes/assets/profile.png"
[ -f "${ROOT}/branding/icons/dremes-logo-512.png" ] && cp "${ROOT}/branding/icons/dremes-logo-512.png" "${PKG}/usr/share/doc/dremes/assets/dremes-logo.png"

# compute installed size
INSTALLED=$(du -sk "${PKG}/usr" 2>/dev/null | cut -f1)
sed -i "s/^Installed-Size:.*/Installed-Size: ${INSTALLED}/" "${PKG}/DEBIAN/control"

echo "==> Assembling .deb"
OUT="${ROOT}/target/dremes_${VERSION}_${ARCH}.deb"
mkdir -p "$(dirname "$OUT")"
fakeroot dpkg-deb --build "${PKG}" "${OUT}" 2>/dev/null || dpkg-deb --build "${PKG}" "${OUT}"

echo "==> Done: ${OUT}"
echo "  Install:  sudo apt install ${OUT}"
echo "  Remove:   sudo apt remove dremes   (or)   sudo /usr/share/doc/dremes/uninstall.sh"
echo "  Engine (opt-in, OFF by default): systemctl --user enable --now dremes-engine.service"
