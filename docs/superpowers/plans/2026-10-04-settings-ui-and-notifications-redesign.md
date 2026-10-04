# Settings UI and Notification Engine Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Scythe settings UI into a clean, centered NVIDIA ShadowPlay-style split-panel modal with smooth direct numeric input fields and implement a zero-latency in-overlay toast animation system with graceful standalone toast handoff.

**Architecture:** Replace the legacy 3-column freestanding page stack in [src/overlay_egui.rs](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs) with a unified `ShadowPlayView::Settings(SettingsTab)` modal. Implement numeric inputs with bounds clamping and inline stepper buttons. Replace external toast process killing with in-overlay egui animation banners and graceful window messaging.

**Tech Stack:** Rust 2024 edition, egui / eframe 0.31, Win32 DWM / Windows COM, Wayland layer-shell.

**Spec:** [`docs/superpowers/specs/2026-10-04-settings-ui-and-notifications-redesign.md`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/docs/superpowers/specs/2026-10-04-settings-ui-and-notifications-redesign.md)

## Global Constraints

- Absolutely zero emojis across code, comments, logs, test outputs, and commits.
- Clickable markdown links formatted with the `file://` scheme.
- Complete conditional compilation safety across Linux and Windows.
- No blocking operations on UI rendering loops.

## Review Focus

1. Sub-1080p display clipping: Modal must adaptively scale below 850px viewport heights without overflowing the screen.
2. Direct numeric string parsing: Typing non-numeric characters in FPS, Bitrate, or Duration fields must not crash or corrupt settings state.
3. Rapid hotkey keycap binding: Entering key listener mode must capture multi-modifier shortcuts (`Ctrl+Shift+R`, `Alt+Z`) cleanly and allow cancellation with `Escape`.
4. Simultaneous in-overlay notifications: Multiple sequential alerts must reset duration or smoothly update without graphical popping.
5. Multi-monitor Windows geometry: Standalone toasts and modals must respect active monitor bounds when `monitor_x` is negative or non-zero.

---

### Task 1: Data Structures, Enums, and Numeric Input Component

**Files:**
- Modify: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)
- Test: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)

**Interfaces:**
- Produces:
  - `pub enum SettingsTab { Video, Audio, Shortcuts, ReplayAndStorage, Livestream, AppearanceAndSystem }`
  - `pub enum ShadowPlayView { MainHud, Settings(SettingsTab), Gallery }`
  - `pub fn numeric_input_box(ui: &mut egui::Ui, value_str: &mut String, val: &mut u32, min: u32, max: u32, step: u32, unit: &str, accent: Color32) -> bool`
  - `pub struct InOverlayToast { pub title: String, pub subtitle: String, pub icon: ToastIcon, pub created_at: Instant, pub duration: Duration }`

- [ ] **Step 1: Write unit tests for numeric input bounds and Tab enums**

In `src/overlay_egui.rs` tests module:

```rust
#[test]
fn test_numeric_input_clamping_and_validation() {
    let mut val = 60u32;
    let mut text = "60".to_string();
    assert!((15..=360).contains(&val));

    // Test text update
    text = "144".to_string();
    if let Ok(parsed) = text.trim().parse::<u32>() {
        val = parsed.clamp(15, 360);
    }
    assert_eq!(val, 144);

    // Test out of bounds clamping
    text = "999".to_string();
    if let Ok(parsed) = text.trim().parse::<u32>() {
        val = parsed.clamp(15, 360);
    }
    assert_eq!(val, 360);
}
```

- [ ] **Step 2: Run test to verify it passes or fails**

Run: `cargo test --lib test_numeric_input_clamping_and_validation`

- [ ] **Step 3: Define `SettingsTab` and `numeric_input_box` in `src/overlay_egui.rs`**

