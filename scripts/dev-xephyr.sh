#!/bin/sh
cd "$(dirname "$0")/.." || exit 1

Xephyr :2 -screen 1280x720 -ac &
trap 'kill $! 2>/dev/null' EXIT

waited=0
while [ ! -e /tmp/.X11-unix/X2 ] && [ "$waited" -lt 50 ]; do
    sleep 0.1
    waited=$((waited + 1))
done
if [ ! -e /tmp/.X11-unix/X2 ]; then
    echo "xephyr did not start" >&2
    exit 1
fi

DISPLAY=:2 RUST_LOG=debug cargo run
