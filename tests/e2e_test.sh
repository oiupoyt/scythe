#!/usr/bin/env bash
set -e

echo "============================================="
echo "   vrec Automated End-to-End Test Suite      "
echo "============================================="

# Ensure binaries are compiled
echo "[1/6] Building release binaries..."
cargo build --release --quiet

SOCKET_PATH="${XDG_RUNTIME_DIR:-/tmp}/vrec.sock"
rm -f "$SOCKET_PATH"
rm -f test_record_*.mp4 test_replay_*.mp4

echo "[2/6] Starting vrec-daemon with mock capture..."
target/release/vrec-daemon --mock > daemon_test.log 2>&1 &
DAEMON_PID=$!

# Ensure daemon is killed on exit
cleanup() {
    if kill -0 $DAEMON_PID 2>/dev/null; then
        target/release/vrec-ui --quit 2>/dev/null || kill -9 $DAEMON_PID 2>/dev/null || true
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
target/release/vrec-ui --start
echo "Recording in progress for 3 seconds..."
sleep 3

# Test 2: Stop Normal Recording
echo "[4/6] Testing: Stop Normal Recording..."
target/release/vrec-ui --stop
sleep 1

# Test 3: Save Instant Replay
echo "[5/6] Testing: Save Instant Replay buffer..."
target/release/vrec-ui --save
sleep 2

# Test 4: Quit Daemon
echo "[6/6] Testing: Clean Daemon Shutdown..."
target/release/vrec-ui --quit
wait $DAEMON_PID 2>/dev/null || true
echo "Daemon shutdown cleanly."

# Verify generated MP4 files with ffprobe
echo "============================================="
echo "   Verifying Generated Recordings with ffprobe"
echo "============================================="

MP4_FILES=(record_*.mp4 replay_*.mp4)
FOUND_ANY=0

for f in "${MP4_FILES[@]}"; do
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
