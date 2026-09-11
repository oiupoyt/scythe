use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Margin, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};
use crate::config::ScytheConfig;
use crate::ipc::{self, Command, DaemonStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowPlayView {
    MainHud,
    Settings,
    Gallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindAction {
    Menu,
    SaveReplay,
    ToggleRecord,
    ToggleCursor,
}

pub fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "modern_sans".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf"))),
    );
    fonts.families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "modern_sans".to_owned());
    ctx.set_fonts(fonts);
}

fn format_egui_key(key: egui::Key) -> Option<String> {
    match key {
        egui::Key::Escape => None,
        egui::Key::Space => Some("Space".to_string()),
        egui::Key::Tab => Some("Tab".to_string()),
        egui::Key::Enter => Some("Return".to_string()),
        egui::Key::Backspace => Some("BackSpace".to_string()),
        egui::Key::Insert => Some("Insert".to_string()),
        egui::Key::Delete => Some("Delete".to_string()),
        egui::Key::Home => Some("Home".to_string()),
        egui::Key::End => Some("End".to_string()),
        egui::Key::PageUp => Some("Page_Up".to_string()),
        egui::Key::PageDown => Some("Page_Down".to_string()),
        _ => {
            let name = format!("{:?}", key);
            if name.starts_with("Num") && name.len() > 3 {
                Some(name[3..].to_string())
            } else {
                Some(name)
            }
        }
    }
}

fn probe_duration_sec(path: &std::path::Path) -> f32 {
    let mut cmd = std::process::Command::new("ffprobe");
    cmd.args([
        "-v", "error",
        "-show_entries", "format=duration",
        "-of", "default=noprint_wrappers=1:nokey=1",
    ])
    .arg(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output();
    if let Ok(out) = out {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Ok(val) = text.trim().parse::<f32>() {
            return val;
        }
    }
    0.0
}

fn trim_clip(
    input_path: &std::path::Path,
    start_sec: f32,
    end_sec: f32,
) -> Result<PathBuf, String> {
    let stem = input_path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = input_path.extension().unwrap_or_default().to_string_lossy();
    let parent = input_path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut out_path = parent.join(format!("{}_trimmed.{}", stem, ext));
    let mut counter = 1;
    while out_path.exists() {
        out_path = parent.join(format!("{}_trimmed_{}.{}", stem, counter, ext));
        counter += 1;
    }

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-ss", &format!("{:.2}", start_sec),
        "-to", &format!("{:.2}", end_sec),
        "-i",
    ])
    .arg(input_path)
    .args([
        "-c", "copy",
        "-avoid_negative_ts", "make_zero",
    ])
    .arg(&out_path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let res = cmd.output().map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;
    if res.status.success() {
        Ok(out_path)
    } else {
        let err_str = String::from_utf8_lossy(&res.stderr);
        Err(format!("FFmpeg trim failed: {}", err_str.lines().last().unwrap_or("Unknown error")))
    }
}

fn play_clip(path: &std::path::Path) {
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use windows::core::HSTRING;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            let path_str = p.to_string_lossy().to_string();
            unsafe {
                let _ = ShellExecuteW(
                    None,
                    windows::core::w!("open"),
                    &HSTRING::from(&path_str),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if std::process::Command::new("mpv").arg(&p).spawn().is_err()
                && std::process::Command::new("vlc").arg(&p).spawn().is_err()
            {
                let _ = std::process::Command::new("xdg-open").arg(&p).spawn();
            }
        }
    });
}

fn render_vu_meter(ui: &mut egui::Ui, level: f32, width: f32, height: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let clamped = level.clamp(0.0, 1.0);

    let bg_color = Color32::from_rgb(14, 14, 16);
    let border_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 20));
    ui.painter().rect(rect, CornerRadius::ZERO, bg_color, border_stroke, egui::StrokeKind::Inside);

    let fill_w = (rect.width() * clamped).max(0.0);
    if fill_w > 0.5 {
        let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        let fill_color = if clamped > 0.85 {
            Color32::from_rgb(239, 68, 68)
        } else if clamped > 0.65 {
            Color32::from_rgb(234, 179, 8)
        } else {
            Color32::from_rgb(34, 197, 94)
        };
        ui.painter().rect_filled(fill_rect, CornerRadius::ZERO, fill_color);
    }

    if clamped > 0.05 {
        let tick_x = rect.left() + rect.width() * clamped;
        ui.painter().line_segment(
            [egui::pos2(tick_x, rect.top()), egui::pos2(tick_x, rect.bottom())],
            Stroke::new(1.5_f32, Color32::WHITE),
        );
    }

    if !label.is_empty() {
        ui.painter().text(
            rect.left_center() + Vec2::new(3.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(8.5),
            Color32::from_rgba_unmultiplied(255, 255, 255, 200),
        );
    }
}

#[derive(Debug, Clone)]
pub struct VideoClipInfo {
    pub filename: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified: SystemTime,
    pub is_replay: bool,
}

fn scan_recordings(dir_str: &str) -> Vec<VideoClipInfo> {
    let dir = ScytheConfig::expand_tilde(dir_str);
    let mut clips = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if ext_str == "mp4" || ext_str == "mkv" {
                        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let is_replay = filename.to_lowercase().starts_with("replay");
                        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                        clips.push(VideoClipInfo {
                            filename,
                            path,
                            size_bytes,
                            modified,
                            is_replay,
                        });
                    }
            }
        }
    }
    clips.sort_by_key(|b| std::cmp::Reverse(b.modified));
    clips
}

// Cross-platform helper to reveal or open directories
fn open_folder(path: &std::path::Path) {
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        let is_file = p.is_file();
        let folder = if is_file {
            p.parent().unwrap_or(&p).to_path_buf()
        } else {
            p.clone()
        };
        let _ = std::fs::create_dir_all(&folder);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            if is_file {
                let p_str = p.to_string_lossy().replace('/', "\\");
                let _ = std::process::Command::new("explorer.exe")
                    .arg(format!("/select,{}", p_str))
                    .creation_flags(0x08000000)
                    .spawn();
            } else {
                let f_str = folder.to_string_lossy().replace('/', "\\");
                let _ = std::process::Command::new("explorer.exe")
                    .arg(&f_str)
                    .creation_flags(0x08000000)
                    .spawn();
            }
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&folder).spawn();
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&folder).spawn();
        }
    });
}

// Cross-platform folder picker dialog
fn pick_folder(current_dir: &str, tx: Sender<String>, is_active: Arc<AtomicBool>) {
    is_active.store(true, Ordering::SeqCst);
    let cur = current_dir.to_string();
    let flag = is_active.clone();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let clean_cur = cur.replace('/', "\\").replace('\'', "''");
            let script = format!(
                "[System.Reflection.Assembly]::LoadWithPartialName('System.Windows.Forms') | Out-Null; \
                 $f = New-Object System.Windows.Forms.FolderBrowserDialog; \
                 $f.Description = 'Select Recordings Directory'; \
                 $f.UseDescriptionForTitle = $true; \
                 if (Test-Path '{}') {{ $f.SelectedPath = '{}' }}; \
                 if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $f.SelectedPath }}",
                clean_cur, clean_cur
            );
            let mut cmd = std::process::Command::new("powershell.exe");
            cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-STA", "-Command", &script]);
            cmd.creation_flags(0x08000000);
            if let Ok(out) = cmd.output() {
                let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sel.is_empty() {
                    let _ = tx.send(sel);
                }
            }
            flag.store(false, Ordering::SeqCst);
        }

        #[cfg(not(target_os = "windows"))]
        {
            // Dynamically ensure Hyprland floats, pins, and focuses the folder picker dialog on top
            let _ = std::process::Command::new("hyprctl")
                .args(["eval", r#"hl.window_rule({ match = { title = "Select Recordings Directory" }, float = true, pin = true, stay_focused = true, center = true })"#])
                .output();
            let _ = std::process::Command::new("hyprctl")
                .args(["eval", r#"hl.window_rule({ match = { class = "org.kde.kdialog" }, float = true, pin = true, stay_focused = true, center = true })"#])
                .output();
            let _ = std::process::Command::new("hyprctl")
                .args(["eval", r#"hl.window_rule({ match = { class = "kdialog" }, float = true, pin = true, stay_focused = true, center = true })"#])
                .output();

            if let Ok(out) = std::process::Command::new("kdialog")
                .args(["--title", "Select Recordings Directory", "--getexistingdirectory", &cur])
                .output()
            {
                let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sel.is_empty() {
                    let _ = tx.send(sel);
                    flag.store(false, Ordering::SeqCst);
                    return;
                }
            }
            if let Ok(out) = std::process::Command::new("zenity")
                .args(["--title=Select Recordings Directory", "--file-selection", "--directory", &format!("--filename={}", cur)])
                .output()
            {
                let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sel.is_empty() {
                    let _ = tx.send(sel);
                    flag.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }

        flag.store(false, Ordering::SeqCst);
    });
}

fn spawn_daemon_process() {
    let mut cmd = if let Ok(mut path) = std::env::current_exe() {
        path.pop();
        #[cfg(target_os = "windows")]
        let primary = path.join("scythe-daemon.exe");
        #[cfg(not(target_os = "windows"))]
        let primary = path.join("scythe-daemon");

        if primary.exists() {
            std::process::Command::new(primary)
        } else {
            #[cfg(target_os = "windows")]
            { std::process::Command::new("scythe-daemon.exe") }
            #[cfg(not(target_os = "windows"))]
            { std::process::Command::new("scythe-daemon") }
        }
    } else {
        #[cfg(target_os = "windows")]
        { std::process::Command::new("scythe-daemon.exe") }
        #[cfg(not(target_os = "windows"))]
        { std::process::Command::new("scythe-daemon") }
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

// Asynchronous daemon command dispatcher to avoid blocking the egui render loop
fn async_send_command(cmd: Command) {
    std::thread::spawn(move || {
        let _ = ipc::send_command(cmd);
    });
}

// Helper to render mechanical keyboard keycap badges
#[allow(dead_code)]
fn render_keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(18, 20, 26))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 26)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(7_i8, 3_i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(FontId::monospace(9.5))
                    .strong()
                    .color(Color32::from_rgb(220, 222, 228)),
            );
        });
}

pub fn resolve_accent_color(accent: &str) -> Color32 {
    match accent.to_lowercase().as_str() {
        "green" | "emerald" => Color32::from_rgb(34, 197, 94),
        "lime" => Color32::from_rgb(163, 230, 53),
        "yellow" | "solar" => Color32::from_rgb(250, 204, 21),
        "amber" | "orange" => Color32::from_rgb(245, 158, 11),
        "red" | "crimson" => Color32::from_rgb(239, 68, 68),
        "pink" | "rose" => Color32::from_rgb(244, 63, 94),
        "purple" | "violet" => Color32::from_rgb(168, 85, 247),
        "blue" | "sapphire" => Color32::from_rgb(56, 189, 248),
        _ => Color32::from_rgb(56, 189, 248),
    }
}

// Clickable keycap button for interactive rebinding
fn render_keycap_button(
    ui: &mut egui::Ui,
    text: &str,
    listening: bool,
    accent: Color32,
) -> bool {
    let (fill, stroke, text_color, label_text) = if listening {
        (
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 32),
            Stroke::new(1.5_f32, accent),
            accent,
            "PRESS KEYS...".to_string(),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(20, 22, 30, 210),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
            Color32::from_rgb(220, 220, 228),
            text.to_string(),
        )
    };

    let btn = egui::Button::new(
        egui::RichText::new(label_text)
            .font(FontId::monospace(10.5))
            .strong()
            .color(text_color),
    )
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::ZERO)
    .min_size(Vec2::new(130.0, 26.0));

    ui.add(btn).clicked()
}

