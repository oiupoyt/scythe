# Design Specification: Scythe Settings UI and Notification Engine Redesign

## 1. Overview & Objective

This specification details the comprehensive redesign of the Settings User Interface and Notification Engine in [src/overlay_egui.rs](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs). The objective is to replace the legacy 3-column freestanding settings pages and fragmented external toast process spawning with:

1. **A Unified NVIDIA ShadowPlay-Inspired Settings Modal**:
   - Centered, frosted-glass modal (`780px x 520px`) with split-panel layout.
   - Left-hand category sidebar (`Video`, `Audio`, `Shortcuts`, `Replay & Storage`, `Livestream`, `Appearance & System`).
   - Clean, direct numeric input boxes for Bitrate, Framerate, and Replay Duration (with inline unit tags, instant validation, and increment/decrement steppers), replacing cluttered pill grids and sliders.
   - Live calculated buffer metrics and instant folder browser buttons.
2. **Fluid, Multi-Tier Notification Engine**:
   - In-Overlay Notification Banner: Rendered in-process by egui with zero process spawn latency and cubic ease-out animations.
   - Standalone Toast Window: Clean, single-window presentation on active monitors across Linux and Windows with graceful lifecycle teardown.
3. **Cross-Platform Zero-Defect Architecture**:
   - Full parity and bug-free execution across Linux (Wayland layer-shell, X11) and Windows 10/11 (DWM per-pixel glass transparency, multi-monitor geometry detection).

---

## 2. Architecture & Component Decomposition

### 2.1 UI State Machine & Views
In [src/overlay_egui.rs](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs), replace the disjointed view variants (`ReplaySettings`, `RecordSettings`, `StreamSettings`, `GlobalSettings`, `ScreenshotSettings`) with a cohesive modal state:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowPlayView {
    MainHud,
    Settings(SettingsTab),
    Gallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Video,
    Audio,
    Shortcuts,
    ReplayAndStorage,
    Livestream,
    AppearanceAndSystem,
}
```

### 2.2 Modal Window Geometry & Styling
- **Modal Dimensions**: Width `780.0px`, Height `520.0px` (adaptively scaled on viewports with height `< 850.0px` to `680.0px x 450.0px`).
- **Dimmed Backdrop**: Dimming scrim covering the screen (`rgba(0, 0, 0, 0.70)` on Linux, transparent backbuffer on Windows with DWM glass). Clicking outside the modal or pressing `Escape` saves settings and returns cleanly to `MainHud`.
- **Card Framing**:
  - Fill: Obsidian Glass `Color32::from_rgb(16, 20, 24)`.
  - Border: 1px subtle stroke `Color32::from_rgba_unmultiplied(255, 255, 255, 18)`.
  - Accent Line: Top 3px highlight band colored by user's active theme accent.

### 2.3 Left-Hand Sidebar Navigation (Width `200.0px`)
Displays a vertical stack of category buttons:
- Tab item height: `38.0px`, rounded corners (`CornerRadius::same(4)`).
- Active tab styling: Accent-tinted background (`rgba(accent, 0.20)`), 1.5px accent left border pill, and bold white text.
- Inactive tab styling: Transparent background with `#94A3B8` text, shifting to `#CBD5E1` on hover.
- Footer controls: Compact secondary button: "Reset to Defaults" (`height: 30.0px`) and version identifier ("Scythe v0.2.17").

### 2.4 Right-Hand Content Panel (Width `540.0px`)
Displays the content for the currently active `SettingsTab`:

#### Tab 1: Video Settings
- **Target Framerate (FPS)**:
  - Numeric input box with unit `FPS`. Range `15` to `360`. Stepper buttons `[-]` and `[+]` by 5/10 steps.
- **Recording Video Bitrate**:
  - Numeric input box with unit `Mbps`. Range `5` to `150`.
- **Video Codec**:
  - Clean horizontal segment pills `[ H.264 ]`, `[ HEVC (H.265) ]`, `[ AV1 ]`.
- **Mouse Cursor Recording**:
  - Sleek toggle switch: "Record mouse cursor".

#### Tab 2: Audio Settings
- **Audio Capture Source**:
  - Segment pills `[ System Audio ]`, `[ Microphone ]`, `[ Both ]`, `[ Muted ]`.
