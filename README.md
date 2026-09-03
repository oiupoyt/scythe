# ⚡ vrec

> **High-performance GPU hardware screen recorder & instant replay overlay for Linux (Wayland & X11) and Windows.**
>
> *Engineered specifically for mid- and low-end PCs to deliver high-FPS, zero-lag gameplay recording without macroblock pixelation or dropped frames.*

---

## 🚀 Key Features

* **🏎️ True Zero-Copy GPU Pipeline:**
  * **Wayland:** Linux PipeWire + DMA-BUF file descriptors imported straight into VAAPI without copying through system RAM.
  * **X11:** Low-CPU MIT-SHM (`xcb::shm`) shared memory architecture (zero socket IPC serialization).
  * **Windows:** DXGI Desktop Duplication (`IDXGIOutput1`) keeping textures 100% in GPU VRAM with zero PCIe bus readback.
* **🎮 In-Game HUD Overlay (Xbox Game Bar / GPU Screen Recorder Style):**
  * Borderless, frosted-glass top-centered HUD.
  * **Zero separate windows:** 100% inline settings stack so tiling window managers (Hyprland, Sway) never tile it.
  * Instant Replay toggle with 1-click Save button.
  * Live ticking recording timer (`[02:14]`) synced directly with the background daemon over IPC.
  * Microphone & audio live mute/unmute toggle.
* **💾 Ultra-Lightweight Instant Replay Buffer:**
  * Compressed H.264/AAC packets stored in a rolling RAM ringbuffer.
  * 60 seconds of instant replay uses only **~110 MB of RAM**.
* **💎 Studio-Grade Quality (No "Slop" or Macroblocks):**
  * Tuned VBR rate control with `1.5x` burst headroom for rapid camera turns.
  * Enforced quantization floor and ceiling (`qmin=16`, `qmax=28`) and H.264 High Profile.
  * Accurate container timestamp rescaling (`av_packet_rescale_ts`) locked to clean 60.0 fps.
* **🎛️ Tailored for Low-End Rigs:**
  * **30 FPS / 60 FPS / 120 FPS Selector:** Drop to 30 FPS to cut encoder workload by 50% on older hardware.
  * **Custom Save Directories:** Defaulting to `~/Videos/vrec` or any external hard drive.
  * **Audio Device Selector:** Choose between your system default or specific USB headset/mic.
  * **Codec Selector:** H.264 / AVC, HEVC / H.265, and AV1.
* **🌐 Universal GPU Support:**
  * **NVIDIA:** NVENC hardware acceleration.
  * **AMD:** VAAPI & AMF hardware acceleration.
  * **Intel:** QuickSync & VAAPI hardware acceleration.
  * **CPU Fallback:** Multi-threaded `libx264` if no hardware encoder device is found.

---

## 📥 Installation

### Arch Linux (AUR)
```bash
cd packaging/aur
makepkg -si
```

### From Source (Release Installer)
```bash
git clone https://github.com/oiupoyt/vrec.git
cd vrec
./install.sh
```
This compiles the release binaries and installs `vrec-daemon` and `vrec-ui` into `~/.local/bin/`.

---

## 🎮 Compositor Keybindings

### Hyprland (`~/.config/hypr/hyprland.conf`)
```ini
# Start the recording engine automatically on login
exec-once = vrec-daemon

# Hotkeys for HUD Overlay and Instant Replay
bind = ALT, Z, exec, vrec-ui --menu
bind = CONTROL SHIFT, R, exec, vrec-ui --save
bind = CONTROL SHIFT, F9, exec, vrec-ui --toggle
```

### Sway (`~/.config/sway/config`)
```ini
exec vrec-daemon
bindsym Mod1+z exec vrec-ui --menu
bindsym Control+Shift+r exec vrec-ui --save
```

---

## ⌨️ Command-Line Usage

You can control `vrec` directly from scripts, stream decks, or terminal:

```bash
vrec-daemon           # Starts background recording engine (run once)
vrec-ui --menu        # Toggles the in-game HUD overlay
vrec-ui --save        # Saves the rolling replay buffer immediately
vrec-ui --toggle      # Starts or stops normal recording
vrec-ui --start       # Starts recording
vrec-ui --stop        # Stops recording and finalizes MP4
vrec-ui --reload      # Reloads configuration from disk
vrec-ui --quit        # Shuts down the background daemon cleanly
```

---

## ⚙️ Configuration (`~/.config/vrec/config.json`)

Settings can be changed graphically in the HUD overlay or edited in `config.json`:

```json
{
  "replay_enabled": true,
  "replay_duration_sec": 60,
  "replay_bitrate_kbps": 18000,
  "record_enabled": false,
  "record_bitrate_kbps": 20000,
  "fps": 60,
  "video_codec": "h264",
  "output_directory": "/home/user/Videos/vrec",
  "audio_device": "default",
  "autostart": false,
  "language": "en",
  "ui_color_theme": "dark",
  "save_hotkey": "Ctrl+Shift+R",
  "menu_hotkey": "Alt+Z"
}
```

---

## 🧪 Automated Testing

`vrec` includes an automated integration test suite that tests recording, replay slicing, and file verification:

```bash
./tests/e2e_test.sh
```

---

## 📜 License
Licensed under MIT or Apache-2.0.