// Modern sleek rounded button
fn squared_button(ui: &mut egui::Ui, text: &str, active: bool, accent: Color32) -> bool {
    let fill = if active {
        accent
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    };
    let stroke = if active {
        Stroke::new(1.0_f32, accent)
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 24))
    };
    let text_color = if active {
        Color32::from_rgb(10, 10, 12)
    } else {
        Color32::from_rgb(215, 215, 220)
    };
    let btn = egui::Button::new(egui::RichText::new(text).size(11.5).strong().color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::ZERO);
    ui.add(btn).clicked()
}

#[allow(dead_code)]
fn pill_button(ui: &mut egui::Ui, text: &str, active: bool, accent: Color32) -> bool {
    squared_button(ui, text, active, accent)
}

// Sleek Modern Switch Toggle
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, accent: Color32) -> egui::Response {
    let desired_size = egui::vec2(38.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, *on);
        let bg_color = if *on {
            accent
        } else {
            Color32::from_rgb(26, 28, 34)
        };
        let stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
        ui.painter().rect(rect, CornerRadius::ZERO, bg_color, stroke, egui::StrokeKind::Inside);
        let knob_w = rect.height() - 4.0;
        let knob_x = egui::lerp((rect.left() + 2.0)..=(rect.right() - knob_w - 2.0), how_on);
        let knob_rect = egui::Rect::from_min_size(egui::pos2(knob_x, rect.top() + 2.0), egui::vec2(knob_w, knob_w));
        ui.painter().rect_filled(knob_rect, CornerRadius::ZERO, Color32::WHITE);
    }
    response
}

// Minimal Squared Vector Icon Renderers (Clean, Sleek & Modern)
fn draw_replay_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_active: bool, accent: Color32) {
    let color = if is_active {
        accent
    } else {
        Color32::from_rgb(150, 150, 155)
    };

    // Dual Rewind Triangles ◀ ◀ without outer circle (crisp, prominent, centered)
    let tri_h = radius * 0.60;
    let tri_w = radius * 0.42;
    let gap = radius * 0.10;

    // Left triangle ◀ (pointing left)
    let t1_tip = center + Vec2::new(-tri_w - gap * 0.5, 0.0);
    let t1_top = center + Vec2::new(-gap * 0.5, -tri_h);
    let t1_bot = center + Vec2::new(-gap * 0.5, tri_h);
    painter.add(egui::Shape::convex_polygon(
        vec![t1_tip, t1_top, t1_bot],
        color,
        Stroke::NONE,
    ));

    // Right triangle ◀ (pointing left)
    let t2_tip = center + Vec2::new(gap * 0.5, 0.0);
    let t2_top = center + Vec2::new(tri_w + gap * 0.5, -tri_h);
    let t2_bot = center + Vec2::new(tri_w + gap * 0.5, tri_h);
    painter.add(egui::Shape::convex_polygon(
        vec![t2_tip, t2_top, t2_bot],
        color,
        Stroke::NONE,
    ));
}

fn draw_record_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_recording: bool, _anim_time: f32) {
    let half = radius * 0.88;
    let arm = radius * 0.36;

    if is_recording {
        let red_bright = Color32::from_rgb(239, 68, 68);

        // Viewfinder corner brackets (Red)
        let stroke = Stroke::new(2.2_f32, red_bright);
        // Top-left
        painter.line_segment([center + Vec2::new(-half + arm, -half), center + Vec2::new(-half, -half)], stroke);
        painter.line_segment([center + Vec2::new(-half, -half), center + Vec2::new(-half, -half + arm)], stroke);
        // Top-right
        painter.line_segment([center + Vec2::new(half - arm, -half), center + Vec2::new(half, -half)], stroke);
        painter.line_segment([center + Vec2::new(half, -half), center + Vec2::new(half, -half + arm)], stroke);
        // Bottom-left
        painter.line_segment([center + Vec2::new(-half + arm, half), center + Vec2::new(-half, half)], stroke);
        painter.line_segment([center + Vec2::new(-half, half), center + Vec2::new(-half, half - arm)], stroke);
        // Bottom-right
        painter.line_segment([center + Vec2::new(half - arm, half), center + Vec2::new(half, half)], stroke);
        painter.line_segment([center + Vec2::new(half, half), center + Vec2::new(half, half - arm)], stroke);

        // Center recording core (completely static, solid and crisp, no bobbing/pulsing)
        let core_r = radius * 0.42;
        painter.circle_filled(center, core_r, red_bright);
    } else {
        let frame_color = Color32::from_rgb(150, 150, 155);
        let core_color = Color32::from_rgb(203, 213, 225);

        // Viewfinder corner brackets (Slate)
        let stroke = Stroke::new(2.0_f32, frame_color);
        // Top-left
        painter.line_segment([center + Vec2::new(-half + arm, -half), center + Vec2::new(-half, -half)], stroke);
        painter.line_segment([center + Vec2::new(-half, -half), center + Vec2::new(-half, -half + arm)], stroke);
        // Top-right
        painter.line_segment([center + Vec2::new(half - arm, -half), center + Vec2::new(half, -half)], stroke);
        painter.line_segment([center + Vec2::new(half, -half), center + Vec2::new(half, -half + arm)], stroke);
        // Bottom-left
        painter.line_segment([center + Vec2::new(-half + arm, half), center + Vec2::new(-half, half)], stroke);
        painter.line_segment([center + Vec2::new(-half, half), center + Vec2::new(-half, half - arm)], stroke);
        // Bottom-right
        painter.line_segment([center + Vec2::new(half - arm, half), center + Vec2::new(half, half)], stroke);
        painter.line_segment([center + Vec2::new(half, half), center + Vec2::new(half, half - arm)], stroke);

        // Center standby dot
        painter.circle_filled(center, radius * 0.35, core_color);
    }
}

fn draw_settings_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    let track_h = radius * 1.05;
    let track_spacing = radius * 0.48;
    let track_stroke = Stroke::new(1.8_f32, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 140));

    // 3 vertical tracks
    let xs = [-track_spacing, 0.0, track_spacing];
    for &x_off in &xs {
        let x = center.x + x_off;
        painter.line_segment(
            [egui::pos2(x, center.y - track_h), egui::pos2(x, center.y + track_h)],
            track_stroke,
        );
    }

    // 3 slider knobs positioned at different heights for dynamic equalizer / settings look
    let knob_w = radius * 0.42;
    let knob_h = radius * 0.22;
    let knob_offsets = [
        (-track_spacing, -track_h * 0.35),
        (0.0, track_h * 0.40),
        (track_spacing, -track_h * 0.10),
    ];

    for (x_off, y_off) in knob_offsets {
        let knob_rect = egui::Rect::from_center_size(
            center + Vec2::new(x_off, y_off),
            Vec2::new(knob_w, knob_h),
        );
        painter.rect_filled(knob_rect, CornerRadius::ZERO, color);
    }
}

// Modern Sleek Action Card Renderer
#[allow(clippy::too_many_arguments)]
fn render_action_card(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    title: &str,
    is_active: bool,
    dropdown_open: bool,
    draw_icon: impl FnOnce(&egui::Painter, egui::Pos2),
    status_text: &str,
    status_color: Color32,
    sub_text: &str,
    accent: Color32,
) -> bool {
    let (raw_rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let rect = egui::Rect::from_min_size(
        egui::pos2(raw_rect.left().round(), raw_rect.top().round()),
        Vec2::new(width.round(), height.round()),
    );
    let hovered = response.hovered();
    let painter = ui.painter();
    // Neutral translucent black card fill (pure black, zero blue hue, zero accent fill)
    let bg = if dropdown_open {
        Color32::from_rgba_unmultiplied(18, 18, 18, 175)
    } else if hovered {
        Color32::from_rgba_unmultiplied(22, 22, 22, 180)
    } else {
        Color32::from_rgba_unmultiplied(10, 10, 10, 155)
    };

    painter.rect_filled(rect, CornerRadius::ZERO, bg);

    // Clean, crisp outlines on cards: prominent accent on hover/selected, subtle accent when idle
    let stroke = if dropdown_open {
        Stroke::new(1.5_f32, accent)
    } else if hovered {
        Stroke::new(1.5_f32, accent)
    } else if is_active {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 70))
    };

    painter.rect_stroke(rect, CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);

    // Card Title (clean modern sans typography)
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 25.0),
        egui::Align2::CENTER_CENTER,
        title,
        FontId::proportional(14.0),
        if is_active { accent } else { Color32::WHITE },
    );

    // Centered vector icon (no ambient fill circle)
    let icon_center = egui::pos2(rect.center().x, rect.top() + 85.0);
    draw_icon(painter, icon_center);

    // Status subtitle (e.g. "01:23" or "Not recording" / "Buffer 60s")
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 32.0),
        egui::Align2::CENTER_CENTER,
        status_text,
        FontId::proportional(11.5),
        status_color,
    );

    // Secondary hint
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 14.0),
        egui::Align2::CENTER_CENTER,
        sub_text,
        FontId::proportional(10.0),
        Color32::from_rgb(130, 135, 145),
    );

    response.clicked()
}

// Modern Sleek Translucent Dropdown Action Menu Container
fn render_dropdown_menu(
    ui: &mut egui::Ui,
    card_width: f32,
    accent: Color32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(6.0);
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(10, 10, 10, 185))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 140)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::ZERO)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.set_width(card_width);
            ui.set_min_width(card_width);
            ui.set_max_width(card_width);
            add_contents(ui);
        });
}

// Remade Dropdown Action Menu Item (Left-Aligned, Subtitles, Hotkey Badges, Left Indicator)
fn render_menu_item(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: Option<&str>,
    hotkey_hint: Option<&str>,
    is_active: bool,
    accent: Color32,
    is_last: bool,
) -> bool {
    let (raw_rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::click());
    let rect = egui::Rect::from_min_size(
        egui::pos2(raw_rect.left().round(), raw_rect.top().round()),
        Vec2::new(raw_rect.width().round(), 44.0),
    );
    let hovered = response.hovered();

    let bg = if hovered {
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 36)
    } else {
        Color32::TRANSPARENT
    };

    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);

    // Left vertical accent indicator bar on hover
    if hovered {
        let left_bar = egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height()));
        ui.painter().rect_filled(left_bar, CornerRadius::ZERO, accent);
    }

    // Left-aligned Title
    let title_color = if hovered {
        accent
    } else if is_active {
        Color32::WHITE
    } else {
        Color32::from_rgb(222, 226, 234)
    };

    let title_y = if subtitle.is_some() { rect.top() + 13.0 } else { rect.center().y };
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, title_y),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(12.5),
        title_color,
    );

    // Subtitle under Title
    if let Some(sub) = subtitle {
        ui.painter().text(
            egui::pos2(rect.left() + 14.0, rect.bottom() - 12.0),
            egui::Align2::LEFT_CENTER,
            sub,
            FontId::proportional(10.0),
            Color32::from_rgb(135, 140, 150),
        );
    }

    // Right-aligned Hotkey Badge (dynamically sized to fit user-configured hotkeys)
    if let Some(hk) = hotkey_hint {
        let badge_w = (hk.len() as f32 * 6.5 + 14.0).max(52.0);
        let badge_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - badge_w / 2.0 - 10.0, rect.center().y),
            Vec2::new(badge_w, 18.0),
        );
        ui.painter().rect_filled(
            badge_rect,
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(255, 255, 255, 14),
        );
        ui.painter().rect_stroke(
            badge_rect,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 22)),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            hk,
            FontId::monospace(9.0),
            if hovered { accent } else { Color32::from_rgb(180, 185, 195) },
        );
    }

    // Clean subtle 1px divider between items (not drawn on last item so it stays flush)
    if !is_last && !hovered {
        ui.painter().line_segment(
            [egui::pos2(rect.left() + 8.0, rect.bottom()), egui::pos2(rect.right() - 8.0, rect.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 14)),
        );
    }

    response.clicked()
}

