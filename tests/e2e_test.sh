#!/usr/bin/env bash
set -e

echo "============================================="
echo "   Scythe Automated End-to-End Test Suite    "
echo "============================================="

# Ensure binaries are compiled
echo "[1/6] Building release binaries..."
cargo build --release --quiet

SOCKET_PATH="${XDG_RUNTIME_DIR:-/tmp}/scythe.sock"

# Liveness probe with no side effects. `scythe --status` calls
# ensure_daemon_running(), so probing with it spawns a daemon and the check
# starts chasing its own tail.
daemon_alive() {
    [ -S "$SOCKET_PATH" ] || return 1
    python3 -c '
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(0.5)
try:
    s.connect(sys.argv[1])
except OSError:
    sys.exit(1)
finally:
    s.close()
' "$SOCKET_PATH" 2>/dev/null
}

# A live daemon owns this socket. Removing the path entry while it runs leaves the
# kernel socket bound but unreachable, breaking the user's session until restart.
# Stop it gracefully, remember how it ran, and put it back when the test finishes.
PREVIOUS_DAEMON_CMD=""
PREVIOUS_DAEMON_CWD=""
if daemon_alive; then
    PREVIOUS_DAEMON_CMD=$(pgrep -af 'scythe.*daemon' 2>/dev/null | head -1 | cut -d' ' -f2- || true)
    PREVIOUS_DAEMON_CWD=$(readlink -f /proc/"$(pgrep -f 'scythe.*daemon' 2>/dev/null | head -1)"/cwd 2>/dev/null || true)
    echo "[0/6] Existing daemon detected; stopping it for the test run..."
    target/release/scythe --quit >/dev/null 2>&1 || true
    for _ in $(seq 1 80); do
        daemon_alive || break
        sleep 0.1
    done
    if daemon_alive; then
        echo "ERROR: A scythe daemon is running and did not stop cleanly."
        echo "       Refusing to unlink its socket. Stop it first: scythe --quit"
        exit 1
    fi
fi

rm -f "$SOCKET_PATH"
rm -f record_*.mp4 replay_*.mp4

SAVE_DIR=$(python3 -c 'import json, os; p=os.path.expanduser("~/.config/scythe/config.json"); print(os.path.expanduser(json.load(open(p))["output_directory"])) if os.path.exists(p) else print(os.path.expanduser("~/Videos/Scythe"))' 2>/dev/null || echo "$HOME/Videos/Scythe")
mkdir -p "$SAVE_DIR"

WAS_SERVICE_ACTIVE=0
if systemctl --user is-active --quiet scythe-daemon.service 2>/dev/null; then
    WAS_SERVICE_ACTIVE=1
    systemctl --user stop scythe-daemon.service 2>/dev/null || true
    sleep 0.5
fi

echo "[2/6] Starting scythe daemon with mock capture..."
target/release/scythe daemon --mock > daemon_test.log 2>&1 &
DAEMON_PID=$!

cleanup() {
    if kill -0 $DAEMON_PID 2>/dev/null; then
        target/release/scythe --quit 2>/dev/null || kill -9 $DAEMON_PID 2>/dev/null || true
    fi
    rm -f daemon_test.log
    if [ "$WAS_SERVICE_ACTIVE" -eq 1 ]; then
        systemctl --user start scythe-daemon.service 2>/dev/null || true
    elif [ -n "$PREVIOUS_DAEMON_CMD" ]; then
        echo "Restoring the daemon that was running before the test..."
        ( cd "${PREVIOUS_DAEMON_CWD:-$PWD}" && setsid $PREVIOUS_DAEMON_CMD >/dev/null 2>&1 & ) || true
        sleep 1
    fi
}
trap cleanup EXIT

# Wait for socket to become available
echo "Waiting for daemon socket..."
for i in {1..30}; do
    if [ -S "$SOCKET_PATH" ]; then
        break
    fi
    sleep 0.1
done

if [ ! -S "$SOCKET_PATH" ]; then
    echo "ERROR: Daemon IPC socket was not created!"
    cat daemon_test.log
    exit 1
fi
echo "Daemon is live (PID: $DAEMON_PID, Socket: $SOCKET_PATH)"

# Test 1: Start Normal Recording
echo "[3/6] Testing: Start Normal Recording..."
target/release/scythe --start
echo "Recording in progress for 3 seconds..."
sleep 3

# Test 2: Stop Normal Recording
echo "[4/6] Testing: Stop Normal Recording..."
target/release/scythe --stop
sleep 1

# Test 3: Save Instant Replay
echo "[5/6] Testing: Save Instant Replay buffer..."
target/release/scythe --save
sleep 2

# Test 4: Quit Daemon
echo "[6/6] Testing: Clean Daemon Shutdown..."
target/release/scythe --quit
wait $DAEMON_PID 2>/dev/null || true
echo "Daemon shutdown cleanly."

# Verify generated MP4 files with ffprobe
echo "============================================="
echo "   Verifying Generated Recordings with ffprobe"
echo "============================================="

FOUND_ANY=0
while IFS= read -r -d '' f; do
    if [ -f "$f" ]; then
        FOUND_ANY=1
        echo "--> Testing file: $f ($(ls -lh "$f" | awk '{print $5}'))"
        
        # Check container and stream information
        PROBE_OUT=$(ffprobe -v error -show_entries format=duration,size,bit_rate:stream=codec_name,width,height,r_frame_rate -of default=noprint_wrappers=1 "$f")
        echo "$PROBE_OUT"
        
        # Check for stream decode errors
        ffmpeg -v error -i "$f" -f null - 2>&1
        echo "File $f verified: VALID & NO CORRUPTION!"
        echo "---------------------------------------------"
        rm -f "$f"
    fi
done < <(find "$SAVE_DIR" "$HOME/Videos/Scythe" "$HOME/Videos/vrec" . -maxdepth 2 -mmin -2 \( -iname "record*.mp4" -o -iname "replay*.mp4" \) -print0 2>/dev/null)

if [ $FOUND_ANY -eq 0 ]; then
    echo "FAILED: No recording or replay files were produced!"
    cat daemon_test.log
    exit 1
fi

echo "============================================="
echo "   ALL AUTOMATED TESTS PASSED SUCCESSFULLY!  "
echo "============================================="