Implement:
- `SettingsTab` enum with `Video`, `Audio`, `Shortcuts`, `ReplayAndStorage`, `Livestream`, `AppearanceAndSystem`.
- `numeric_input_box` rendering a dark input box with `[-]` and `[+]` stepper buttons and unit label.
- Add `active_tab: SettingsTab` and `in_overlay_toast: Option<InOverlayToast>` to `ScytheOverlayApp`.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --all-targets`

- [ ] **Step 5: Commit**

```bash
git add src/overlay_egui.rs
git commit -m "feat(ui): add SettingsTab enum, InOverlayToast struct, and numeric input component"
```

---

### Task 2: Unified Centered ShadowPlay Settings Modal Layout

**Files:**
- Modify: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)
- Test: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)

**Interfaces:**
- Consumes: `SettingsTab`, `ScytheOverlayApp`, `accent_color()`.
- Produces: `render_settings_modal(&mut self, ctx: &egui::Context, ui: &mut egui::Ui)`.

- [ ] **Step 1: Write layout geometry test for settings modal**

In `src/overlay_egui.rs` tests module:

```rust
#[test]
fn test_settings_modal_geometry_bounds() {
    for (screen_w, screen_h) in [(1280.0, 720.0), (1920.0, 1080.0), (2560.0, 1440.0), (3840.0, 2160.0)] {
        let is_compact_h = screen_h < 850.0;
        let modal_w = if screen_w < 1400.0 { 680.0 } else { 760.0 };
        let modal_h = if is_compact_h { 450.0 } else { 520.0 };
        let pos_x = (screen_w - modal_w) * 0.5;
        let pos_y = (screen_h - modal_h) * 0.5;

        assert!(pos_x >= 0.0);
        assert!(pos_y >= 0.0);
        assert!(pos_x + modal_w <= screen_w);
        assert!(pos_y + modal_h <= screen_h);
    }
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --lib test_settings_modal_geometry_bounds`

- [ ] **Step 3: Implement `render_settings_modal`**

- Centered modal container (`modal_w x modal_h`) with obsidian glass fill `#101418` and top accent line.
- Left sidebar (`190px` width) with vertical tab buttons for `Video`, `Audio`, `Shortcuts`, `Replay & Storage`, `Livestream`, `Appearance & System`.
- Active tab highlight with accent pill border and crisp text.
- Compact "Reset to Defaults" button and version tag at sidebar footer.
- Close button (`X`) and "Done" button at top-right.
- Clicking outside or pressing Escape returns to `ShadowPlayView::MainHud`.

- [ ] **Step 4: Run compiler check**

Run: `cargo check --all-targets`

- [ ] **Step 5: Commit**

```bash
git add src/overlay_egui.rs
git commit -m "feat(ui): implement centered ShadowPlay settings modal with tab navigation"
```

---

### Task 3: Category Settings Panels Implementation

**Files:**
- Modify: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)

**Interfaces:**
- Produces:
  - `render_tab_video`
  - `render_tab_audio`
  - `render_tab_shortcuts`
  - `render_tab_replay_storage`
  - `render_tab_livestream`
  - `render_tab_appearance_system`

- [ ] **Step 1: Implement `render_tab_video`**
  - Numeric input box for Target Framerate (`15` to `360` FPS) with `[-]` and `[+]` steppers.
  - Numeric input box for Recording Bitrate (`5` to `150` Mbps).
  - Codec selector (`H.264`, `HEVC`, `AV1`).
  - Toggle switch for "Record mouse cursor".

- [ ] **Step 2: Implement `render_tab_audio`**
  - Source selector (`System Audio`, `Microphone`, `Both`, `Muted`).
  - Sliders for System Volume (0-100%) and Mic Volume (0-100%).
  - Live stereo VU meters with soft-limit indicators.

- [ ] **Step 3: Implement `render_tab_shortcuts`**
  - Interactive cards for `Menu Overlay`, `Save Replay`, `Start/Stop Record`, `Toggle Cursor`.
  - Click to listen for key combinations, with instant rebind and Esc to cancel.