// Section card helper for Settings view
fn render_section_card(ui: &mut egui::Ui, header: &str, accent: Color32, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(16, 17, 21, 240))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 20)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(16_i8, 12_i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(header)
                    .size(11.5)
                    .strong()
                    .color(accent),
            );
            ui.add_space(6.0);
            add_contents(ui);
        });
}

pub struct ScytheOverlayApp {
    config: ScytheConfig,
    status: DaemonStatus,
    daemon_connected: bool,
    current_view: ShadowPlayView,
    replay_dropdown_open: bool,
    record_dropdown_open: bool,
    output_dir: String,
    replay_sec: u32,
    bitrate_mbps: u32,
    target_fps: u32,
    video_codec: String,
    audio_mode_idx: usize,
    show_cursor: bool,
    mic_volume_pct: u32,
    system_volume_pct: u32,
    mic_vu: f32,
    sys_vu: f32,
    selected_clip_idx: Option<usize>,
    trim_start_sec: f32,
    trim_end_sec: f32,
    trim_status_msg: Option<(String, Instant)>,
    clip_duration_sec: f32,
    anim_time: f32,
    status_rx: Receiver<DaemonStatus>,
    folder_tx: Sender<String>,
    folder_rx: Receiver<String>,
    folder_picking_active: Arc<AtomicBool>,
    clips: Vec<VideoClipInfo>,
    initial_pos_set: bool,
    listening_keybind: Option<KeybindAction>,
    fps_input_str: String,
    bitrate_input_str: String,
    replay_sec_input_str: String,
    panel_rect: egui::Rect,
    frame_count: u32,
    update_status: Arc<std::sync::Mutex<crate::updater::UpdateStatus>>,
    update_dismissed: bool,
    auto_check_updates: bool,
    autostart_replay: bool,
    autostart_overlay: bool,
    hud_notification: Option<HudNotification>,
}

#[derive(Clone, Debug)]
pub struct HudNotification {
    pub title: String,
    pub subtitle: String,
    pub icon: crate::overlay::ToastIcon,
    pub start_time: Instant,
    pub duration_secs: f32,
}

pub type VrecOverlayApp = ScytheOverlayApp;

