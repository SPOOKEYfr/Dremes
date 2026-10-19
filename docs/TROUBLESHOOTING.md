# Dremes — Troubleshooting & Ops Notes

Notes that don't belong in the main README. Everything here reflects what
Dremes actually does on this machine class (Ubuntu / GNOME Wayland), stated
honestly.

## Why OBS "Screen Capture (PipeWire)" may be missing

Dremes does not (and cannot) control how OBS lists sources. On GNOME Wayland,
OBS's PipeWire Screen/Window Capture sources depend on the **desktop portal**
(`xdg-desktop-portal` + `xdg-desktop-portal-gnome`), not on Dremes. If the
option is sometimes there and sometimes not after a reboot, the portal had not
finished starting before OBS launched, or OBS was started from a non-Wayland /
root context.

Fixes:

- Start OBS from your normal Wayland desktop session (never `sudo`).
- If the source is missing, run **Fix All** first (it ensures the portals are
  running), then fully exit OBS and relaunch it.
- Both portals are statically socket-activated, so they cannot be "disabled";
  a reboot alone should never remove them. Extra manual guard:
  `sudo apt install --reinstall xdg-desktop-portal-gnome`.

## The "active but stale" portal case

The portal can cache "0 available source types" (its backend synced too
early) and OBS then logs `[pipewire] No capture sources available` and hides
the source. Dremes ships a user one-shot unit, `dremes-portal-sync.service`,
that restarts the portal when it reports 0 types at login, and the Engine
detects/fixes it via **Fix All** (no root needed). Enable both for automatic
coverage:

```bash
systemctl --user enable --now dremes-portal-sync.service
systemctl --user enable --now dremes-engine.service
```

## Loopback nodes

A general (non-Waydroid) capture needs an extra loopback node. Reserve both
nodes at boot with:

```bash
sudo modprobe v4l2loopback video_nr=10,11
# persistacross reboots:
echo "options v4l2loopback video_nr=10,11" | sudo tee /etc/modprobe.d/v4l2loopback.conf
echo "v4l2loopback" | sudo tee /etc/modules-load.d/v4l2loopback.conf
```

- `/dev/video10` stays reserved for the Waydroid pipeline.
- General capture (Camera/Game/Window/Monitor/Mobile) uses a free node such
  as `/dev/video11`.

The Capture tab's **Capture Health** card (`CaptureDiagnose`) plus one-click
**Fix All** (`CaptureFix`) checks most of this and repairs what it can
without root. Making the nodes survive reboots is handled by the same
mechanism above, or by **"Apply permanent fixes (password)"** in the app,
which runs `fix-capture-root.sh` through `pkexec`.

## Game capture (obs-vkcapture)

The Vulkan layer `obs-vkcapture` is the only clean way to capture native
Vulkan games. It is not in apt — build from source. In OBS, add a
**Game Capture** source, then launch the game as `obs-vkcapture <command>`.
On GNOME Wayland this covers Vulkan games; OpenGL-only games on GNOME fall
back to OBS Screen Capture (PipeWire). Dremes detects whether the layer is
installed and tells you honestly in the Capture catalog.