- [ ] **Step 4: Implement `render_tab_replay_storage`**
  - Numeric input box for Replay Buffer Duration (`5` to `1800` seconds).
  - Calculated RAM footprint badge.
  - Path input with "Browse" button (invoking native Windows COM `IFileOpenDialog` / Linux portal) and "Open Folder".

- [ ] **Step 5: Implement `render_tab_livestream` and `render_tab_appearance_system`**
  - Service dropdown (`Twitch`, `YouTube`, `Kick`, `Custom RTMP`), URL, and masked stream key field.
  - 8 accent color chips with instant preview.
  - Startup toggles ("Start replay on login", "Open overlay on login").
  - In-app version checker with "Check for Updates" button.

- [ ] **Step 6: Run verification and tests**

Run: `cargo check --all-targets && cargo test`

- [ ] **Step 7: Commit**

```bash
git add src/overlay_egui.rs
git commit -m "feat(ui): implement clean settings tab panels with numeric input boxes"
```

---

### Task 4: In-Overlay Notification Animation Engine & Standalone Toast Overhaul

**Files:**
- Modify: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)
- Test: [`src/overlay_egui.rs`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/src/overlay_egui.rs)

**Interfaces:**
- Produces:
  - `ScytheOverlayApp::show_hud_notification` (queues in-process `InOverlayToast`)
  - `render_in_overlay_toast(&mut self, ctx: &egui::Context, ui: &mut egui::Ui)`
  - Overhauled `run_egui_toast` with cubic ease-out animation

- [ ] **Step 1: Write test for cubic ease-out calculation**

```rust
#[test]
fn test_cubic_ease_out_progress() {
    let ease_out = |t: f32| -> f32 {
        let inv = 1.0 - t.clamp(0.0, 1.0);
        1.0 - inv * inv * inv
    };
    assert_eq!(ease_out(0.0), 0.0);
    assert_eq!(ease_out(1.0), 1.0);
    assert!(ease_out(0.5) > 0.5); // Fast start, gentle deceleration
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --lib test_cubic_ease_out_progress`

- [ ] **Step 3: Implement `render_in_overlay_toast`**
  - Paints floating notification banner in top-right corner of overlay.
  - `0.0s – 0.22s`: Slide in from right viewport margin using cubic ease-out.
  - `0.22s – 2.4s`: Fully visible with icon, bold title, subtitle, and discharging accent progress line.
  - `2.4s – 2.7s`: Smooth alpha fade-out to zero.
  - Renders directly in the egui viewport with zero process spawning overhead.

- [ ] **Step 4: Refactor `run_egui_toast` standalone window**
  - Uses the same cubic ease-out curve and ShadowPlay aesthetic card.
  - Positioned at `(active_mon_x + active_mon_w - 364, active_mon_y + 24)`.
  - Clean automatic exit after 2.7s duration.

- [ ] **Step 5: Verify compiler and tests**

Run: `cargo check --all-targets && cargo test`

- [ ] **Step 6: Commit**

```bash
git add src/overlay_egui.rs
git commit -m "feat(ui): implement in-overlay toast animation engine and overhaul standalone toasts"
```

---

### Task 5: Integration, End-to-End Verification, and Build Deployment

**Files:**
- Modify: [`TODO_CHECKLIST.md`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/TODO_CHECKLIST.md)
- Test: [`tests/e2e_test.sh`](file:///home/oiupoyt/my%20stuff%20yea/projects/scythe/tests/e2e_test.sh)

- [ ] **Step 1: Run clippy and check across all targets**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 0 warnings, 0 errors.

- [ ] **Step 2: Run full unit test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Run end-to-end integration test**

Run: `bash tests/e2e_test.sh`
Expected: Recording and replay succeed without corruption.

- [ ] **Step 4: Compile release binaries and install locally**

Run: `cargo build --release && bash install.sh`
Expected: Fresh binaries deployed to `~/.local/bin/` and daemon restarted.

- [ ] **Step 5: Commit final changes**

```bash
git add TODO_CHECKLIST.md
git commit -m "chore: update maintainer checklist with settings modal and notification overhaul"
```