impl Default for ScytheOverlayApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ScytheOverlayApp {
    pub fn new() -> Self {
        let config = ScytheConfig::load();
        let replay_sec = config.replay_duration_sec;
        let bitrate_mbps = (config.record_bitrate_kbps / 1000).max(1);
        let target_fps = config.fps;
        let output_dir = config.output_directory.clone();
        let show_cursor = config.show_cursor;
        let video_codec = config.video_codec.clone();
        let mic_volume_pct = (config.mic_volume * 100.0).round().clamp(0.0, 200.0) as u32;
        let system_volume_pct = (config.system_volume * 100.0).round().clamp(0.0, 200.0) as u32;
        let audio_mode_idx = match config.audio_mode.as_str() {
            "mic" => 1,
            "both" => 2,
            "muted" => 3,
            _ => 0,
        };

        let (status_tx, status_rx) = channel::<DaemonStatus>();
        std::thread::spawn(move || {
            loop {
                if let Ok(s) = ipc::query_status() {
                    let _ = status_tx.send(s);
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });

        let (folder_tx, folder_rx) = channel::<String>();
        let folder_picking_active = Arc::new(AtomicBool::new(false));
        let clips = scan_recordings(&output_dir);
        let fps_input_str = target_fps.to_string();
        let bitrate_input_str = bitrate_mbps.to_string();
        let replay_sec_input_str = replay_sec.to_string();

        let auto_check_updates = config.auto_check_updates;
        let autostart_replay = config.autostart_replay;
        let autostart_overlay = config.autostart_overlay;
        let update_status = Arc::new(std::sync::Mutex::new(crate::updater::UpdateStatus::Idle));
        if auto_check_updates {
            crate::updater::spawn_update_check(update_status.clone());
        }

        Self {
            config,
            status: DaemonStatus::default(),
            daemon_connected: false,
            current_view: ShadowPlayView::MainHud,
            replay_dropdown_open: false,
            record_dropdown_open: false,
            output_dir,
            replay_sec,
            bitrate_mbps,
            target_fps,
            video_codec,
            audio_mode_idx,
            show_cursor,
            mic_volume_pct,
            system_volume_pct,
            mic_vu: 0.0,
            sys_vu: 0.0,
            selected_clip_idx: None,
            trim_start_sec: 0.0,
            trim_end_sec: 30.0,
            trim_status_msg: None,
            clip_duration_sec: 0.0,
            anim_time: 0.0,
            status_rx,
            folder_tx,
            folder_rx,
            folder_picking_active,
            clips,
            initial_pos_set: false,
            listening_keybind: None,
            fps_input_str,
            bitrate_input_str,
            replay_sec_input_str,
            panel_rect: egui::Rect::NOTHING,
            frame_count: 0,
            update_status,
            update_dismissed: false,
            auto_check_updates,
            autostart_replay,
            autostart_overlay,
            hud_notification: None,
        }
    }

    pub fn show_hud_notification(&mut self, title: &str, subtitle: &str, icon: crate::overlay::ToastIcon) {
        self.hud_notification = Some(HudNotification {
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            icon,
            start_time: Instant::now(),
            duration_secs: 2.8,
        });
    }

    pub fn accent_color(&self) -> Color32 {
        resolve_accent_color(&self.config.accent_color)
    }

    pub fn accent_alpha(&self, alpha: u8) -> Color32 {
        let c = self.accent_color();
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
    }

    pub fn refresh_clips(&mut self) {
        self.clips = scan_recordings(&self.output_dir);
    }

    fn poll_async_events(&mut self) {
        while let Ok(s) = self.status_rx.try_recv() {
            let target_mic = s.mic_level_peak;
            let target_sys = s.system_level_peak;
            self.mic_vu = if target_mic > self.mic_vu {
                target_mic
            } else {
                self.mic_vu * 0.85 + target_mic * 0.15
            };
            self.sys_vu = if target_sys > self.sys_vu {
                target_sys
            } else {
                self.sys_vu * 0.85 + target_sys * 0.15
            };
            self.status = s;
            self.daemon_connected = true;
        }

        while let Ok(new_dir) = self.folder_rx.try_recv() {
            if !new_dir.is_empty() {
                self.output_dir = new_dir.clone();
                self.config.output_directory = new_dir;
                let _ = self.config.save();
                async_send_command(Command::ReloadConfig);
                self.refresh_clips();
            }
        }
    }

    pub fn update_window_size(&self, _ctx: &egui::Context) {
        // Fullscreen surface handles internal layout dynamically without window resize jitter
    }

    pub fn switch_view(&mut self, view: ShadowPlayView, _ctx: &egui::Context) {
        self.current_view = view;
        self.listening_keybind = None;
        self.replay_dropdown_open = false;
        self.record_dropdown_open = false;
    }

    fn render_main_hud(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let anim_time = self.anim_time;
        let is_recording = self.status.is_recording;
        let rec_dur = self.status.recording_duration_sec;
        let is_replay_active = self.status.is_replay_active;
        let accent = self.accent_color();

        let screen_w = ui.available_width();
        let card_w = 210.0;
        let card_h = 185.0;
        let card_gap = 14.0;
        let total_cards_w = 3.0 * card_w + 2.0 * card_gap;
        let left_pad = ((screen_w - total_cards_w) / 2.0).max(10.0);
        let top_pad = 70.0;

        let hud_rect = egui::Rect::from_min_size(
            egui::pos2(left_pad, top_pad),
            egui::vec2(
                total_cards_w,
                card_h + 120.0,
            ),
        );
        self.panel_rect = hud_rect;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(hud_rect), |ui| {
            ui.vertical_centered(|ui| {
                // Voluntary, non-intrusive Update Notification Banner
                let cur_update = self.update_status.lock().ok().map(|g| g.clone()).unwrap_or_default();
                if let crate::updater::UpdateStatus::Available(ref info) = cur_update
                    && !self.update_dismissed {
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(12, 13, 16, 250))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160)))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::symmetric(14_i8, 7_i8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("UPDATE: v{}", info.version))
                                            .size(11.5)
                                            .strong()
                                            .color(accent),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("(Installed: v{})", crate::updater::CURRENT_VERSION))
                                            .size(10.5)
                                            .color(Color32::from_rgb(150, 150, 155)),
                                    );
                                    ui.add_space(8.0);
                                    if squared_button(ui, "DOWNLOAD", true, accent) {
                                        crate::updater::open_browser_url(&info.html_url);
                                    }
                                    ui.add_space(4.0);
                                    if squared_button(ui, "DISMISS", false, accent) {
                                        self.update_dismissed = true;
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }

                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(card_gap, 0.0);

                    // =========================================================================
                    // CARD 1: INSTANT REPLAY
                    // =========================================================================
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.set_min_width(card_w);
                        ui.set_max_width(card_w);
                        let card1_clicked = render_action_card(
                            ui,
                            card_w,
                            card_h,
                            "INSTANT REPLAY",
                            is_replay_active,
                            self.replay_dropdown_open,
                            |painter, center| {
                                draw_replay_icon(painter, center, 24.0, is_replay_active, accent);
                            },
                            if is_replay_active { "Turned on" } else { "Turned off" },
                            if is_replay_active { accent } else { Color32::from_rgb(150, 150, 155) },
                            "(Click for menu)",
                            accent,
                        );

                        if card1_clicked {
                            self.replay_dropdown_open = !self.replay_dropdown_open;
                            self.record_dropdown_open = false;
                        }

                        if self.replay_dropdown_open {
                            render_dropdown_menu(ui, card_w, accent, |ui| {
                                let toggle_title = if is_replay_active { "Turn off" } else { "Turn on" };
                                let toggle_sub = if is_replay_active { "Stop background buffer" } else { "Capture continuous replay" };
                                if render_menu_item(ui, toggle_title, Some(toggle_sub), None, is_replay_active, accent, false) {
                                    let mut cfg = ScytheConfig::load();
                                    cfg.replay_enabled = !cfg.replay_enabled;
                                    let _ = cfg.save();
                                    ScytheConfig::notify_daemon_reload();
                                    self.config.replay_enabled = cfg.replay_enabled;
                                    self.status.is_replay_active = cfg.replay_enabled;
                                    self.replay_dropdown_open = false;
                                }
                                let save_hk = self.config.save_hotkey.trim();
                                let save_hk_opt = if save_hk.is_empty() { None } else { Some(save_hk) };
                                if render_menu_item(ui, "Save Replay", Some("Export buffer to file"), save_hk_opt, false, accent, true) {
                                    if is_replay_active {
                                        async_send_command(Command::SaveReplay);
                                        self.show_hud_notification("INSTANT REPLAY", "Saved to Videos", crate::overlay::ToastIcon::Replay);
                                    } else {
                                        self.show_hud_notification("INSTANT REPLAY", "Replay is turned off", crate::overlay::ToastIcon::Error);
                                    }
                                    self.replay_dropdown_open = false;
                                }
                            });
                        }
                    });

                    // =========================================================================
                    // CARD 2: RECORD
                    // =========================================================================
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.set_min_width(card_w);
                        ui.set_max_width(card_w);
                        let rec_status_str = if is_recording {
                            let mins = rec_dur / 60;
                            let secs = rec_dur % 60;
                            format!("Recording {:02}:{:02}", mins, secs)
                        } else {
                            "Not recording".to_string()
                        };

                        let card2_clicked = render_action_card(
                            ui,
                            card_w,
                            card_h,
                            "RECORD",
                            is_recording,
                            self.record_dropdown_open,
                            |painter, center| {
                                draw_record_icon(painter, center, 24.0, is_recording, anim_time);
                            },
                            &rec_status_str,
                            if is_recording { Color32::from_rgb(239, 68, 68) } else { Color32::from_rgb(150, 150, 155) },
                            "(Click for menu)",
                            accent,
                        );

                        if card2_clicked {
                            self.record_dropdown_open = !self.record_dropdown_open;
                            self.replay_dropdown_open = false;
                        }

                        if self.record_dropdown_open {
                            render_dropdown_menu(ui, card_w, accent, |ui| {
                                let rec_toggle_title = if is_recording { "Stop Recording" } else { "Start Recording" };
                                let rec_toggle_sub = if is_recording { "Finalize and save video" } else { "Capture video to disk" };
                                let rec_hk = self.config.record_hotkey.trim();
                                let rec_hk_opt = if rec_hk.is_empty() { None } else { Some(rec_hk) };
                                if render_menu_item(ui, rec_toggle_title, Some(rec_toggle_sub), rec_hk_opt, is_recording, accent, true) {
                                    async_send_command(Command::ToggleRecording);
                                    if is_recording {
                                        self.show_hud_notification("RECORDING", "Recording saved", crate::overlay::ToastIcon::Save);
                                    } else {
                                        self.show_hud_notification("RECORDING", "Recording started", crate::overlay::ToastIcon::Record);
                                    }
                                    self.record_dropdown_open = false;
                                }
                            });
                        }
                    });

                    // =========================================================================
                    // CARD 3: SETTINGS
                    // =========================================================================
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.set_min_width(card_w);
                        ui.set_max_width(card_w);
                        let card3_clicked = render_action_card(
                            ui,
                            card_w,
                            card_h,
                            "SETTINGS",
                            false,
                            false,
                            |painter, center| {
                                draw_settings_icon(painter, center, 24.0, Color32::from_rgb(150, 150, 155));
                            },
                            "Hardware & Tuning",
                            Color32::from_rgb(150, 150, 155),
                            "(Click to open)",
                            accent,
                        );

                        if card3_clicked {
                            self.replay_dropdown_open = false;
                            self.record_dropdown_open = false;
                            self.switch_view(ShadowPlayView::Settings, ctx);
                        }
                    });
                });
            });
        });
    }
    fn render_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let screen_w = ui.available_width();
        let screen_h = ui.available_height();
        let modal_w = 680.0_f32;
        let modal_h = (screen_h - 40.0).clamp(660.0, 920.0);
        let left_pad = ((screen_w - modal_w) / 2.0).max(10.0);
        let top_pad = ((screen_h - modal_h) / 2.0).max(15.0);
        let accent = self.accent_color();

        let modal_rect = egui::Rect::from_min_size(egui::pos2(left_pad, top_pad), egui::vec2(modal_w, modal_h));
        self.panel_rect = modal_rect;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(modal_rect), |ui| {
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(11, 12, 15, 252))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 140)))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::symmetric(24_i8, 20_i8))
                .show(ui, |ui| {
                    ui.set_width(modal_w - 48.0);

                    // Portrait Header: Back Button, Title, Close Hint (No X button)
                    ui.horizontal(|ui| {
                        let back_btn = egui::Button::new(
                            egui::RichText::new("< BACK")
                                .size(11.5)
                                .strong()
                                .color(accent),
                        )
                        .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 24))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160)))
                        .corner_radius(CornerRadius::ZERO);

                        if ui.add(back_btn).clicked() {
                            self.switch_view(ShadowPlayView::MainHud, ctx);
                            return;
                        }

                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("SETTINGS")
                                .font(FontId::proportional(15.0))
                                .strong()
                                .color(Color32::WHITE),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::NONE
                                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 12))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 22)))
                                .corner_radius(CornerRadius::ZERO)
                                .inner_margin(Margin::symmetric(9_i8, 4_i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("ESC TO CLOSE")
                                            .size(10.5)
                                            .strong()
                                            .color(Color32::from_rgb(160, 160, 168)),
                                    );
                                });
                        });
                    });

                    ui.add_space(14.0);

                    let scroll_h = modal_h - 115.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // -----------------------------------------------------------------
                            // SECTION 1: SYSTEM & STARTUP
                            // -----------------------------------------------------------------
                            render_section_card(ui, "SYSTEM & STARTUP", accent, |ui| {
                                // Autostart Instant Replay
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new("Autostart Instant Replay").size(12.5).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("Automatically launch the background recording engine on login so replay is always ready.").size(10.5).color(Color32::from_rgb(150, 150, 155)));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut ar = self.autostart_replay;
                                        if toggle_switch(ui, &mut ar, accent).changed() {
                                            self.autostart_replay = ar;
                                            self.config.autostart_replay = ar;
                                            self.config.autostart = ar;
                                            let _ = self.config.save();
                                            if ar {
                                                spawn_daemon_process();
                                            }
                                            self.show_hud_notification(
                                                "AUTOSTART REPLAY",
                                                if ar { "Enabled: Starts on login" } else { "Disabled" },
                                                crate::overlay::ToastIcon::Info,
                                            );
                                        }
                                    });
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                // Autostart HUD Overlay
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new("Autostart HUD Overlay").size(12.5).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("Automatically open the HUD overlay menu when logging into your desktop session.").size(10.5).color(Color32::from_rgb(150, 150, 155)));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut ao = self.autostart_overlay;
                                        if toggle_switch(ui, &mut ao, accent).changed() {
                                            self.autostart_overlay = ao;
                                            self.config.autostart_overlay = ao;
                                            let _ = self.config.save();
                                            self.show_hud_notification(
                                                "AUTOSTART OVERLAY",
                                                if ao { "Enabled: Opens on login" } else { "Disabled" },
                                                crate::overlay::ToastIcon::Info,
                                            );
                                        }
                                    });
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 2: DISPLAY & CAPTURE
                            // -----------------------------------------------------------------
                            render_section_card(ui, "DISPLAY & CAPTURE", accent, |ui| {
                                // Record Mouse Cursor
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Record Mouse Cursor").size(12.0).strong().color(Color32::WHITE));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut cur = self.show_cursor;
                                        if toggle_switch(ui, &mut cur, accent).changed() {
                                            self.show_cursor = cur;
                                            self.config.show_cursor = cur;
                                            let _ = self.config.save();
                                            async_send_command(Command::ToggleCursor);
                                            self.show_hud_notification(
                                                "MOUSE CURSOR",
                                                if cur { "Visible in recording" } else { "Hidden from recording" },
                                                crate::overlay::ToastIcon::Cursor,
                                            );
                                        }
                                    });
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                // Framerate (FPS) with direct input and presets
                                ui.label(egui::RichText::new("Target Framerate (FPS):").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Custom:").size(11.0).color(Color32::from_rgb(150, 150, 155)));
                                    let edit_resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.fps_input_str)
                                            .desired_width(55.0)
                                            .font(FontId::monospace(11.5))
                                    );
                                    if edit_resp.changed()
                                        && let Ok(parsed) = self.fps_input_str.trim().parse::<u32>()
                                            && (15..=360).contains(&parsed) {
                                                self.target_fps = parsed;
                                            }

                                    ui.add_space(8.0);
                                    for fps in [30, 60, 120, 144, 240] {
                                        if squared_button(ui, &fps.to_string(), self.target_fps == fps, accent) {
                                            self.target_fps = fps;
                                            self.fps_input_str = fps.to_string();
                                        }
                                    }
                                });

                                ui.add_space(8.0);

                                // Video Bitrate (Mbps) with direct input, presets, and slider
                                ui.label(egui::RichText::new("Video Bitrate:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Custom:").size(11.0).color(Color32::from_rgb(150, 150, 155)));
                                    let edit_resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.bitrate_input_str)
                                            .desired_width(55.0)
                                            .font(FontId::monospace(11.5))
                                    );
                                    if edit_resp.changed()
                                        && let Ok(parsed) = self.bitrate_input_str.trim().parse::<u32>()
                                            && (1..=300).contains(&parsed) {
                                                self.bitrate_mbps = parsed;
                                            }
                                    ui.label(egui::RichText::new("Mbps").size(10.5).color(Color32::from_rgb(150, 150, 155)));

                                    ui.add_space(8.0);
                                    for mbps in [10, 20, 35, 50, 80] {
                                        if squared_button(ui, &format!("{}M", mbps), self.bitrate_mbps == mbps, accent) {
                                            self.bitrate_mbps = mbps;
                                            self.bitrate_input_str = mbps.to_string();
                                        }
                                    }
                                    ui.add_space(6.0);
                                    let mut br = self.bitrate_mbps;
                                    if ui.add(egui::Slider::new(&mut br, 5..=150).suffix(" Mbps").step_by(5.0)).changed() {
                                        self.bitrate_mbps = br;
                                        self.bitrate_input_str = br.to_string();
                                    }
                                });

                                ui.add_space(8.0);

                                // Instant Replay Buffer Duration with direct input and presets
                                ui.label(egui::RichText::new("Instant Replay Buffer Duration:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Custom (sec):").size(11.0).color(Color32::from_rgb(150, 150, 155)));
                                    let edit_resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.replay_sec_input_str)
                                            .desired_width(55.0)
                                            .font(FontId::monospace(11.5))
                                    );
                                    if edit_resp.changed()
                                        && let Ok(parsed) = self.replay_sec_input_str.trim().parse::<u32>()
                                            && (5..=1800).contains(&parsed) {
                                                self.replay_sec = parsed;
                                            }
                                    ui.label(egui::RichText::new("sec").size(10.5).color(Color32::from_rgb(150, 150, 155)));

                                    ui.add_space(8.0);
                                    for sec in [15, 30, 60, 120, 300] {
                                        let label = if sec >= 60 { format!("{}m", sec / 60) } else { format!("{}s", sec) };
                                        if squared_button(ui, &label, self.replay_sec == sec, accent) {
                                            self.replay_sec = sec;
                                            self.replay_sec_input_str = sec.to_string();
                                        }
                                    }
                                });

                                ui.add_space(8.0);

                                // Encoder Codec
                                ui.label(egui::RichText::new("Encoder Codec:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for (codec_key, label) in [("h264", "H.264"), ("hevc", "HEVC / H.265"), ("av1", "AV1")] {
                                        if squared_button(ui, label, self.video_codec == codec_key, accent) {
                                            self.video_codec = codec_key.to_string();
                                        }
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 2: AUDIO CONFIGURATION
                            // -----------------------------------------------------------------
                            render_section_card(ui, "AUDIO CONFIGURATION", accent, |ui| {
                                ui.label(egui::RichText::new("Audio Source:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let modes = [("system", "System"), ("mic", "Microphone"), ("both", "Both"), ("muted", "Muted")];
                                    for (idx, (_, m_label)) in modes.iter().enumerate() {
                                        if squared_button(ui, m_label, self.audio_mode_idx == idx, accent) {
                                            self.audio_mode_idx = idx;
                                        }
                                    }
                                });

                                ui.add_space(8.0);

                                // Mic Volume + VU
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Mic Vol:").size(12.0).strong().color(Color32::WHITE));
                                    let mut mv = self.mic_volume_pct;
                                    if ui.add(egui::Slider::new(&mut mv, 0..=200).suffix("%")).changed() {
                                        self.mic_volume_pct = mv;
                                    }
                                    ui.add_space(6.0);
                                    render_vu_meter(ui, self.mic_vu, 65.0, 18.0, "MIC");
                                });

                                ui.add_space(8.0);

                                // System Audio Volume + VU
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Sys Vol:").size(12.0).strong().color(Color32::WHITE));
                                    let mut sv = self.system_volume_pct;
                                    if ui.add(egui::Slider::new(&mut sv, 0..=200).suffix("%")).changed() {
                                        self.system_volume_pct = sv;
                                    }
                                    ui.add_space(6.0);
                                    render_vu_meter(ui, self.sys_vu, 65.0, 18.0, "SYS");
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 3: KEYBOARD SHORTCUTS (INTERACTIVE REBINDING)
                            // -----------------------------------------------------------------
                            render_section_card(ui, "KEYBOARD SHORTCUTS (CLICK TO REBIND)", accent, |ui| {
                                let binds = [
                                    (KeybindAction::Menu, "Menu Overlay", &self.config.menu_hotkey),
                                    (KeybindAction::SaveReplay, "Save Instant Replay", &self.config.save_hotkey),
                                    (KeybindAction::ToggleRecord, "Start / Stop Record", &self.config.record_hotkey),
                                    (KeybindAction::ToggleCursor, "Toggle Mouse Cursor", &self.config.cursor_hotkey),
                                ];

                                for (action, label, current_hotkey) in binds {
                                    let is_listening = self.listening_keybind == Some(action);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(label)
                                                .size(12.0)
                                                .color(if is_listening { accent } else { Color32::from_rgb(203, 213, 225) }),
                                        );

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if render_keycap_button(ui, current_hotkey, is_listening, accent) {
                                                if is_listening {
                                                    self.listening_keybind = None;
                                                } else {
                                                    self.listening_keybind = Some(action);
                                                }
                                            }
                                        });
                                    });
                                    ui.add_space(6.0);
                                }

                                if self.listening_keybind.is_some() {
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("Listening... Press your desired key combination or Esc to cancel.")
                                            .size(11.0)
                                            .strong()
                                            .color(accent),
                                    );
                                }

                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if squared_button(ui, "Reset Hotkeys to Defaults", false, accent) {
                                        let old_menu = self.config.menu_hotkey.clone();
                                        let old_save = self.config.save_hotkey.clone();
                                        let old_rec = self.config.record_hotkey.clone();
                                        let old_cur = self.config.cursor_hotkey.clone();
                                        crate::hyprland_binds::unbind_hotkey_async(&old_menu);
                                        crate::hyprland_binds::unbind_hotkey_async(&old_save);
                                        crate::hyprland_binds::unbind_hotkey_async(&old_rec);
                                        crate::hyprland_binds::unbind_hotkey_async(&old_cur);
                                        self.config.menu_hotkey = "Alt+Z".to_string();
                                        self.config.save_hotkey = "Ctrl+Shift+R".to_string();
                                        self.config.record_hotkey = "Ctrl+Shift+F9".to_string();
                                        self.config.cursor_hotkey = "Ctrl+Shift+F10".to_string();
                                        let _ = self.config.save();
                                        crate::hyprland_binds::register_hyprland_binds_async(&self.config);
                                        crate::config::ScytheConfig::notify_daemon_reload();
                                        self.show_hud_notification("KEYBINDS", "Restored default hotkeys", crate::overlay::ToastIcon::Info);
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 5: STORAGE & FILE NAMING
                            // -----------------------------------------------------------------
                            render_section_card(ui, "STORAGE & FILE NAMING", accent, |ui| {
                                ui.label(egui::RichText::new("Save Folder:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(340.0));
                                    if squared_button(ui, "Change", false, accent) {
                                        pick_folder(&self.output_dir, self.folder_tx.clone(), self.folder_picking_active.clone());
                                    }
                                    if squared_button(ui, "Open", false, accent) {
                                        open_folder(&ScytheConfig::expand_tilde(&self.output_dir));
                                    }
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(egui::RichText::new("Default File Naming Format:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(3.0);
                                let sample_replay = ScytheConfig::format_video_filename("Replay", "mp4");
                                let sample_record = ScytheConfig::format_video_filename("Recording", "mp4");
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Template:").size(11.0).color(Color32::from_rgb(150, 150, 155)));
                                    ui.label(egui::RichText::new("Prefix-Timestamp_Date_Month_Year.mp4").size(11.0).font(FontId::monospace(11.0)).color(accent));
                                });
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Instant Replay:").size(11.0).color(Color32::from_rgb(150, 150, 155)));
                                    ui.label(egui::RichText::new(&sample_replay).size(11.0).font(FontId::monospace(11.0)).color(Color32::WHITE));
                                });
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Manual Record:").size(11.0).color(Color32::from_rgb(150, 150, 155)));
                                    ui.label(egui::RichText::new(&sample_record).size(11.0).font(FontId::monospace(11.0)).color(Color32::WHITE));
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 5: ACCENT COLOR THEME (AT THE BOTTOM)
                            // -----------------------------------------------------------------
                            render_section_card(ui, "ACCENT COLOR THEME", accent, |ui| {
                                ui.label(egui::RichText::new("Select Interface Accent Color:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(6.0);
                                ui.horizontal_wrapped(|ui| {
                                    let palettes = [
                                        ("blue", "Charming Blue", Color32::from_rgb(56, 189, 248)),
                                        ("green", "Emerald Green", Color32::from_rgb(34, 197, 94)),
                                        ("lime", "Electric Lime", Color32::from_rgb(163, 230, 53)),
                                        ("yellow", "Solar Yellow", Color32::from_rgb(250, 204, 21)),
                                        ("amber", "Sunset Amber", Color32::from_rgb(245, 158, 11)),
                                        ("red", "Crimson Red", Color32::from_rgb(239, 68, 68)),
                                        ("pink", "Neon Pink", Color32::from_rgb(244, 63, 94)),
                                        ("purple", "Royal Purple", Color32::from_rgb(168, 85, 247)),
                                    ];

                                    for (id, name, col) in palettes {
                                        let is_sel = self.config.accent_color.to_lowercase() == id;
                                        let bg = if is_sel {
                                            Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 50)
                                        } else {
                                            Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 16)
                                        };
                                        // Colored border with each color's respective look
                                        let stroke = Stroke::new(if is_sel { 2.0_f32 } else { 1.2_f32 }, col);

                                        let btn = egui::Button::new(
                                            egui::RichText::new(name)
                                                .size(11.5)
                                                .strong()
                                                .color(if is_sel { Color32::WHITE } else { Color32::from_rgb(226, 232, 240) }),
                                        )
                                        .fill(bg)
                                        .stroke(stroke)
                                        .corner_radius(CornerRadius::ZERO)
                                        .min_size(Vec2::new(140.0, 30.0));

                                        if ui.add(btn).clicked() {
                                            self.config.accent_color = id.to_string();
                                            let _ = self.config.save();
                                            self.show_hud_notification("THEME ACCENT", &format!("Selected: {}", name), crate::overlay::ToastIcon::Info);
                                        }
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 6: ABOUT & AUTO-UPDATES
                            // -----------------------------------------------------------------
                            render_section_card(ui, "ABOUT & AUTO-UPDATES", accent, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("Installed Version: v{}", crate::updater::CURRENT_VERSION))
                                            .size(12.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                    ui.add_space(10.0);

                                    let cur_status = self.update_status.lock().ok().map(|g| g.clone()).unwrap_or_default();
                                    match cur_status {
                                        crate::updater::UpdateStatus::Idle => {
                                            if squared_button(ui, "Check for Updates", false, accent) {
                                                crate::updater::spawn_update_check(self.update_status.clone());
                                            }
                                        }
                                        crate::updater::UpdateStatus::Checking => {
                                            ui.label(
                                                egui::RichText::new("Checking for updates...")
                                                    .size(11.5)
                                                    .color(Color32::from_rgb(150, 150, 155)),
                                            );
                                        }
                                        crate::updater::UpdateStatus::UpToDate { version } => {
                                            ui.label(
                                                egui::RichText::new(format!("✓ Up to date (v{})", version))
                                                    .size(11.5)
                                                    .color(Color32::from_rgb(34, 197, 94))
                                                    .strong(),
                                            );
                                            ui.add_space(6.0);
                                            if squared_button(ui, "Check Again", false, accent) {
                                                crate::updater::spawn_update_check(self.update_status.clone());
                                            }
                                        }
                                        crate::updater::UpdateStatus::Available(info) => {
                                            ui.label(
                                                egui::RichText::new(format!("New version available: v{}", info.version))
                                                    .size(11.5)
                                                    .color(Color32::from_rgb(245, 158, 11))
                                                    .strong(),
                                            );
                                            ui.add_space(6.0);
                                            if squared_button(ui, "VIEW RELEASE / DOWNLOAD", true, accent) {
                                                crate::updater::open_browser_url(&info.html_url);
                                            }
                                        }
                                        crate::updater::UpdateStatus::Failed(err) => {
                                            ui.label(
                                                egui::RichText::new(format!("Offline or error: {}", err))
                                                    .size(11.0)
                                                    .color(Color32::from_rgb(239, 68, 68)),
                                            );
                                            ui.add_space(6.0);
                                            if squared_button(ui, "Retry", false, accent) {
                                                crate::updater::spawn_update_check(self.update_status.clone());
                                            }
                                        }
                                    }
                                });

                                ui.add_space(6.0);
                                ui.checkbox(
                                    &mut self.auto_check_updates,
                                    egui::RichText::new("Automatically check for updates on startup")
                                        .size(11.5)
                                        .color(Color32::from_rgb(226, 232, 240)),
                                );
                            });

                            ui.add_space(14.0);

                            // BOTTOM ACTIONS (RESET DEFAULTS + APPLY & SAVE)
                            ui.horizontal(|ui| {
                                let reset_btn = egui::Button::new(
                                    egui::RichText::new("RESET DEFAULTS")
                                        .font(FontId::proportional(11.5))
                                        .color(Color32::from_rgb(180, 180, 190)),
                                )
                                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 14))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28)))
                                .corner_radius(CornerRadius::ZERO)
                                .min_size(Vec2::new(135.0, 38.0));

                                if ui.add(reset_btn).clicked() {
                                    let def = ScytheConfig::default();
                                    self.target_fps = def.fps;
                                    self.fps_input_str = def.fps.to_string();
                                    self.bitrate_mbps = def.record_bitrate_kbps / 1000;
                                    self.bitrate_input_str = self.bitrate_mbps.to_string();
                                    self.replay_sec = def.replay_duration_sec;
                                    self.replay_sec_input_str = def.replay_duration_sec.to_string();
                                    self.video_codec = def.video_codec;
                                    self.show_cursor = def.show_cursor;
                                    self.mic_volume_pct = (def.mic_volume * 100.0) as u32;
                                    self.system_volume_pct = (def.system_volume * 100.0) as u32;
                                    self.autostart_replay = def.autostart_replay;
                                    self.autostart_overlay = def.autostart_overlay;
                                    self.show_hud_notification("DEFAULTS", "Settings restored to defaults", crate::overlay::ToastIcon::Info);
                                }

                                ui.add_space(8.0);

                                let apply_btn = egui::Button::new(
                                    egui::RichText::new("APPLY & SAVE SETTINGS")
                                        .font(FontId::proportional(12.5))
                                        .strong()
                                        .color(Color32::from_rgb(11, 18, 4)),
                                )
                                .fill(accent)
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::ZERO)
                                .min_size(Vec2::new(ui.available_width(), 38.0));

                                if ui.add(apply_btn).clicked() {
                                    self.config.show_cursor = self.show_cursor;
                                    self.config.fps = self.target_fps;
                                    self.config.record_bitrate_kbps = self.bitrate_mbps * 1000;
                                    self.config.replay_bitrate_kbps = self.bitrate_mbps * 1000;
                                    self.config.video_codec = self.video_codec.clone();
                                    self.config.replay_duration_sec = self.replay_sec;
                                    self.config.output_directory = self.output_dir.clone();
                                    self.config.mic_volume = self.mic_volume_pct as f32 / 100.0;
                                    self.config.system_volume = self.system_volume_pct as f32 / 100.0;
                                    self.config.audio_mode = match self.audio_mode_idx {
                                        1 => "mic".to_string(),
                                        2 => "both".to_string(),
                                        3 => "muted".to_string(),
                                        _ => "system".to_string(),
                                    };
                                    self.config.auto_check_updates = self.auto_check_updates;
                                    self.config.autostart_replay = self.autostart_replay;
                                    self.config.autostart_overlay = self.autostart_overlay;
                                    self.config.autostart = self.autostart_replay;
                                    let _ = self.config.save();
                                    crate::hyprland_binds::register_hyprland_binds(&self.config);
                                    crate::config::ScytheConfig::notify_daemon_reload();
                                    self.show_hud_notification("SETTINGS", "Settings Saved & Applied!", crate::overlay::ToastIcon::Info);
                                    self.switch_view(ShadowPlayView::MainHud, ctx);
                                }
                            });
                        });
                });
        });
    }
    fn render_gallery_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let screen_w = ui.available_width();
        let screen_h = ui.available_height();
        let modal_w = 740.0_f32;
        let modal_h = (screen_h - 70.0).clamp(520.0, 700.0);
        let left_pad = ((screen_w - modal_w) / 2.0).max(10.0);
        let top_pad = ((screen_h - modal_h) / 2.0).max(20.0);
        let accent = self.accent_color();

        let modal_rect = egui::Rect::from_min_size(egui::pos2(left_pad, top_pad), egui::vec2(modal_w, modal_h));
        self.panel_rect = modal_rect;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(modal_rect), |ui| {
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(11, 12, 15, 252))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 140)))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::symmetric(20_i8, 16_i8))
                .show(ui, |ui| {
                    ui.set_width(modal_w - 40.0);

                    // Header with Back Button and Refresh
                    ui.horizontal(|ui| {
                        let back_btn = egui::Button::new(
                            egui::RichText::new("< BACK TO SETTINGS")
                                .size(11.5)
                                .strong()
                                .color(accent),
                        )
                        .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 24))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160)))
                        .corner_radius(CornerRadius::ZERO);

                        if ui.add(back_btn).clicked() {
                            self.switch_view(ShadowPlayView::Settings, ctx);
                            return;
                        }

                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("GALLERY & CLIP TRIMMER")
                                .font(FontId::proportional(15.0))
                                .strong()
                                .color(Color32::WHITE),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if squared_button(ui, "REFRESH", false, accent) {
                                self.refresh_clips();
                            }
                            ui.add_space(4.0);
                            if squared_button(ui, "OPEN FOLDER", false, accent) {
                                open_folder(&ScytheConfig::expand_tilde(&self.output_dir));
                            }
                        });
                    });

                    ui.add_space(10.0);

                    if let Some((msg, ts)) = &self.trim_status_msg
                        && ts.elapsed() < Duration::from_secs(4) {
                            egui::Frame::NONE
                                .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 25))
                                .stroke(Stroke::new(1.0_f32, accent))
                                .corner_radius(CornerRadius::ZERO)
                                .inner_margin(Margin::symmetric(10_i8, 6_i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(msg)
                                            .size(11.5)
                                            .strong()
                                            .color(accent),
                                    );
                                });
                            ui.add_space(8.0);
                        }

                    // Main two-column split
                    ui.horizontal(|ui| {
                        // Left Column: Clips List
                        ui.vertical(|ui| {
                            ui.set_width(280.0);
                            ui.label(egui::RichText::new(format!("RECORDED CLIPS ({})", self.clips.len())).size(11.0).strong().color(Color32::from_rgb(150, 150, 155)));
                            ui.add_space(4.0);

                            egui::ScrollArea::vertical()
                                .max_height(modal_h - 120.0)
                                .id_salt("gallery_clips_scroll")
                                .show(ui, |ui| {
                                    if self.clips.is_empty() {
                                        ui.label(egui::RichText::new("No recordings found yet.\nPress hotkeys to capture clips.").size(11.0).color(Color32::from_rgb(120, 120, 126)));
                                    } else {
                                        for (idx, clip) in self.clips.iter().enumerate() {
                                            let is_sel = self.selected_clip_idx == Some(idx);
                                            let card_bg = if is_sel {
                                                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 35)
                                            } else {
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 10)
                                            };
                                            let border = if is_sel {
                                                Stroke::new(1.0_f32, accent)
                                            } else {
                                                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 20))
                                            };

                                            let resp = egui::Frame::NONE
                                                .fill(card_bg)
                                                .stroke(border)
                                                .corner_radius(CornerRadius::ZERO)
                                                .inner_margin(Margin::symmetric(8_i8, 6_i8))
                                                .show(ui, |ui| {
                                                    ui.set_width(260.0);
                                                    ui.horizontal(|ui| {
                                                        let badge_col = if clip.is_replay { accent } else { Color32::from_rgb(239, 68, 68) };
                                                        let badge_txt = if clip.is_replay { "REPLAY" } else { "REC" };
                                                        let (b_rect, _) = ui.allocate_exact_size(Vec2::new(44.0, 16.0), egui::Sense::hover());
                                                        ui.painter().rect_filled(b_rect, CornerRadius::ZERO, badge_col);
                                                        ui.painter().text(b_rect.center(), egui::Align2::CENTER_CENTER, badge_txt, FontId::monospace(8.5), Color32::from_rgb(11, 18, 4));

                                                        ui.add_space(4.0);
                                                        let size_mb = clip.size_bytes as f64 / (1024.0 * 1024.0);
                                                        ui.label(egui::RichText::new(format!("{:.1} MB", size_mb)).size(10.0).color(Color32::from_rgb(150, 150, 155)));
                                                    });
                                                    ui.add_space(2.0);
                                                    ui.label(egui::RichText::new(&clip.filename).size(10.5).strong().color(Color32::WHITE));
                                                });

                                            if resp.response.interact(egui::Sense::click()).clicked() {
                                                self.selected_clip_idx = Some(idx);
                                                let dur = probe_duration_sec(&clip.path);
                                                self.clip_duration_sec = dur;
                                                self.trim_start_sec = 0.0;
                                                self.trim_end_sec = if dur > 0.0 { dur } else { 30.0 };
                                            }
                                            ui.add_space(4.0);
                                        }
                                    }
                                });
                        });

                        ui.add_space(16.0);

                        // Right Column: Preview, Details & Trimmer
                        ui.vertical(|ui| {
                            ui.set_width(modal_w - 320.0);
                            if let Some(idx) = self.selected_clip_idx {
                                if idx < self.clips.len() {
                                    let clip = self.clips[idx].clone();
                                    let size_mb = clip.size_bytes as f64 / (1024.0 * 1024.0);

                                    render_section_card(ui, "CLIP DETAILS", accent, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("File:").size(11.0).strong().color(Color32::WHITE));
                                            ui.label(egui::RichText::new(&clip.filename).size(11.0).color(Color32::from_rgb(203, 213, 225)));
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Size:").size(11.0).strong().color(Color32::WHITE));
                                            ui.label(egui::RichText::new(format!("{:.2} MB ({} bytes)", size_mb, clip.size_bytes)).size(10.5).color(Color32::from_rgb(150, 150, 155)));
                                        });
                                        if self.clip_duration_sec > 0.0 {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("Duration:").size(11.0).strong().color(Color32::WHITE));
                                                let total_s = self.clip_duration_sec as u32;
                                                ui.label(egui::RichText::new(format!("{:02}:{:02} ({:.1}s)", total_s / 60, total_s % 60, self.clip_duration_sec)).size(10.5).color(accent));
                                            });
                                        }

                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            let play_btn = egui::Button::new(egui::RichText::new("PLAY VIDEO").size(11.0).strong().color(Color32::from_rgb(11, 18, 4)))
                                                .fill(accent)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(CornerRadius::ZERO);
                                            if ui.add(play_btn).clicked() {
                                                play_clip(&clip.path);
                                            }

                                            if squared_button(ui, "SHOW IN FOLDER", false, accent) {
                                                open_folder(&clip.path);
                                            }

                                            let del_btn = egui::Button::new(egui::RichText::new("DELETE").size(11.0).strong().color(Color32::from_rgb(239, 68, 68)))
                                                .fill(Color32::from_rgba_unmultiplied(239, 68, 68, 20))
                                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68)))
                                                .corner_radius(CornerRadius::ZERO);
                                            if ui.add(del_btn).clicked() {
                                                let _ = std::fs::remove_file(&clip.path);
                                                self.trim_status_msg = Some((format!("Deleted {}", clip.filename), Instant::now()));
                                                self.selected_clip_idx = None;
                                                self.refresh_clips();
                                            }
                                        });
                                    });

                                    ui.add_space(10.0);

                                    // LOSSLESS TRIMMER SECTION
                                    render_section_card(ui, "LOSSLESS VIDEO TRIMMER", accent, |ui| {
                                        let max_dur = if self.clip_duration_sec > 0.0 { self.clip_duration_sec } else { 300.0 };

                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Start Trim:").size(11.0).strong().color(Color32::WHITE));
                                            let mut s = self.trim_start_sec;
                                            if ui.add(egui::Slider::new(&mut s, 0.0..=max_dur).suffix("s")).changed() {
                                                self.trim_start_sec = s.min(self.trim_end_sec);
                                            }
                                        });

                                        ui.add_space(4.0);

                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("End Trim:").size(11.0).strong().color(Color32::WHITE));
                                            let mut e = self.trim_end_sec;
                                            if ui.add(egui::Slider::new(&mut e, 0.0..=max_dur).suffix("s")).changed() {
                                                self.trim_end_sec = e.max(self.trim_start_sec);
                                            }
                                        });

                                        let trimmed_dur = (self.trim_end_sec - self.trim_start_sec).max(0.0);
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(format!("Trimmed output length: {:.1}s (Instant lossless copy)", trimmed_dur)).size(10.5).color(Color32::from_rgb(150, 150, 155)));

                                        ui.add_space(8.0);
                                        let trim_btn = egui::Button::new(egui::RichText::new("TRIM & EXPORT COPY").size(11.5).strong().color(Color32::from_rgb(11, 18, 4)))
                                            .fill(accent)
                                            .stroke(Stroke::NONE)
                                            .corner_radius(CornerRadius::ZERO)
                                            .min_size(Vec2::new(ui.available_width(), 32.0));

                                        if ui.add(trim_btn).clicked() {
                                            match trim_clip(&clip.path, self.trim_start_sec, self.trim_end_sec) {
                                                Ok(out) => {
                                                    let fname = out.file_name().unwrap_or_default().to_string_lossy().to_string();
                                                    self.trim_status_msg = Some((format!("Exported trimmed clip: {}", fname), Instant::now()));
                                                    self.refresh_clips();
                                                }
                                                Err(e) => {
                                                    self.trim_status_msg = Some((format!("Trim failed: {}", e), Instant::now()));
                                                }
                                            }
                                        }
                                    });
                                }
                            } else {
                                render_section_card(ui, "CLIP PREVIEW & TRIMMER", accent, |ui| {
                                    ui.add_space(40.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(egui::RichText::new("No clip selected").font(FontId::proportional(13.0)).strong().color(Color32::from_rgb(150, 150, 155)));
                                        ui.add_space(6.0);
                                        ui.label(egui::RichText::new("Select a recording or instant replay from the list on the left to inspect, play, or losslessly trim.").size(10.5).color(Color32::from_rgb(120, 120, 126)));
                                    });
                                    ui.add_space(40.0);
                                });
                            }
                        });
                    });
                });
        });
    }

    fn render_slide_notification(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let accent = self.accent_color();
        if let Some(notif) = &self.hud_notification {
            let elapsed = notif.start_time.elapsed().as_secs_f32();
            if elapsed < notif.duration_secs {
                ctx.request_repaint(); // 60 FPS animation

                let slide_x = if elapsed < 0.35 {
                    let t = (elapsed / 0.35).min(1.0);
                    let ease = 1.0 - (1.0 - t).powi(3);
                    (1.0 - ease) * 360.0
                } else if elapsed < notif.duration_secs - 0.40 {
                    0.0
                } else {
                    let t = ((elapsed - (notif.duration_secs - 0.40)) / 0.40).min(1.0);
                    let ease = t.powi(3);
                    ease * 360.0
                };

                let screen_w = ui.available_width();
                let card_w = 320.0;
                let card_h = 56.0;
                let toast_rect = egui::Rect::from_min_size(
                    egui::pos2(screen_w - card_w - 28.0 + slide_x, 28.0),
                    egui::vec2(card_w, card_h),
                );

                let painter = ui.painter();

                // Drop shadow
                painter.rect_filled(
                    toast_rect.translate(Vec2::new(0.0, 4.0)),
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                );

                // Modern sleek dark glass toast card
                painter.rect(
                    toast_rect,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(12, 13, 16, 252),
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
                    egui::StrokeKind::Inside,
                );

                // Left vertical accent bar
                let line_color = if notif.icon == crate::overlay::ToastIcon::Record {
                    Color32::from_rgb(239, 68, 68)
                } else {
                    accent
                };
                let accent_line = egui::Rect::from_min_size(toast_rect.min + Vec2::new(1.0, 1.0), Vec2::new(3.5, toast_rect.height() - 2.0));
                painter.rect_filled(accent_line, CornerRadius::ZERO, line_color);

                // Left icon area (32x32 at center-left)
                let icon_center = egui::pos2(toast_rect.left() + 24.0, toast_rect.center().y);
                match notif.icon {
                    crate::overlay::ToastIcon::Replay => {
                        draw_replay_icon(painter, icon_center, 9.5, true, accent);
                    }
                    crate::overlay::ToastIcon::Record => {
                        painter.circle_filled(icon_center, 5.5, Color32::from_rgb(239, 68, 68));
                        painter.circle_stroke(icon_center, 9.5, Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(239, 68, 68, 120)));
                    }
                    crate::overlay::ToastIcon::Save => {
                        let c = icon_center;
                        let p1 = c + Vec2::new(-5.5, 0.0);
                        let p2 = c + Vec2::new(-1.5, 4.0);
                        let p3 = c + Vec2::new(5.5, -4.0);
                        painter.line_segment([p1, p2], Stroke::new(2.2_f32, accent));
                        painter.line_segment([p2, p3], Stroke::new(2.2_f32, accent));
                    }
                    crate::overlay::ToastIcon::Cursor => {
                        let c = icon_center;
                        painter.text(c, egui::Align2::CENTER_CENTER, "⯈", FontId::proportional(14.0), accent);
                    }
                    crate::overlay::ToastIcon::Error => {
                        let c = icon_center;
                        let red = Color32::from_rgb(239, 68, 68);
                        painter.line_segment([c + Vec2::new(-4.0, -4.0), c + Vec2::new(4.0, 4.0)], Stroke::new(2.0_f32, red));
                        painter.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(-4.0, 4.0)], Stroke::new(2.0_f32, red));
                    }
                    crate::overlay::ToastIcon::Info => {
                        painter.circle_filled(icon_center, 5.0, accent);
                    }
                }

                // Text stack (Title + Subtitle)
                let text_left = toast_rect.left() + 48.0;
                painter.text(
                    egui::pos2(text_left, toast_rect.top() + 19.0),
                    egui::Align2::LEFT_CENTER,
                    &notif.title,
                    FontId::proportional(12.5),
                    Color32::WHITE,
                );
                painter.text(
                    egui::pos2(text_left, toast_rect.top() + 37.0),
                    egui::Align2::LEFT_CENTER,
                    &notif.subtitle,
                    FontId::proportional(11.0),
                    Color32::from_rgb(160, 160, 168),
                );
            } else {
                self.hud_notification = None;
            }
        }
    }
}

