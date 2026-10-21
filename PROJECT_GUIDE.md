# Scythe: Comprehensive Project & Architecture Guide

This document is the definitive technical reference for developers and AI agents working on the Scythe codebase. It outlines the project's design philosophy, unified binary architecture, hardware encoding pathways, audio subsystem, power management, HUD overlay, and development standards.

---

## 1. Project Overview & Current Status

**Current Version:** `v0.2.18`  
**License:** Dual MIT / Apache 2.0  

### Repository Topology

| Remote | URL | Role | Status |
|---|---|---|---|
| `forgejo` | `https://git.oiupoyt.space/oiupoyt/scythe.git` | Self-hosted primary; source of truth, release assets | Active |
| `origin` | `https://oiupoyt@github.com/oiupoyt/scythe.git` | **GitHub primary** (the `oiupoyt` account) | Active |
| `spearzificial` | `https://spearzificial@github.com/spearzificial/scythe.git` | GitHub mirror under the `spearzificial` account | Active |
| `gitlab` | `https://gitlab.com/oiupoyt.cat/scythe.git` | Optional mirror | Optional / stale |

**GitHub account policy:** two GitHub accounts are authenticated in the credential store, `oiupoyt` and `spearzificial`, and both have a repository for this project. `origin` is the `oiupoyt` repository; `spearzificial` is the mirror. Always name the account in the remote URL (`https://oiupoyt@github.com/...`, `https://spearzificial@github.com/...`) so Git selects the matching stored credential instead of guessing between them.

The two GitHub repositories started as separate histories. They diverged once, on the commit that added the LICENSE files, which was authored independently on both sides with byte-identical content. They are now joined by a merge commit, so a push to either one is a fast-forward from the other.

### Dual-Push Rule

Every push must reach every active remote:

```bash
git push origin main
git push forgejo main
git push spearzificial main
```


Scythe is a cross-platform, ultra-low-overhead desktop screen recorder, instant replay buffer, and livestreamer written in Rust. It delivers functionality comparable to NVIDIA ShadowPlay and GPU Screen Recorder with minimal CPU and GPU overhead.

