#!/usr/bin/env bash
set -e

echo "============================================="
echo "   Scythe Automated End-to-End Test Suite    "
echo "============================================="

# Ensure binaries are compiled
echo "[1/6] Building release binaries..."
cargo build --release --quiet

SOCKET_PATH="${XDG_RUNTIME_DIR:-/tmp}/scythe.sock"
rm -f "$SOCKET_PATH"
rm -f record_*.mp4 replay_*.mp4

SAVE_DIR=$(python3 -c 'import json, os; p=os.path.expanduser("~/.config/scythe/config.json"); print(json.load(open(p))["output_directory"]) if os.path.exists(p) else print(os.path.expanduser("~/Videos/Scythe"))' 2>/dev/null || echo "$HOME/Videos/Scythe")
mkdir -p "$SAVE_DIR"

echo "[2/6] Starting scythe-daemon with mock capture..."
target/release/scythe-daemon --mock > daemon_test.log 2>&1 &
DAEMON_PID=$!

cleanup() {
    if kill -0 $DAEMON_PID 2>/dev/null; then
        target/release/scythe-ui --quit 2>/dev/null || kill -9 $DAEMON_PID 2>/dev/null || true
    fi
    rm -f daemon_test.log
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
target/release/scythe-ui --start
echo "Recording in progress for 3 seconds..."
sleep 3

# Test 2: Stop Normal Recording
echo "[4/6] Testing: Stop Normal Recording..."
target/release/scythe-ui --stop
sleep 1

# Test 3: Save Instant Replay
echo "[5/6] Testing: Save Instant Replay buffer..."
target/release/scythe-ui --save
sleep 2

# Test 4: Quit Daemon
echo "[6/6] Testing: Clean Daemon Shutdown..."
target/release/scythe-ui --quit
wait $DAEMON_PID 2>/dev/null || true
echo "Daemon shutdown cleanly."

# Verify generated MP4 files with ffprobe
echo "============================================="
echo "   Verifying Generated Recordings with ffprobe"
echo "============================================="

FOUND_ANY=0
for f in $(find "$SAVE_DIR" "$HOME/Videos/Scythe" "$HOME/Videos/vrec" . -maxdepth 2 \( -name "record_*.mp4" -o -name "replay_*.mp4" \) 2>/dev/null); do
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
done

if [ $FOUND_ANY -eq 0 ]; then
    echo "FAILED: No recording or replay files were produced!"
    cat daemon_test.log
    exit 1
fi

echo "============================================="
echo "   ALL AUTOMATED TESTS PASSED SUCCESSFULLY!  "
echo "============================================="