impl eframe::App for ScytheOverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async_events();
        self.mic_vu = (self.mic_vu * 0.94).max(0.0);
        self.sys_vu = (self.sys_vu * 0.94).max(0.0);
        self.anim_time += 0.033;
        if self.listening_keybind.is_some() || self.hud_notification.is_some() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        self.frame_count += 1;
        // Auto-position, DWM transparency, and size to monitor on launch
        if !self.initial_pos_set && self.frame_count >= 2 {
            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size)
                && monitor_size.x > 100.0 && monitor_size.y > 100.0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(monitor_size));
                }
            #[cfg(target_os = "windows")]
            apply_windows_transparency("Scythe");
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.initial_pos_set = true;
        }

        // Handle interactive keybind recording mode
        if let Some(action) = self.listening_keybind {
            let mut captured_combo: Option<String> = None;
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                        if *key == egui::Key::Escape {
                            captured_combo = Some("CANCEL".to_string());
                            break;
                        }
                        if let Some(key_name) = format_egui_key(*key) {
                            let k_lower = key_name.to_lowercase();
                            if ["alt", "ctrl", "control", "shift", "super", "meta", "command"].contains(&k_lower.as_str()) {
                                continue;
                            }
                            let mut parts = Vec::new();
                            if modifiers.ctrl {
                                parts.push("Ctrl");
                            }
                            if modifiers.alt {
                                parts.push("Alt");
                            }
                            if modifiers.shift {
                                parts.push("Shift");
                            }
                            if modifiers.command && !modifiers.ctrl {
                                parts.push("Super");
                            }
                            parts.push(&key_name);
                            captured_combo = Some(parts.join("+"));
                            break;
                        }
                    }
                }
            });

            if let Some(combo) = captured_combo {
                if combo == "CANCEL" {
                    self.listening_keybind = None;
                } else {
                    let (action_name, old_key) = match action {
                        KeybindAction::Menu => {
                            let old = self.config.menu_hotkey.clone();
                            self.config.menu_hotkey = combo.clone();
                            ("Menu Overlay", old)
                        }
                        KeybindAction::SaveReplay => {
                            let old = self.config.save_hotkey.clone();
                            self.config.save_hotkey = combo.clone();
                            ("Instant Replay", old)
                        }
                        KeybindAction::ToggleRecord => {
                            let old = self.config.record_hotkey.clone();
                            self.config.record_hotkey = combo.clone();
                            ("Record Toggle", old)
                        }
                        KeybindAction::ToggleCursor => {
                            let old = self.config.cursor_hotkey.clone();
                            self.config.cursor_hotkey = combo.clone();
                            ("Cursor Toggle", old)
                        }
                    };
                    crate::hyprland_binds::unbind_hotkey_async(&old_key);
                    let _ = self.config.save();
                    crate::hyprland_binds::register_hyprland_binds_async(&self.config);
                    crate::config::ScytheConfig::notify_daemon_reload();
                    self.show_hud_notification("KEYBIND", &format!("Bound {}: {}", action_name, combo), crate::overlay::ToastIcon::Info);
                    self.listening_keybind = None;
                }
            }
        } else {
            // Normal Escape handling
            if !self.folder_picking_active.load(Ordering::SeqCst) && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                if self.replay_dropdown_open || self.record_dropdown_open {
                    self.replay_dropdown_open = false;
                    self.record_dropdown_open = false;
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    crate::ipc::clean_overlay_pid();
                    std::process::exit(0);
                }
            }

            // Click outside active panel on darkened background to dismiss
            if !self.folder_picking_active.load(Ordering::SeqCst) && ctx.input(|i| i.pointer.primary_clicked())
                && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
                    && self.panel_rect.width() > 10.0 && !self.panel_rect.expand(6.0).contains(pos) {
                        if self.replay_dropdown_open || self.record_dropdown_open {
                            self.replay_dropdown_open = false;
                            self.record_dropdown_open = false;
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            crate::ipc::clean_overlay_pid();
                            std::process::exit(0);
                        }
                    }
        }

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::TRANSPARENT;
        visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
        visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
        visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
        visuals.widgets.active.corner_radius = CornerRadius::ZERO;
        visuals.widgets.open.corner_radius = CornerRadius::ZERO;
        visuals.selection.bg_fill = self.accent_color();
        ctx.set_visuals(visuals);

        // Background screen darkening scrim (translucent dimming so background remains visible)
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(0, 0, 0, 80)))
            .show(ctx, |ui| {
                match self.current_view {
                    ShadowPlayView::MainHud => self.render_main_hud(ctx, ui),
                    ShadowPlayView::Settings => self.render_settings_view(ctx, ui),
                    ShadowPlayView::Gallery => self.render_gallery_view(ctx, ui),
                }
                self.render_slide_notification(ctx, ui);
            });
    }
}

