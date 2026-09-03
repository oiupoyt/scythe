#!/usr/bin/env bash
set -e

echo "============================================="
echo "   Installing vrec Screen Recorder           "
echo "============================================="

# Compile release binaries
echo "[1/4] Compiling release binaries..."
cargo build --release

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"

mkdir -p "$BIN_DIR"
mkdir -p "$APP_DIR"

echo "[2/4] Installing binaries to $BIN_DIR..."
install -m 755 target/release/vrec-daemon "$BIN_DIR/vrec-daemon"
install -m 755 target/release/vrec-ui "$BIN_DIR/vrec-ui"

echo "[3/4] Creating application launcher..."
cat > "$APP_DIR/vrec.desktop" << EOF
[Desktop Entry]
Type=Application
Name=vrec Screen Recorder
Comment=Fast GPU Hardware Screen Recorder & Instant Replay
Exec=vrec-ui --menu
Icon=media-record
Terminal=false
Categories=AudioVideo;Recorder;
Keywords=screen;recorder;replay;capture;
EOF

echo "[4/4] Verifying PATH..."
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo "Notice: $BIN_DIR is not in your PATH. Consider adding:"
    echo '  export PATH="$HOME/.local/bin:$PATH"'
fi

echo "============================================="
echo "   vrec successfully installed!              "
echo "============================================="
echo "You can now run:"
echo "  vrec-daemon           # Starts background recording engine"
echo "  vrec-ui --menu        # Opens the HUD overlay menu"
echo "  vrec-ui --record      # Toggles normal recording"
echo "  vrec-ui --save        # Saves instant replay"
echo ""
echo "Tip: You can bind 'vrec-ui --menu' or 'vrec-ui --save' in Hyprland config (hyprland.conf):"
echo '  bind = ALT, Z, exec, vrec-ui --menu'
echo '  bind = CONTROL SHIFT, R, exec, vrec-ui --save'
echo "============================================="
