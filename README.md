# Scythe

> High-performance, zero-copy GPU hardware screen recorder and instant replay overlay for Linux (Wayland & X11) and Windows.

[![Release](https://img.shields.io/github/v/release/oiupoyt/scythe?style=flat-square)](https://github.com/oiupoyt/scythe/releases)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue?style=flat-square)](LICENSE)

Scythe is a native, ultra-lightweight alternative to NVIDIA ShadowPlay and GPU Screen Recorder. It records gameplay and desktop screens with near-zero CPU and GPU overhead by leveraging hardware video acceleration and zero-copy direct memory pipelines.

---

## Features

- **Instant Replay Buffer**: Constantly records gameplay into a high-speed in-memory circular ring buffer. Save the last 15s to 5 minutes of action instantly with one hotkey (`Ctrl+Shift+R`) without writing temporary files to disk.
- **Zero-Copy Capture Engine**:
  - **Wayland**: PipeWire DMA-BUF frame-passing directly to hardware encoder surfaces.
  - **X11**: XCB Shared Memory (MIT-SHM) capture with per-frame pool reuse.
  - **Windows**: Desktop Duplication API via Direct3D 11 (DXGI) texture binding.
- **Hardware-Accelerated Encoding**:
  - NVIDIA NVENC (`h264_nvenc`, `hevc_nvenc`)
  - AMD & Intel VAAPI (`h264_vaapi`, `hevc_vaapi`)
  - Automatic CPU fallback (`libx264`)
- **ShadowPlay-Style Stealth Overlay**:
  - Native GTK 3 Layer-Shell overlay with real background blur for Wayland compositors (Hyprland, Sway, etc.).
  - Cross-platform hardware-rendered `egui` overlay for Windows, GNOME, KDE, and X11.
  - Translucent frosted glass HUD dock, centered controls, and contextual dropdowns.
- **Dynamic Hyprland Integration**: Automatically registers overlay blur rules and binds hotkeys at runtime without modifying `hyprland.conf`.
- **Multi-Channel Audio Routing**: Record system audio, microphone, or both simultaneously with individual volume controls via native PulseAudio/PipeWire and WASAPI.

---

## Default Shortcuts

| Action | Shortcut | CLI Command |
|---|---|---|
| Open / Close HUD Overlay | `Alt + Z` | `scythe-ui --menu` |
| Save Instant Replay | `Ctrl + Shift + R` | `scythe-ui --save` |
| Toggle Normal Recording | `Ctrl + Shift + F9` | `scythe-ui --record` |
| Toggle Mouse Cursor | `Ctrl + Shift + F10` | `scythe-ui --cursor` |

---

## Installation

### Arch Linux (AUR)

```bash
git clone https://aur.archlinux.org/scythe-git.git
cd scythe-git
makepkg -si
```

### Pre-Built Packages (Releases)

Download the latest packages directly from [GitHub Releases](https://github.com/oiupoyt/scythe/releases):

- **Debian / Ubuntu / Linux Mint**: `scythe_0.1.0_amd64.deb`
- **Fedora / RHEL / openSUSE**: `scythe-0.1.0-2.x86_64.rpm`
- **Universal Linux**: `scythe-x86_64.AppImage`
- **Windows**: `scythe-setup.exe`

### Build from Source

#### Prerequisites

- Rust (1.80+)
- FFmpeg 6+ / 7+ development libraries (`libavcodec`, `libavformat`, `libavutil`, `libswresample`, `libswscale`)
- PipeWire (`libpipewire-0.3-dev`)
- GTK 3 and GTK Layer Shell (`libgtk-3-dev`, `libgtk-layer-shell-dev`)

#### Compile & Install

```bash
git clone https://github.com/oiupoyt/scythe.git
cd scythe
bash install.sh
```

---

## CLI Usage

```bash
scythe-ui --menu        # Open the interactive overlay UI menu
scythe-ui --status      # Query background daemon state and metrics
scythe-ui --save        # Save instant replay clip to disk
scythe-ui --record      # Toggle normal recording on/off
scythe-ui --cursor      # Toggle mouse cursor capture
scythe-ui --reload      # Reload daemon settings
scythe-ui --quit        # Stop background recording daemon
```

---

## Configuration

Settings are saved in JSON format at `~/.config/scythe/config.json`:

```json
{
  "fps": 60,
  "record_bitrate_kbps": 18000,
  "replay_duration_sec": 60,
  "video_codec": "h264",
  "output_directory": "~/Videos/Scythe",
  "show_cursor": true,
  "audio_mode": "system",
  "mic_volume": 0.6,
  "system_volume": 1.0,
  "menu_hotkey": "Alt+Z",
  "save_hotkey": "Ctrl+Shift+R",
  "record_hotkey": "Ctrl+Shift+F9",
  "cursor_hotkey": "Ctrl+Shift+F10"
}
```

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
