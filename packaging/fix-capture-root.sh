#!/usr/bin/env bash
# Dremes — permanent capture-stack repairs (run as root, e.g. via pkexec from
# the Capture tab's "Fix All" flow, or manually: sudo bash /usr/lib/dremes-engine/fix-capture-root.sh).
#
# What it does:
#   1. Makes BOTH loopback V4L2 nodes persist across reboots (Waydroid node
#      /dev/video10 + a free general-capture node /dev/video11) so OBS always
#      has the capture outputs after a restart.
#   2. Loads the module now if it is missing.
#   3. Verifies the nodes and OBS's linux-pipewire plugin.
set -euo pipefail

CONF="/etc/modprobe.d/v4l2loopback.conf"
LOAD="/etc/modules-load.d/v4l2loopback.conf"

echo "== v4l2loopback persistence =="
# Preserve the existing Waydroid node-10 setup and add node 11 for general
# capture. exclusive_caps=0 keeps the Waydroid pipeline behavior unchanged.
mkdir -p /etc/modprobe.d /etc/modules-load.d
cat >"$CONF" <<'EOF'
options v4l2loopback video_nr=10,11 card_label=WaydroidCapture,GeneralCapture exclusive_caps=0
EOF
printf 'v4l2loopback\n' >"$LOAD"
echo "  wrote $CONF"
echo "  wrote $LOAD"

if ! grep -qw v4l2loopback /proc/modules; then
  echo "== loading v4l2loopback module =="
  modprobe v4l2loopback video_nr=10,11 card_label=WaydroidCapture,GeneralCapture exclusive_caps=0
  udevadm settle
else
  echo "== v4l2loopback already loaded =="
fi

echo "== loopback nodes =="
for n in /dev/video10 /dev/video11; do
  v="$(basename "$n")"
  if [ -e "$n" ]; then
    echo "  $n present — $(cat "/sys/class/video4linux/$v/name" 2>/dev/null)"
  else
    echo "  $n MISSING"
  fi
done

echo "== OBS linux-pipewire plugin =="
if [ -f /usr/lib/x86_64-linux-gnu/obs-plugins/linux-pipewire.so ]; then
  echo "  present — OBS Screen Capture / Window Capture (PipeWire) will be available"
else
  echo "  not found under /usr/lib/x86_64-linux-gnu/obs-plugins/ — check the OBS install"
fi

echo "== done =="