#[cfg(target_os = "windows")]
pub fn apply_windows_transparency(title: &str) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, SetClassLongPtrW, GCLP_HBRBACKGROUND,
        };
        use windows::Win32::System::Threading::GetCurrentProcessId;
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
        use windows::Win32::UI::Controls::MARGINS;
        use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

        let my_pid = GetCurrentProcessId();
        let is_overlay = title == "Scythe";

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                let (my_pid, is_overlay) = {
                    let ptr = lparam.0 as *const (u32, bool);
                    *ptr
                };
                let mut proc_id = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut proc_id));
                if proc_id == my_pid {
                    // 1. Set window class background brush to 0 (NULL) to prevent GDI white flash
                    #[cfg(target_pointer_width = "64")]
                    let _ = SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);

                    // 2. Extend DWM frame margins into the client area
                    let margins = MARGINS {
                        cxLeftWidth: -1,
                        cxRightWidth: -1,
                        cyTopHeight: -1,
                        cyBottomHeight: -1,
                    };
                    let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

                    // 3. Set AccentPolicy via SetWindowCompositionAttribute for true transparency/blur
                    #[repr(C)]
                    struct AccentPolicy {
                        accent_state: u32,
                        accent_flags: u32,
                        gradient_color: u32,
                        animation_id: u32,
                    }

                    #[repr(C)]
                    struct WindowCompositionAttributeData {
                        attribute: u32, // WCA_ACCENT_POLICY = 19
                        data: *mut AccentPolicy,
                        size_of_data: usize,
                    }

                    type SetWindowCompositionAttributeFn =
                        unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> BOOL;

                    if let Ok(user32) = GetModuleHandleA(windows::core::s!("user32.dll")) {
                        if let Some(proc) = GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute")) {
                            let set_wca: SetWindowCompositionAttributeFn = std::mem::transmute(proc);
                            
                            let mut policy = if is_overlay {
                                AccentPolicy {
                                    accent_state: 3, // ACCENT_ENABLE_BLURBEHIND
                                    accent_flags: 2,
                                    gradient_color: 0x99101014, // Translucent dark obsidian tint
                                    animation_id: 0,
                                }
                            } else {
                                AccentPolicy {
                                    accent_state: 2, // ACCENT_ENABLE_TRANSPARENTGRADIENT
                                    accent_flags: 2,
                                    gradient_color: 0x00000000, // Fully transparent
                                    animation_id: 0,
                                }
                            };

                            let mut data = WindowCompositionAttributeData {
                                attribute: 19,
                                data: &mut policy,
                                size_of_data: std::mem::size_of::<AccentPolicy>(),
                            };

                            let _ = set_wca(hwnd, &mut data);
                        }
                    }
                }
                BOOL(1)
            }
        }

        let ctx_data = (my_pid, is_overlay);
        let _ = EnumWindows(Some(enum_proc), LPARAM(&ctx_data as *const _ as isize));
    }
}