### Core Capabilities:
1. **Unified Multicall Binary Architecture**: UI overlay, CLI client, and headless daemon engine are consolidated into a single executable ([`src/main.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/main.rs)) with subcommands (`scythe daemon`, `scythe record`, `scythe save`, etc.) and backward-compatible symlinks (`scythe-daemon`, `scythe-ui`).
2. **Zero-Copy Hardware Video Encoding**:
   - **Linux**: FFmpeg VA-API (`h264_vaapi`, `hevc_vaapi`, `av1_vaapi`) with direct DRM DMA-BUF import for Intel and AMD GPUs. Automatic low-power hardware fallback handles drivers lacking `VAEntrypointEncSliceLP`.
   - **Windows**: Native NVENC, AMD AMF, Intel QuickSync (QSV), and multithreaded software fallbacks via DirectX Graphics Infrastructure (DXGI) desktop duplication.
3. **High-Fidelity Audio Engine**:
   - Multi-source routing: System Audio, Microphone, Both, or Muted.
   - Dual codec support: Native Opus (`AV_CODEC_ID_OPUS`, 48 kHz stereo, 160 kbps, `AV_SAMPLE_FMT_FLT`) as recommended default, with native AAC (`AV_CODEC_ID_AAC`, 48 kHz stereo, 192 kbps, `AV_SAMPLE_FMT_FLTP`) as universal fallback.
   - Interactive sound verification suite with live dB / percentage VU meters and test tone playback.
4. **Lock-Free Circular RAM Replay Buffer**:
   - Continuous lock-free packet ring buffer in system RAM (configurable from 5 seconds to 30 minutes).
   - Keyframe-aware draining ensures saved MP4 clips begin cleanly on IDR boundaries with zero decoder corruption.
5. **Smart Power Management**:
   - AC power connectivity monitoring via Linux `/sys/class/power_supply` and Windows Win32 `GetSystemPowerStatus`.
   - Dedicated 1-second background power poller thread.
   - "Power-only" replay mode automatically drains and pauses the replay buffer on battery power to conserve battery life, and seamlessly resumes with keyframe forced resync upon AC connection.
6. **KAT Square Minimalist Interface**:
   - Strictly conforms to `DESIGN.md` guidelines: `border-radius: 0 !important;` across all windows, inputs, buttons, and badges.
   - Obsidian dark palette (`#0b0b0b` / `#111111`) with hairline `#222222` framing and restrained functional accents.
   - Borderless, shadowless terminal-style notification toasts.
   - Monospace typography option with integrated `Adwaita Mono` font.
7. **Screenshot Resolution Presets**:
   - Intuitive presets: `Native (Source)`, `1080p (FHD)`, `1440p (QHD)`, `2160p (4K UHD)`, `720p (HD)` with hardware-accelerated Lanczos scaling via FFmpeg.
8. **In-App Auto-Updater**:
   - Synchronizes against Forgejo releases API (`https://git.oiupoyt.space/api/v1/repos/oiupoyt/scythe/releases/latest`) with automated binary replacement.

---

## 2. High-Level Architecture & Process Model

Scythe uses a unified multicall binary dispatch model:

```
                         +-----------------------------------+
                         |           scythe binary           |
                         +-----------------+-----------------+
                                           |
               Inspects argv[0] and first CLI argument
                                           |
         +---------------------------------+---------------------------------+
         |                                                                   |
         v                                                                   v
   "scythe daemon"                                                   "scythe [command]"
  or argv[0] == "scythe-daemon"                                     or argv[0] == "scythe-ui"
         |                                                                   |
+--------v-------------------------+                               +---------v-----------------------+
|    Headless Daemon Engine        |                               |      Client CLI & Overlay       |
|      (src/daemon.rs)             | <--- Unix Domain Socket / --- |       (src/overlay_egui.rs)     |
| - Screen Capture (PipeWire/DXGI) |      Local TCP IPC Loop       | - egui HUD Overlay (Alt+Z)      |
| - Hardware Video Encoder         |                               | - Audio Diagnostics & VU Meters |
| - Native Opus/AAC Audio Encoder  |                               | - Clip Trimmer & Gallery        |
| - Lock-Free Circular Ring Buffer |                               | - Terminal Notification Toasts  |
| - AC Power Watcher Thread        |                               | - Global Hotkey Listener Pump   |
| - MP4 Muxer & RTMP Streamer      |                               +---------------------------------+
+----------------------------------+
```

### Multicall Dispatching:
- **Daemon Mode**: Invoked when `args[1] == "daemon"`, `args[1] == "--mock"`, or when executed via symlink `scythe-daemon` / `vrec-daemon`. Runs the headless capture and recording engine ([`src/daemon.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/daemon.rs)).
- **Dispatch uses argv[0], not `current_exe()`**: `current_exe()` reads `/proc/self/exe`, which resolves symlinks to the real file. Every installed alias would therefore report `scythe` and dispatch to the client, so `scythe-daemon` would open the HUD instead of starting the engine. `invoked_stem()` reads argv[0] and falls back to `current_exe()` only when argv[0] has no usable stem.
- **Client / UI Mode**: Invoked when executed as `scythe`, `scythe-ui`, or with client arguments (`save`, `record`, `screenshot`, `status`, `menu`, etc.). Runs [`src/main.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/main.rs) and launches [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs).

---

## 3. Inter-Process Communication (IPC)

The protocol is implemented in [`src/ipc.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/ipc.rs).

### Transport Layer:
- **Linux**: Unix Domain Socket at `$XDG_RUNTIME_DIR/scythe.sock` (with legacy fallback to `/run/user/<UID>/vrec.sock`).
- **Windows**: Local TCP socket on `127.0.0.1` binding to candidate ports `42069`–`42072` and persisting the active port in `%TEMP%\scythe-ipc.port`.

### Message Payload:
Packets are framed with a 4-byte little-endian length prefix followed by JSON-serialized structs:
- **Command (`Command`)**:
  - `SaveReplay`: Drains the circular buffer and muxes an MP4 replay.
  - `StartRecording` / `StopRecording` / `ToggleRecording`: Controls manual recording.
  - `StartStreaming` / `StopStreaming` / `ToggleStreaming`: Controls RTMP broadcasting.
  - `ToggleAudio`: Toggles microphone/system audio mute state.
  - `CycleAudioMode`: Cycles through System -> Mic -> Both -> Muted.
  - `ToggleCursor`: Inverts cursor visibility state and triggers capture synchronization.
  - `ShowOverlay`: Opens the HUD overlay via the daemon if it is not already running (`scythe --show-overlay`). Distinct from `--menu`, which toggles an already-open overlay closed.
  - `ReloadConfig`: Live-reloads configuration changes without restarting the daemon unless video parameters change.
  - `PowerStateChanged(bool)`: Internal notification dispatched by the power poller when AC state transitions.
  - `GetStatus`: Requests the current `DaemonStatus`.
  - `StopDaemon`: Triggers clean daemon shutdown.
- **DaemonStatus (`DaemonStatus`)**:
  - `is_recording`: bool
  - `recording_duration_sec`: u64
  - `is_replay_active`: bool
  - `audio_muted`: bool
  - `audio_mode`: String ("system", "mic", "both", "muted")
  - `show_cursor`: bool
  - `mic_volume` / `system_volume`: f32
  - `mic_level_peak` / `system_level_peak`: f32 (0.0 to 1.0 real-time peak values)
  - `is_streaming`: bool
  - `streaming_duration_sec`: u64

---

## 4. Video Capture Subsystem

Capture sources implement the [`FrameSource`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/mod.rs#L21) trait located in [`src/capture/mod.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/mod.rs).

### Frame Representation:
```rust
pub enum Frame {
    Raw {
        width: u32,
        height: u32,
        stride: u32,
        data: Arc<Vec<u8>>,
    },
    DmaBuf {
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        stride: u32,
        format: u32,
        modifier: u64,
        offset: u32,
    },
    D3D11Texture {
        handle: usize,
        width: u32,
        height: u32,
    },
}
```

### Platform Implementations:
1. **Wayland ([`src/capture/wayland.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/wayland.rs) & [`src/capture/wayland_stream.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/wayland_stream.rs))**:
   - Uses `xdg-desktop-portal` and PipeWire screencast stream.
   - Negotiates DMA-BUF memory buffers (`SPA_DATA_DmaBuf`) for zero-copy GPU video encoding.
   - Falls back to shared memory buffers (`SPA_DATA_MemPtr`) when DMA-BUF is unavailable.
2. **X11 ([`src/capture/x11.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/x11.rs))**:
   - Uses MIT-SHM (`xcb::shm`) for direct shared-memory frame transfer from the X server.
3. **Windows ([`src/capture/windows.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/windows.rs))**:
   - Uses the DirectX Graphics Infrastructure (DXGI) Desktop Duplication API (`IDXGIOutputDuplication`).
   - Iterates through all display adapters and outputs to probe and select the active desktop.
   - Uses a single Direct3D 11 staging texture (`D3D11_USAGE_STAGING`) with `D3D11_CPU_ACCESS_READ` for low-latency GPU readback.
   - Tightly packs rows into BGRA format if GPU pitch padding is detected.
   - Ensures fallback / idle frames generate opaque black (`[0, 0, 0, 255]`) to prevent uninitialized green chroma artifacts.
4. **Mock Capture ([`src/capture/mock.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/mock.rs))**:
   - Generates simulated 1080p frames for headless unit tests and CI runners where no display server is attached.

---

## 5. Video Encoding Pipeline

The video encoder module is located in [`src/encoder.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/encoder.rs).

### Linux Encoder (`VaapiEncoder`):
- Uses FFmpeg libavcodec with VA-API hardware acceleration (`h264_vaapi`, `hevc_vaapi`, `av1_vaapi`).
- Direct DRM DMA-BUF Import: Zero-copy surface creation via `vaCreateSurfacesFromProperties` for Intel and AMD GPUs.
- Color Standards: ITU-R BT.709 color primaries (`AVCOL_PRI_BT709`), transfer characteristics (`AVCOL_TRC_BT709`), and MPEG limited range (`AVCOL_RANGE_MPEG`).
- SIMD Color Space Conversion: When software fallback is required, `sws_scale` operates with `SWS_FAST_BILINEAR` and explicit ITU-709 coefficient mapping.

### Windows Encoder (`WindowsHwEncoder`):
- Hardware Probing Priority:
  - H.264: `h264_nvenc` -> `h264_amf` -> `h264_qsv` -> `libx264`
  - HEVC: `hevc_nvenc` -> `hevc_amf` -> `hevc_qsv` -> `libx265`
  - AV1: `av1_nvenc` -> `av1_amf` -> `av1_qsv` -> `libsvtav1`
- Standardized Pixel Format: NV12 (`AV_PIX_FMT_NV12`) across hardware encoders, with YUV420P fallback.
- Color Conversion: Converts captured BGRA to NV12 using `sws_scale` configured with ITU-R BT.709 matrix and full-range to limited-range mapping (`sws_setColorspaceDetails`), eliminating washed-out colors and green screen bugs.
- Low Latency Settings: Rate control VBR with burst ceiling, zero B-frames (`max_b_frames = 0`), ultrafast tuning, and 1-second GOP size (`gop_size = fps`).

---

## 6. Audio Subsystem

Audio capture and conversion is implemented in [`src/capture/audio.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/capture/audio.rs) and [`src/encoder.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/encoder.rs).

### Pipeline:
1. **Capture**:
   - **Linux**: Uses PulseAudio/PipeWire `parec` targeting `@DEFAULT_MONITOR@` (for system audio loopback) and `@DEFAULT_SOURCE@` (for microphone input) with a 20ms low-latency buffer.
   - **Windows**: Direct WASAPI loopback capture using the `cpal` crate.
2. **Channel Downmixing & Resampling**:
   - Downmixes 5.1 and 7.1 surround sound to stereo using center and surround coefficients:
     ```
     L = FL + 0.707 * C + 0.707 * SL + 0.5 * LFE
     R = FR + 0.707 * C + 0.707 * SR + 0.5 * LFE
     ```
   - Resamples arbitrary input sample rates to 48,000 Hz using linear interpolation and soft-clipping limiting (`tanh` curve).
3. **Encoding (`AudioEncoder`)**:
   - **Opus (`AV_CODEC_ID_OPUS`)**: Native Opus stereo at 160 kbps, 48 kHz with float sample format (`AV_SAMPLE_FMT_FLT`). Primary recommended standard.
   - **AAC (`AV_CODEC_ID_AAC`)**: Native AAC stereo at 192 kbps, 48 kHz with planar float format (`AV_SAMPLE_FMT_FLTP`). Universal fallback standard.
   - Microsecond-accurate PTS rebasing aligns audio packets against video IDR keyframes.
4. **Interactive Verification & Live VU Meters**:
   - The UI includes live decibel / percentage VU meters for both system sound and microphone.
   - Integrated test suite (`test mic` and `test sys` with automated chime playback) allows users to immediately verify sound capture without running external applications.

---

## 7. Lock-Free RAM Ring Buffer & Power Management

Implemented in [`src/ring.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/ring.rs), [`src/daemon.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/daemon.rs), and [`src/config.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/config.rs).

### Buffer Mechanics:
- Keeps encoded video and audio packets in RAM without writing to disk during active buffer mode.
- Fixed Capacity: Capacity is calculated dynamically from duration (seconds) * target framerate * safety factor. On low-power hardware, capacity is tightly bounded to conserve memory.
- **Keyframe-Aware Pruning**:
  - Replay clips must always begin with an IDR keyframe to prevent visual corruption.
  - When the circular buffer reaches maximum capacity, packets are dropped starting from the oldest non-keyframe sequence up to the next IDR keyframe boundary.
- Drain Operation: When `Command::SaveReplay` is received, the buffer packets are cloned into an asynchronous channel without blocking the capture and encoding threads.

### AC Power Management:
- **Connectivity Detection**: `ScytheConfig::is_ac_power_connected()` inspects `/sys/class/power_supply/*/online` on Linux and calls `GetSystemPowerStatus` on Windows.
- **Poller Thread**: A dedicated 1-second background thread continuously monitors AC power changes.
- **Automatic Buffer Gating**: When `autostart_power_only` is enabled:
  - On battery power loss: The replay buffer is automatically paused, drained of existing frames, and a toast notification (`power lost, replay paused`) is dispatched.
  - On AC connection restore: Replay buffering automatically resumes, forces an encoder keyframe, and dispatches a notification (`power restored, replay resumed`).

---

## 8. Muxer & Livestreaming

Implemented in [`src/muxer.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/muxer.rs).

### MP4 Container Muxing (`Muxer`):
- Wraps FFmpeg libavformat to produce universal MP4 files (`.mp4`) containing H.264/HEVC/AV1 video and Opus/AAC audio.
- Emits faststart headers (`+faststart`) to place the `moov` atom at the beginning of the file for instant streaming and playback.
- Rebases PTS and DTS timestamps to zero for both video and audio streams at file initialization.
- Automatic Collision Handling: Formats filenames as `Replay-HH-MM-SS_DD-MM-YYYY.mp4` or `Recording-HH-MM-SS_DD-MM-YYYY.mp4` and resolves conflicts via incremental suffixes (`_1`, `_2`).

### RTMP Livestreaming (`spawn_stream_worker`):
- Connects to RTMP endpoints (Twitch, YouTube, Kick, Custom RTMP).
- Encapsulates video and audio NAL units into FLV/RTMP transport on a dedicated worker thread with non-blocking channel dispatch.

---

## 9. In-Game HUD Overlay & Notifications

Implemented in [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs).

### Design System: KAT Square Minimalist:
- Strictly adheres to `DESIGN.md`:
  - **`border-radius: 0 !important;`**: Zero rounded corners or pill badges anywhere in the interface.
  - **Obsidian Dark Palette**: `#0b0b0b` root background, `#111111` surface panels, `#222222` hairline borders.
  - **Restrained Accents**: Functional emerald (`#10b981`), red (`#ef4444`), cyan (`#38bdf8`), and vendor themes (AMD, NVIDIA, Intel).
  - **Monospace Typography**: Full support for `Adwaita Mono` / `JetBrains Mono` with an interface toggle.

### Views:
- **Main HUD**:
  - Rolldown / swipe animated dropdown menus (`ui.ctx().animate_bool(...)` over 180ms) for Replay, Record, and Broadcast.
  - Quick actions with drop shadow framing.
- **Settings View**:
  - File Information: Output directory with native folder pickers.
  - Video Parameters: Resolution scaling, FPS (15 to 360), bitrate (5 to 150 Mbps), codec (H.264, HEVC, AV1), color range (Full vs Limited), low-power recording mode.
  - Audio: Mode routing, volume sliders (0-150%), Opus/AAC codec choice, and live verification test suite.
  - Screenshot: Resolution presets (`Native`, `1080p`, `1440p`, `2160p`, `720p`), format (`PNG` vs `JPEG`), and cursor capture toggle.
  - General: Theme accents, 8 global keybinds with interactive recording listener, compact `Reset Hotkeys` button, and typography toggle.
- **Notification Toasts (`ShadowPlayToastApp`)**:
  - Borderless, shadowless terminal-style card.
  - Obsidian `#111111` fill with 2px vertical accent rule and 6px status dot.
  - Uppercase monospace title, lowercase metadata subtitle, and 2px hairline progress countdown line.

---

## 10. Global Hotkeys & Window Hooks

Implemented in [`src/hotkey.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/hotkey.rs) and [`src/hyprland_binds.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/hyprland_binds.rs).

### Keybind Registry:
1. Menu Overlay: `Alt+Z`
2. Save Instant Replay: `Ctrl+Shift+R`
3. Start / Stop Recording: `Ctrl+Shift+F9`
4. Pause / Resume Recording: `Ctrl+Shift+F8`
5. Take Screenshot: `Ctrl+Shift+S`
6. Toggle Mouse Cursor: `Ctrl+Shift+F10`
7. Mute / Unmute Microphone: `Ctrl+Shift+M`
8. Mute / Unmute System Audio: `Ctrl+Shift+N`

### Dispatch Strategies:
1. **Hyprland Direct IPC**: Registers binds dynamically using `hyprctl keyword bind` to bypass Wayland global hotkey restrictions. Watches `hyprland.conf` modifications and auto-reapplies.
2. **Universal Desktop**: Uses the `global-hotkey` crate for standard Windows and X11 hotkey registration.

---

## 11. Configuration Management

Implemented in [`src/config.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/config.rs).

### File Locations:
- Linux: `~/.config/scythe/config.json`
- Windows: `%APPDATA%\scythe\config.json`

### Autostart Integration:
`ScytheConfig::apply_autostart_system(enable_replay, enable_overlay)` manages:
1. Systemd user service: `systemctl --user enable/disable scythe-daemon.service`.
2. Standard XDG autostart desktop entries in `~/.config/autostart/scythe-daemon.desktop` and `scythe-overlay.desktop`.

Linux only. The Windows path is a no-op: there is no registry `Run` key write in either the overlay toggle or the NSIS installer, so the autostart switch in the UI currently has no effect on Windows.

### Retired Settings (v0.2.18 cleanup):
These keys were accepted by the settings UI and persisted to disk, but nothing in the capture, encoder, or daemon paths ever read them. They were removed from `ScytheConfig` and the corresponding UI controls were deleted, so the overlay no longer presents options that do nothing:

| Retired key | Why it was removed |
|---|---|
| `capture_source` | Capture always selected the focused monitor; window/region modes were never implemented |
| `replay_storage` | Replay buffer is always RAM-backed; the disk path was never built |
| `change_resolution`, `resolution_w`, `resolution_h` | Encoder always used native capture dimensions |
| `container_format` | Muxer always writes MP4 |
| `show_scroll_lock_led` | No LED driver integration existed |
| `autostart_on_game` | No game-detection layer existed |
| `save_folder_by_game`, `name_by_game` | No game detection; filenames are timestamp-based |
| `language`, `ui_color_theme` | No translation layer or theme engine; `accent_color` drives the real palette |
| `record_enabled` | Dead field superseded by runtime `is_recording` state |

Unknown keys in an existing `config.json` are ignored by serde, so upgrading does not break saved configurations. `scythe` rewrites the file without these keys on the next save.

---

## 12. In-App Auto-Updater

Implemented in [`src/updater.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/updater.rs).

### Workflow:
1. Queries the self-hosted Forgejo release API:
   `GET https://git.oiupoyt.space/api/v1/repos/oiupoyt/scythe/releases/latest`
   (Falls back to GitHub API if Forgejo is unreachable).
2. Parses SemVer strings and checks if `is_newer(latest, CURRENT_VERSION)`.
3. Downloads the singular platform package:
   - Linux: `scythe-v<version>-linux-x86_64.tar.gz`
   - Windows: `scythe-v<version>-windows-x86_64.zip` (includes standalone portable FFmpeg DLLs).
4. Atomically swaps the executable in `~/.local/bin/scythe` or `%USERPROFILE%\.local\bin\scythe.exe`.

---

## 13. Verification, Testing & Cross-Compilation

### Local Linux Commands:
```bash
# Type check all targets
cargo check --all-targets

# Run the complete unit test suite (27 tests)
cargo test

# Strict clippy lint verification
cargo clippy --all-targets -- -D warnings

# Build optimized Linux release binary
cargo build --release

# Install release binary and symlinks to ~/.local/bin
bash install.sh

# Run end-to-end integration test suite
bash tests/e2e_test.sh
```

`tests/e2e_test.sh` treats a running daemon as a resource it borrows rather than deletes. Unix sockets cannot be re-linked: removing the path entry of a live daemon leaves the kernel socket bound but unreachable, and the session stays broken until that process dies.

The script therefore:
1. probes liveness by connecting to the socket directly (`scythe --status` cannot be used for this, because it calls `ensure_daemon_running()` and would spawn a daemon while probing);
2. records the running daemon's command line and working directory;
3. sends `StopDaemon` and waits for the socket to disappear, aborting without unlinking if it will not stop;
4. restores that daemon on exit, so a developer's recorder survives a test run.

### Windows Cross-Compilation (from Linux Host):
```bash
FFMPEG_DIR="$HOME/.local/share/ffmpeg-win64/ffmpeg-n9.0-latest-win64-gpl-shared-9.0" \
PKG_CONFIG_PATH="$HOME/.local/share/ffmpeg-win64/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/lib/pkgconfig" \
PKG_CONFIG_ALLOW_CROSS=1 \
cargo build --release --target x86_64-pc-windows-gnu
```

### GitHub Actions:

| Workflow | Trigger | Purpose |
|---|---|---|
| [`ci.yml`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/.github/workflows/ci.yml) | push to `main`, pull request, manual | Compiles and tests both targets on every push. Linux job: `cargo check --all-targets`, `cargo test`, `cargo clippy -D warnings`, `cargo build --release`, then runs `scythe --help` to prove the binary starts. Windows job: MSYS2 MINGW64 `cargo build --release`. Both binaries upload as artifacts. |
| [`release.yml`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/.github/workflows/release.yml) | push of a `v*` tag | Packages `.tar.gz`, `.deb`, `.rpm`, `.AppImage` and the NSIS Windows installer, then publishes a GitHub Release. |

The CI workflow is the only thing that compiles the Windows target on a routine basis, and it exists because the Windows build had already rotted: `src/config.rs` calls `GetSystemPowerStatus` but the `windows` crate feature list never enabled `Win32_System_Power`, so `cargo build --target x86_64-pc-windows-gnu` failed with `E0432`. Tag pushes were the only previous trigger, so nothing caught it before a release was attempted.

The `cargo fmt --check` step is advisory (`continue-on-error: true`). The tree has formatting drift that is intentionally not mass-fixed, because reformatting every file would bury the real changes in a diff.

Tests that need real hardware (a live audio server) are opt-in:

```bash
SCYTHE_HW_TESTS=1 cargo test
```

---

## 14. Behavioral Rules & Coding Standards

1. **ABSOLUTELY ZERO EMOJIS**: Under no circumstances should emojis be added to code, comments, log outputs, commit messages, or communication.
2. **File & Symbol Links**: Every file path or code symbol mentioned in communication must be formatted as a clickable markdown link using the `file://` scheme (e.g., [`src/encoder.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/encoder.rs)).
3. **KAT Square Minimalist**: Strict `border-radius: 0 !important;` on all visual elements.
4. **Dual-Push Rule**: When pushing commits or tags, always push to every active remote (`origin` and `spearzificial` for GitHub, `forgejo` for self-hosted):
   ```bash
   git push origin <branch>
   git push spearzificial <branch>
   git push forgejo <branch>
   ```
5. **Memory Safety & Process Isolation**: Avoid blocking the audio or video capture loops. Use unbounded or high-capacity crossbeam channels for frame dispatch.
