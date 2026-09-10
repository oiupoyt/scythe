#!/usr/bin/env bash
set -e

echo "============================================="
echo "   Installing Scythe Screen Recorder         "
echo "============================================="

# Compile release binaries
echo "[1/4] Compiling release binaries..."
cargo build --release

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"

mkdir -p "$BIN_DIR"
mkdir -p "$APP_DIR"

echo "[2/4] Installing binaries to $BIN_DIR..."
install -m 755 target/release/scythe-daemon "$BIN_DIR/scythe-daemon"
install -m 755 target/release/scythe-ui "$BIN_DIR/scythe-ui"
ln -sf "$BIN_DIR/scythe-ui" "$BIN_DIR/scythe"

# Backward-compatible symlinks
ln -sf "$BIN_DIR/scythe-daemon" "$BIN_DIR/vrec-daemon"
ln -sf "$BIN_DIR/scythe-ui" "$BIN_DIR/vrec-ui"
ln -sf "$BIN_DIR/scythe-ui" "$BIN_DIR/vrec"

SYSTEMD_USER_DIR="$HOME/.config/systemd/user"
if [ -f "$SYSTEMD_USER_DIR/vrec-daemon.service" ] || systemctl --user is-active --quiet vrec-daemon.service 2>/dev/null; then
    echo "Migrating legacy vrec-daemon systemd service to scythe-daemon.service..."
    systemctl --user stop vrec-daemon.service 2>/dev/null || true
    systemctl --user disable vrec-daemon.service 2>/dev/null || true
    rm -f "$SYSTEMD_USER_DIR/vrec-daemon.service"
    rm -f "$SYSTEMD_USER_DIR/default.target.wants/vrec-daemon.service"
fi

if systemctl --user is-active --quiet scythe-daemon.service 2>/dev/null; then
    echo "Restarting active scythe-daemon systemd service..."
    systemctl --user restart scythe-daemon.service
elif pgrep -x scythe-daemon > /dev/null || pgrep -x vrec-daemon > /dev/null; then
    echo "Restarting active scythe-daemon engine..."
    pkill -x scythe-daemon 2>/dev/null || true
    pkill -x vrec-daemon 2>/dev/null || true
    sleep 0.5
    nohup "$BIN_DIR/scythe-daemon" > /dev/null 2>&1 &
    disown
fi

echo "[3/4] Creating application launcher..."
cat > "$APP_DIR/scythe.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Scythe Screen Recorder
Comment=Fast GPU Hardware Screen Recorder & Instant Replay
Exec=scythe-ui --menu
Icon=media-record
Terminal=false
Categories=AudioVideo;Recorder;
Keywords=screen;recorder;replay;capture;scythe;shadowplay;
EOF

ln -sf "$APP_DIR/scythe.desktop" "$APP_DIR/vrec.desktop"

echo "[4/4] Verifying PATH..."
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo "Notice: $BIN_DIR is not in your PATH. Consider adding:"
    echo '  export PATH="$HOME/.local/bin:$PATH"'
fi

echo "============================================="
echo "   Scythe successfully installed!            "
echo "============================================="
echo "You can now run:"
echo "  scythe-daemon         # Starts background recording engine"
echo "  scythe-ui --menu      # Opens the HUD overlay menu"
echo "  scythe-ui --record    # Toggles normal recording"
echo "  scythe-ui --save      # Saves instant replay"
echo ""
echo "Tip: You can bind 'scythe-ui --menu' or 'scythe-ui --save' in Hyprland config (hyprland.conf):"
echo '  bind = ALT, Z, exec, scythe-ui --menu'
echo '  bind = CONTROL SHIFT, R, exec, scythe-ui --save'
echo "============================================="