#[cfg(not(target_os = "windows"))]
pub fn query_focused_monitor_rect() -> (f32, f32, f32, f32) {
    // 1. hyprctl monitors -j (Hyprland/Wayland)
    if let Ok(out) = std::process::Command::new("hyprctl").args(["monitors", "-j"]).output()
        && let Ok(v) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) {
            let focused = v.iter().find(|m| m["focused"].as_bool().unwrap_or(false))
                .or_else(|| v.first());
            if let Some(m) = focused {
                let x = m["x"].as_f64().unwrap_or(0.0) as f32;
                let y = m["y"].as_f64().unwrap_or(0.0) as f32;
                let w = m["width"].as_f64().unwrap_or(1920.0) as f32;
                let h = m["height"].as_f64().unwrap_or(1080.0) as f32;
                let scale = m["scale"].as_f64().unwrap_or(1.0) as f32;
                let lw = if scale > 0.0 { w / scale } else { w };
                let lh = if scale > 0.0 { h / scale } else { h };
                let lx = if scale > 0.0 { x / scale } else { x };
                let ly = if scale > 0.0 { y / scale } else { y };
                if lw > 320.0 && lh > 200.0 {
                    return (lx, ly, lw, lh);
                }
            }
        }
    // 2. xrandr fallback (X11 / XWayland)
    if let Ok(out) = std::process::Command::new("xrandr").output()
        && let Ok(text) = std::str::from_utf8(&out.stdout) {
            for line in text.lines() {
                if line.contains(" connected") {
                    for part in line.split_whitespace() {
                        if part.contains('x') && part.contains('+') {
                            let parts: Vec<&str> = part.split('+').collect();
                            if parts.len() >= 3 {
                                let dims: Vec<&str> = parts[0].split('x').collect();
                                if dims.len() == 2
                                    && let (Ok(w), Ok(h), Ok(x), Ok(y)) = (
                                        dims[0].parse::<f32>(),
                                        dims[1].parse::<f32>(),
                                        parts[1].parse::<f32>(),
                                        parts[2].parse::<f32>(),
                                    )
                                        && w > 320.0 && h > 200.0 {
                                            return (x, y, w, h);
                                        }
                            }
                        }
                    }
                }
            }
        }
    (0.0, 0.0, 1920.0, 1080.0)
}

