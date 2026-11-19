#!/usr/bin/env bash
# Start/stop the debug CoolDesk build on test port 4546 for live CLI checks.
#   app.sh start | stop
# The app binary must be built with: cargo build --features tauri/custom-protocol
# (a plain debug build loads localhost:5173 and shows WebView2's error page).
set -u
EXE="$(cd "$(dirname "$0")/../../src-tauri/target/debug" && pwd)/cooldesk.exe"
RUN="${TMPDIR:-/tmp}/cooldesk-p-run"
mkdir -p "$RUN"
case "${1:-}" in
  start)
    # Redirect the whole backgrounded subshell, not just the exe: otherwise the
    # subshell keeps the caller's stdout pipe open as long as CoolDesk runs.
    ( cd "$RUN" && COOLDESK_SIDECAR_PORT=4546 exec "$EXE" ) </dev/null >/dev/null 2>&1 &
    for i in $(seq 1 60); do
      curl -s -m 1 http://127.0.0.1:4546/health >/dev/null && { echo "app up on 4546 after ${i} tries"; exit 0; }
      sleep 0.5
    done
    echo "app did NOT come up"; exit 1 ;;
  stop)
    cd "$RUN" && COOLDESK_SIDECAR_PORT=4546 "$EXE" --quit >/dev/null 2>&1
    sleep 4
    tasklist | grep -qi cooldesk && echo "WARN: cooldesk still running" || echo "no cooldesk process"
    netstat -ano | grep -qE ":4546 .*LISTEN" && echo "WARN: 4546 still listening" || echo "4546 free" ;;
  *) echo "usage: app.sh start|stop"; exit 2 ;;
esac
