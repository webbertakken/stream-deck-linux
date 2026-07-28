#!/usr/bin/env bash
#
# Development loop: always run the latest build of `streamdeck tray`.
#
# On every source change this rebuilds `--release`, stops the previously running
# instance (so two daemons never fight over the single HID device or the fixed
# web port), and relaunches the tray in the foreground.
#
# Usage:
#   scripts/dev.sh          # watch src/ and assets/, rebuild + restart on change
#   scripts/dev.sh --once   # build once, replace the running instance, and run
#
# The autostarted instance (native `.desktop`) must NOT run at the same time:
# this loop stops any running `streamdeck` before launching its own.

set -euo pipefail

cd "$(dirname "$0")/.."

# Absolute path so the launched process cmdline and the pgrep pattern match.
BIN="$(pwd)/target/release/streamdeck"

# Stop every running streamdeck instance we can find, matched precisely on the
# built binary path so we never kill an unrelated process. Returns once they
# are gone.
stop_running() {
  local self=$$
  local pids
  pids=$(pgrep -f "$BIN tray" || true)
  for pid in $pids; do
    [ "$pid" = "$self" ] && continue
    echo "dev: stopping running streamdeck (pid $pid)"
    kill "$pid" 2>/dev/null || true
  done
  # Give the old instance a moment to release the HID device and web port.
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -f "$BIN tray" >/dev/null 2>&1 || break
    sleep 0.2
  done
}

build_and_run() {
  echo "dev: building --release"
  cargo build --release
  stop_running
  echo "dev: launching $BIN tray"
  exec "$BIN" tray
}

if [ "${1:-}" = "--once" ]; then
  build_and_run
fi

if ! command -v cargo-watch >/dev/null 2>&1; then
  echo "dev: 'cargo watch' is required but not installed." >&2
  echo "dev: install it with: cargo install cargo-watch" >&2
  exit 1
fi

echo "dev: watching src/ and assets/ (Ctrl-C to stop)"
# `-s` runs this script in --once mode on every change; cargo watch restarts it,
# which stops the previous tray and launches the freshly built one.
exec cargo watch --no-gitignore -w src -w assets -s "bash scripts/dev.sh --once"