pub fn run_egui_overlay() {
    #[cfg(target_os = "windows")]
    let (screen_x, screen_y, screen_w, screen_h): (f32, f32, f32, f32) = unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let w = GetSystemMetrics(SM_CXSCREEN) as f32;
        let h = GetSystemMetrics(SM_CYSCREEN) as f32;
        let sw = if w > 100.0 { w } else { 1920.0 };
        let sh = if h > 100.0 { h } else { 1080.0 };
        (0.0, 0.0, sw, sh)
    };
    #[cfg(not(target_os = "windows"))]
    let (screen_x, screen_y, screen_w, screen_h) = query_focused_monitor_rect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scythe")
            .with_app_id("scythe-overlay")
            .with_position([screen_x, screen_y])
            .with_inner_size([screen_w, screen_h])
            .with_maximized(false)
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(false)
            .with_always_on_top(),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "scythe-overlay",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(ScytheOverlayApp::new()))
        }),
    );

    crate::ipc::clean_overlay_pid();
}

pub struct ShadowPlayToastApp {
    title: String,
    subtitle: String,
    icon: crate::overlay::ToastIcon,
    accent: Color32,
    created_at: Instant,
    duration: Duration,
    initial_setup: bool,
}

impl eframe::App for ShadowPlayToastApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.initial_setup {
            if let Some(mon_size) = ctx.input(|i| i.viewport().monitor_size) {
                let toast_w = 340.0;
                let target_x = (mon_size.x - toast_w - 28.0).max(10.0);
                let target_y = 28.0;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(toast_w, 64.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(target_x, target_y)));
            }
            #[cfg(target_os = "windows")]
            apply_windows_transparency("Scythe Notification");
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.initial_setup = true;
        }

        let elapsed = self.created_at.elapsed().as_secs_f32();
        let total_dur = self.duration.as_secs_f32();
        if elapsed >= total_dur {
            crate::ipc::clean_toast_pid();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(16));

        let slide_x = if elapsed < 0.35 {
            let t = (elapsed / 0.35).min(1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            (1.0 - ease) * 340.0
        } else if elapsed < total_dur - 0.40 {
            0.0
        } else {
            let t = ((elapsed - (total_dur - 0.40)) / 0.40).min(1.0);
            let ease = t.powi(3);
            ease * 340.0
        };

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::TRANSPARENT;
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(slide_x + 10.0, 4.0),
                    Vec2::new(320.0, 56.0),
                );
                let painter = ui.painter();

                // Drop shadow
                painter.rect_filled(
                    rect.translate(Vec2::new(0.0, 4.0)),
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                );

                // Modern sleek dark glass toast card
                painter.rect(
                    rect,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(12, 13, 16, 252),
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
                    egui::StrokeKind::Inside,
                );

                // Left accent line
                let line_color = if self.icon == crate::overlay::ToastIcon::Record {
                    Color32::from_rgb(239, 68, 68)
                } else {
                    self.accent
                };
                let accent_line = egui::Rect::from_min_size(rect.min + Vec2::new(1.0, 1.0), Vec2::new(3.5, rect.height() - 2.0));
                painter.rect_filled(accent_line, CornerRadius::ZERO, line_color);

                // Left icon area (32x32 at center-left)
                let icon_center = egui::pos2(rect.left() + 24.0, rect.center().y);
                match self.icon {
                    crate::overlay::ToastIcon::Replay => {
                        draw_replay_icon(painter, icon_center, 9.5, true, self.accent);
                    }
                    crate::overlay::ToastIcon::Record => {
                        painter.circle_filled(icon_center, 5.5, Color32::from_rgb(239, 68, 68));
                        painter.circle_stroke(icon_center, 9.5, Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(239, 68, 68, 120)));
                    }
                    crate::overlay::ToastIcon::Save => {
                        let c = icon_center;
                        let p1 = c + Vec2::new(-5.5, 0.0);
                        let p2 = c + Vec2::new(-1.5, 4.0);
                        let p3 = c + Vec2::new(5.5, -4.0);
                        painter.line_segment([p1, p2], Stroke::new(2.2_f32, self.accent));
                        painter.line_segment([p2, p3], Stroke::new(2.2_f32, self.accent));
                    }
                    crate::overlay::ToastIcon::Cursor => {
                        let c = icon_center;
                        painter.text(c, egui::Align2::CENTER_CENTER, "⯈", FontId::proportional(14.0), self.accent);
                    }
                    crate::overlay::ToastIcon::Error => {
                        let c = icon_center;
                        let red = Color32::from_rgb(239, 68, 68);
                        painter.line_segment([c + Vec2::new(-4.0, -4.0), c + Vec2::new(4.0, 4.0)], Stroke::new(2.0_f32, red));
                        painter.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(-4.0, 4.0)], Stroke::new(2.0_f32, red));
                    }
                    crate::overlay::ToastIcon::Info => {
                        painter.circle_filled(icon_center, 5.0, self.accent);
                    }
                }

                // Text stack (Title + Subtitle)
                let text_left = rect.left() + 48.0;
                painter.text(
                    egui::pos2(text_left, rect.top() + 19.0),
                    egui::Align2::LEFT_CENTER,
                    &self.title,
                    FontId::proportional(12.5),
                    Color32::WHITE,
                );
                painter.text(
                    egui::pos2(text_left, rect.top() + 37.0),
                    egui::Align2::LEFT_CENTER,
                    &self.subtitle,
                    FontId::proportional(11.0),
                    Color32::from_rgb(160, 160, 168),
                );
            });
    }
}

pub fn run_egui_toast(title: &str, subtitle: &str, icon: crate::overlay::ToastIcon) {
    let cfg = ScytheConfig::load();
    let accent = resolve_accent_color(&cfg.accent_color);

    let toast_w: f32 = 340.0;
    let toast_h: f32 = 64.0;

    let toast_pid_path = crate::ipc::get_toast_pid_path();
    if let Ok(prev_pid_str) = std::fs::read_to_string(&toast_pid_path)
        && let Ok(prev_pid) = prev_pid_str.trim().parse::<u32>() {
            #[cfg(target_os = "windows")]
            unsafe {
                use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
                if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, prev_pid) {
                    let _ = TerminateProcess(h, 0);
                    let _ = windows::Win32::Foundation::CloseHandle(h);
                }
            }
            #[cfg(not(target_os = "windows"))]
            unsafe {
                let _ = libc::kill(prev_pid as i32, libc::SIGKILL);
            }
        }
    let _ = std::fs::create_dir_all(toast_pid_path.parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(&toast_pid_path, std::process::id().to_string());

    #[cfg(target_os = "windows")]
    let (mon_x, screen_w): (f32, f32) = unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
        let w = GetSystemMetrics(SM_CXSCREEN) as f32;
        let sw = if w > 100.0 { w } else { 1920.0 };
        (0.0, sw)
    };
    #[cfg(not(target_os = "windows"))]
    let (mon_x, _mon_y, screen_w, _screen_h) = query_focused_monitor_rect();

    let pos_x = (mon_x + screen_w - toast_w - 28.0).max(10.0);
    let pos_y = 28.0;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scythe Notification")
            .with_app_id("scythe-toast")
            .with_position([pos_x, pos_y])
            .with_inner_size([toast_w, toast_h])
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(false)
            .with_always_on_top()
            .with_resizable(false),
        ..Default::default()
    };

    let app = ShadowPlayToastApp {
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        icon,
        accent,
        created_at: Instant::now(),
        duration: Duration::from_millis(2800),
        initial_setup: false,
    };

    // Watchdog thread to guarantee exit after duration
    let watchdog_pid_path = toast_pid_path.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5000));
        let _ = std::fs::remove_file(watchdog_pid_path);
        std::process::exit(0);
    });

    let _ = eframe::run_native(
        "scythe-toast",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    );

    crate::ipc::clean_toast_pid();
}