- **Microphone Selection**:
  - Dropdown populated from system input devices.
- **Volume Inputs & Controls**:
  - System Volume (`0% – 100%`) and Microphone Volume (`0% – 100%`) with numeric fields and live visual VU meters.

#### Tab 3: Shortcuts Settings
- **Hotkeys Cards**:
  - Menu Overlay (Default: `Alt+Z`)
  - Save Instant Replay (Default: `Ctrl+Shift+R`)
  - Start/Stop Recording (Default: `Ctrl+Shift+F9`)
  - Toggle Mouse Cursor (Default: `Ctrl+Shift+F10`)
- **Key Listening Mode**:
  - Displays interactive keycap badge. When clicked, enters listening state; pressing any key combination captures and re-binds instantly. Pressing `Escape` cancels without modifying.

#### Tab 4: Replay & Storage Settings
- **Replay Buffer Duration**:
  - Numeric input box with unit `seconds`. Range `5` to `1800` (up to 30 minutes).
  - Displays human-readable label: e.g. `60 sec (1 min 00 sec)`.
- **Estimated RAM Footprint**:
  - Calculated dynamically: `(seconds * bitrate_mbps * 1000) / 8192 MB`.
- **Save Location**:
  - Path text field with "Browse" button (invokes Windows COM `IFileOpenDialog` or Linux portal) and "Open" folder button.

#### Tab 5: Livestream Settings
- **Streaming Service**: Dropdown for `Twitch`, `YouTube`, `Kick`, `Custom RTMP`.
- **Server URL & Stream Key**:
  - Stream URL text input.
  - Stream Key masked input with eye toggle to unmask.

#### Tab 6: Appearance & System Settings
- **Accent Color Theme**:
  - 8 palette chips (AMD Red, NVIDIA Green, Intel Blue, Charming Blue, Emerald Green, Solar Yellow, Royal Purple, Neon Pink).
- **Startup Integration**:
  - Toggles for "Launch background replay engine on login" and "Launch HUD overlay on login".
- **Application Updater**:
  - Status display and "Check for Updates" trigger invoking [src/updater.rs](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/updater.rs).

---

## 3. Notification Animation & Rendering Architecture

### 3.1 In-Overlay Notification System (Overlay Open)
- Stored as an optional field in `ScytheOverlayApp`:
  ```rust
  pub struct InOverlayToast {
      pub title: String,
      pub subtitle: String,
      pub icon: ToastIcon,
      pub created_at: Instant,
      pub duration: Duration,
  }
  ```
- **Lifecycle & Easing**:
  - `0.0s – 0.22s`: Slide in from right viewport edge (`x` offset interpolated with `cubic_ease_out`).
  - `0.22s – 2.4s`: Fully visible top-right card with accent progress bar discharging to 0.
  - `2.4s – 2.7s`: Alpha fade out to `0.0`.
  - Handled directly in egui frame drawing with zero additional OS windows or process spawns.

### 3.2 Standalone Floating Toast System (Overlay Closed)
- Invoked via `scythe-ui --toast <title> <subtitle> <icon>`.
- Displays a dedicated single-purpose viewport card (`340px x 56px`):
  - Borderless, per-pixel transparent background, positioned at `(active_mon_x + active_mon_w - 364, active_mon_y + 24)`.
  - Icon on left, bold title, message, and subtle accent bottom border.
  - Smooth 2.6-second duration with auto-shutdown.
  - Replaces process hard-kills with clean window messages (`WM_CLOSE` on Windows, graceful `SIGTERM` settling on Linux).

---

## 4. Verification & Testing Plan

1. **Unit Tests**:
   - Numeric input bounds validation (FPS 15-360, Bitrate 5-150, Replay 5-1800).
   - Layout geometry assertions ensuring modal cards fit across 720p, 1080p, 1440p, and 4K displays.
   - Color palette mapping and theme serialization.
2. **Integration Verification**:
   - Verify settings persistence to `~/.config/scythe/config.json`.
   - Verify daemon dynamic reload on parameter changes.
   - Verify in-overlay notification display and standalone toast execution.
3. **Compiler Checks**:
   - `cargo check --all-targets` with zero errors.
   - `cargo clippy --all-targets -- -D warnings` with zero warnings.
   - `cargo test` with all tests passing.
