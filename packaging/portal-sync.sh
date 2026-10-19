#!/bin/sh
# Dremes portal re-sync, run once at login.
#
# Problem it solves: the XDG desktop portal sometimes caches "0 available
# source types" because its backend wasn't ready when it first synced. OBS then
# logs "[pipewire] No capture sources available" and hides the
# "Screen Capture (PipeWire)" source entirely.
# A restart of xdg-desktop-portal makes it re-query the GNOME backend (which
# reports 7 types: monitor + window + audio), so OBS sees the source again.
#
# Safe: only restarts the portal when it is stale (0 types), never when healthy.
set -eu

READ_TYPES="busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.ScreenCast AvailableSourceTypes"

i=0
types=""
while [ $i -lt 50 ]; do
    types="$($READ_TYPES 2>/dev/null | awk '{print $NF}')"
    case "$types" in
        ''|*[!0-9]*) true ;;
        *) break ;;
    esac
    i=$((i + 1))
    sleep 0.2
done

case "$types" in
    ''|*[!0-9]*)
        echo "portal-sync: desktop portal not reachable yet; skipping (no restart)."
        exit 0
        ;;
    0)
        echo "portal-sync: portal reports 0 source types -> restarting xdg-desktop-portal."
        systemctl --user restart xdg-desktop-portal.service
        sleep 1
        echo "portal-sync: after restart: $($READ_TYPES 2>/dev/null | awk '{print $NF}') source types."
        ;;
    *)
        echo "portal-sync: portal healthy ($types source types)."
        ;;
esac
exit 0