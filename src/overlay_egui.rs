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
    ReplaySettings,
    RecordSettings,
    StreamSettings,
    GlobalSettings,
    ScreenshotSettings,
    Gallery,
}

#[derive(Clone)]
pub struct OverlayTextures {
    pub replay: egui::TextureHandle,
    pub record: egui::TextureHandle,
    pub stream: egui::TextureHandle,
    pub screenshot: egui::TextureHandle,
    pub settings_small: egui::TextureHandle,
    pub settings_extra_small: egui::TextureHandle,
    pub cross: egui::TextureHandle,
    pub play: egui::TextureHandle,
    pub pause: egui::TextureHandle,
    pub stop: egui::TextureHandle,
    pub save: egui::TextureHandle,
    pub settings_large: egui::TextureHandle,
}

fn load_embedded_png(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let img = image::load_from_memory(bytes).expect("Embedded PNG must be valid").to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let pixels = img.as_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels);
    ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR)
}

fn draw_texture_centered(
    painter: &egui::Painter,
    texture: &egui::TextureHandle,
    center: egui::Pos2,
    target_height: f32,
    tint: Color32,
) {
    let [w, h] = texture.size();
    let aspect = w as f32 / h.max(1) as f32;
    let target_width = target_height * aspect;
    let rect = egui::Rect::from_center_size(center, Vec2::new(target_width, target_height));
    painter.image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}

fn draw_scythe_icon(painter: &egui::Painter, center: egui::Pos2, size: f32, accent: Color32) {
    let s = size;
    // Handle/staff (snath)
    let staff_bottom = center + Vec2::new(-0.24 * s, 0.44 * s);
    let staff_top = center + Vec2::new(0.06 * s, -0.38 * s);
    painter.line_segment([staff_bottom, staff_top], Stroke::new(2.2_f32, Color32::from_rgb(190, 195, 205)));

    // Side grip peg
    let peg_start = center + Vec2::new(-0.09 * s, 0.03 * s);
    let peg_end = center + Vec2::new(-0.24 * s, -0.04 * s);
    painter.line_segment([peg_start, peg_end], Stroke::new(1.8_f32, Color32::from_rgb(190, 195, 205)));

    // Scythe blade (sharp curved crescent)
    let blade_tip = center + Vec2::new(0.44 * s, -0.14 * s);
    let spine_mid = center + Vec2::new(0.28 * s, -0.46 * s);
    let edge_mid = center + Vec2::new(0.20 * s, -0.25 * s);

    // Blade filled polygon: staff_top -> spine_mid -> blade_tip -> edge_mid -> staff_top
    painter.add(egui::epaint::PathShape::convex_polygon(
        vec![
            staff_top,
            spine_mid,
            blade_tip,
            edge_mid,
        ],
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 230),
        Stroke::new(1.0_f32, accent),
    ));

    // Razor cutting edge line (white highlight from edge_mid to blade_tip)
    painter.line_segment([edge_mid, blade_tip], Stroke::new(1.4_f32, Color32::WHITE));

    // Reinforcement collar ring at staff top
    painter.circle_filled(staff_top, 2.4, Color32::from_rgb(230, 235, 245));
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
    let trimmed = accent.trim();
    if trimmed.starts_with('#') && trimmed.len() == 7 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&trimmed[1..3], 16),
            u8::from_str_radix(&trimmed[3..5], 16),
            u8::from_str_radix(&trimmed[5..7], 16),
        ) {
            return Color32::from_rgb(r, g, b);
        }
    }
    match trimmed.to_lowercase().as_str() {
        "amd" | "red" | "crimson" => Color32::from_rgb(221, 0, 49),
        "nvidia" | "green" | "emerald" => Color32::from_rgb(118, 185, 0),
        "intel" | "blue" | "sapphire" => Color32::from_rgb(8, 109, 183),
        "lime" => Color32::from_rgb(163, 230, 53),
        "yellow" | "solar" => Color32::from_rgb(250, 204, 21),
        "amber" | "orange" => Color32::from_rgb(245, 158, 11),
        "pink" | "rose" => Color32::from_rgb(244, 63, 94),
        "purple" | "violet" => Color32::from_rgb(168, 85, 247),
        _ => Color32::from_rgb(221, 0, 49),
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

// Modern squared button matching GPU Screen Recorder Button styling
fn squared_button(ui: &mut egui::Ui, text: &str, active: bool, accent: Color32) -> bool {
    let fill = if active {
        accent
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 120)
    };
    let stroke = if active {
        Stroke::new(1.0_f32, accent)
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 20))
    };
    let text_color = if active {
        if accent.r() as u16 + accent.g() as u16 + accent.b() as u16 > 400 {
            Color32::from_rgb(10, 15, 6)
        } else {
            Color32::WHITE
        }
    } else {
        Color32::from_rgb(220, 220, 225)
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

    // Minimalist Line Chevrons (≪) - Crisp, high-tech stroked rewind glyph
    let stroke_w = (radius * 0.10).clamp(1.7, 2.4);
    let stroke = Stroke::new(stroke_w, color);
    let chevron_h = radius * 0.62;
    let chevron_w = radius * 0.38;
    let gap = radius * 0.36;
    let total_w = chevron_w + gap;
    let left_x = center.x - total_w * 0.5;

    for i in 0..2 {
        let tip_x = left_x + i as f32 * gap;
        let base_x = tip_x + chevron_w;
        let top = egui::pos2(base_x, center.y - chevron_h);
        let tip = egui::pos2(tip_x, center.y);
        let bot = egui::pos2(base_x, center.y + chevron_h);

        painter.add(egui::epaint::PathShape::line(
            vec![top, tip, bot],
            stroke,
        ));
    }
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

// GPU Screen Recorder 1:1 Action Card Renderer
#[allow(clippy::too_many_arguments)]
fn render_gsr_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    title: &str,
    icon_tex: Option<&egui::TextureHandle>,
    fallback_icon: impl FnOnce(&egui::Painter, egui::Pos2),
    status_text: &str,
    is_active: bool,
    dropdown_open: bool,
    accent: Color32,
) -> bool {
    let resp = ui.allocate_rect(rect, egui::Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();

    // 1. Background fill & outlines
    if dropdown_open {
        // Solid pitch black + vibrant top accent bar (3px)
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));
        let top_bar = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 3.0));
        painter.rect_filled(top_bar, CornerRadius::ZERO, accent);
    } else if hovered {
        // Solid pitch black + full card rectangular accent outline (2px)
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));
        painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0_f32, accent), egui::StrokeKind::Inside);
    } else {
        // Translucent dark background (Color(0, 0, 0, 180))
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 180));
    }

    // 2. Title text (top margin ~18px, bold sans, white)
    let title_pos = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.085 + 4.0);
    painter.text(
        title_pos,
        egui::Align2::CENTER_CENTER,
        title,
        FontId::proportional(14.5),
        Color32::WHITE,
    );

    // 3. Center Icon (height ~88px, tinted to accent if active, else white)
    let icon_tint = if is_active { accent } else { Color32::WHITE };
    if let Some(tex) = icon_tex {
        draw_texture_centered(painter, tex, rect.center(), rect.height() * 0.42, icon_tint);
    } else {
        fallback_icon(painter, rect.center());
    }

    // 4. Description/Status text (bottom margin ~18px, accent if active, else #969696)
    let status_pos = egui::pos2(rect.center().x, rect.bottom() - rect.height() * 0.085 - 4.0);
    let status_color = if is_active { accent } else { Color32::from_rgb(150, 150, 150) };
    painter.text(
        status_pos,
        egui::Align2::CENTER_CENTER,
        status_text,
        FontId::proportional(12.5),
        status_color,
    );

    resp.clicked()
}

// GPU Screen Recorder 1:1 Dropdown Menu Item Renderer
fn render_dropdown_item(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    title: &str,
    hotkey: Option<&str>,
    icon_tex: Option<&egui::TextureHandle>,
    accent: Color32,
    is_last: bool,
) -> bool {
    let resp = ui.allocate_rect(rect, egui::Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();

    // Background: solid black
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));

    // Hover outline
    if hovered {
        painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0_f32, accent), egui::StrokeKind::Inside);
    }

    // Left Icon
    let mut text_x = rect.left() + 16.0;
    if let Some(tex) = icon_tex {
        let icon_center = egui::pos2(rect.left() + 24.0, rect.center().y);
        draw_texture_centered(painter, tex, icon_center, 18.0, if hovered { accent } else { Color32::WHITE });
        text_x = rect.left() + 44.0;
    }

    // Title
    painter.text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(12.5),
        if hovered { accent } else { Color32::WHITE },
    );

    // Right Hotkey
    if let Some(hk) = hotkey && !hk.trim().is_empty() {
        painter.text(
            egui::pos2(rect.right() - 14.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            hk,
            FontId::proportional(11.0),
            if hovered { accent } else { Color32::from_rgba_unmultiplied(255, 255, 255, 140) },
        );
    }

    // 1px divider
    if !is_last && !hovered {
        painter.line_segment(
            [egui::pos2(rect.left(), rect.bottom()), egui::pos2(rect.right(), rect.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgb(32, 32, 36)),
        );
    }

    resp.clicked()
}

// Section card helper matching GPU Screen Recorder Subsection design (bg_color: #191E22, bold white title)
fn render_section_card(ui: &mut egui::Ui, header: &str, _accent: Color32, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(25, 30, 34))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 12)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(16_i8, 12_i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !header.is_empty() {
                ui.label(
                    egui::RichText::new(header)
                        .size(12.5)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.add_space(8.0);
            }
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
    stream_dropdown_open: bool,
    stream_service_idx: usize,
    stream_key: String,
    stream_url: String,
    textures: Option<OverlayTextures>,
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
    #[allow(dead_code)]
    update_dismissed: bool,
    auto_check_updates: bool,
    autostart_replay: bool,
    autostart_overlay: bool,
    pub settings_view_advanced: bool,
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
            stream_dropdown_open: false,
            stream_service_idx: 0,
            stream_key: String::new(),
            stream_url: "rtmp://live.twitch.tv/app/".to_string(),
            textures: None,
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
            settings_view_advanced: false,
            hud_notification: None,
        }
    }

    pub fn save_and_apply_settings(&mut self) {
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
    }

    pub fn reset_settings_to_defaults(&mut self) {
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

    pub fn ensure_textures(&mut self, ctx: &egui::Context) {
        if self.textures.is_none() {
            self.textures = Some(OverlayTextures {
                replay: load_embedded_png(ctx, "gsr_replay", include_bytes!("../assets/images/replay.png")),
                record: load_embedded_png(ctx, "gsr_record", include_bytes!("../assets/images/record.png")),
                stream: load_embedded_png(ctx, "gsr_stream", include_bytes!("../assets/images/stream.png")),
                screenshot: load_embedded_png(ctx, "gsr_screenshot", include_bytes!("../assets/images/screenshot.png")),
                settings_small: load_embedded_png(ctx, "gsr_settings_small", include_bytes!("../assets/images/settings_small.png")),
                settings_extra_small: load_embedded_png(ctx, "gsr_settings_extra_small", include_bytes!("../assets/images/settings_extra_small.png")),
                cross: load_embedded_png(ctx, "gsr_cross", include_bytes!("../assets/images/cross.png")),
                play: load_embedded_png(ctx, "gsr_play", include_bytes!("../assets/images/play.png")),
                pause: load_embedded_png(ctx, "gsr_pause", include_bytes!("../assets/images/pause.png")),
                stop: load_embedded_png(ctx, "gsr_stop", include_bytes!("../assets/images/stop.png")),
                save: load_embedded_png(ctx, "gsr_save", include_bytes!("../assets/images/save.png")),
                settings_large: load_embedded_png(ctx, "gsr_settings_large", include_bytes!("../assets/images/settings.png")),
            });
        }
    }

    pub fn switch_view(&mut self, view: ShadowPlayView, _ctx: &egui::Context) {
        self.current_view = view;
        self.listening_keybind = None;
        self.replay_dropdown_open = false;
        self.record_dropdown_open = false;
        self.stream_dropdown_open = false;
    }

    fn render_top_bar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let top_left = ui.max_rect().min;
        let screen_w = ui.max_rect().width();
        let bar_h = 48.0_f32;
        let bar_rect = egui::Rect::from_min_size(top_left, Vec2::new(screen_w, bar_h));
        let accent = self.accent_color();

        // Top bar background: translucent dark (Color(0, 0, 0, 190))
        ui.painter().rect_filled(bar_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 190));

        // Left side: Scythe Icon on MainHud, or Back button on Subpage
        match self.current_view {
            ShadowPlayView::MainHud => {
                let logo_center = egui::pos2(top_left.x + 28.0, top_left.y + 24.0);
                draw_scythe_icon(ui.painter(), logo_center, 26.0, accent);
            }
            _ => {
                let back_rect = egui::Rect::from_min_size(egui::pos2(top_left.x + 12.0, top_left.y + 10.0), Vec2::new(88.0, 28.0));
                let back_resp = ui.allocate_rect(back_rect, egui::Sense::click());
                let back_hov = back_resp.hovered();

                let bg = if back_hov {
                    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 32)
                } else {
                    Color32::from_rgba_unmultiplied(20, 20, 24, 200)
                };
                let stroke = if back_hov {
                    Stroke::new(1.5_f32, accent)
                } else {
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 30))
                };
                let painter = ui.painter();
                painter.rect(back_rect, CornerRadius::ZERO, bg, stroke, egui::StrokeKind::Inside);
                painter.text(
                    back_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "< BACK",
                    FontId::proportional(11.5),
                    if back_hov { accent } else { Color32::WHITE },
                );

                if back_resp.clicked() {
                    self.switch_view(ShadowPlayView::MainHud, ctx);
                }
            }
        }

        // Center Title
        let title_text = match self.current_view {
            ShadowPlayView::MainHud => "Scythe",
            ShadowPlayView::ReplaySettings => "Instant Replay",
            ShadowPlayView::RecordSettings => "Record",
            ShadowPlayView::StreamSettings => "Livestream",
            ShadowPlayView::GlobalSettings => "Settings",
            ShadowPlayView::ScreenshotSettings => "Screenshot",
            ShadowPlayView::Gallery => "Recordings Gallery",
        };
        ui.painter().text(
            egui::pos2(top_left.x + screen_w * 0.5, top_left.y + 24.0),
            egui::Align2::CENTER_CENTER,
            title_text,
            FontId::proportional(15.5),
            Color32::WHITE,
        );

        // Right side: Square close 'X' button
        let close_size = 32.0_f32;
        let close_rect = egui::Rect::from_min_size(
            egui::pos2(top_left.x + screen_w - close_size - 12.0, top_left.y + (bar_h - close_size) * 0.5),
            Vec2::new(close_size, close_size),
        );
        let close_resp = ui.allocate_rect(close_rect, egui::Sense::click());
        let close_hov = close_resp.hovered();

        let painter = ui.painter();
        if close_hov {
            painter.rect_filled(close_rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));
            painter.rect_stroke(close_rect, CornerRadius::ZERO, Stroke::new(1.5_f32, accent), egui::StrokeKind::Inside);
        }

        if let Some(tex) = &self.textures {
            draw_texture_centered(painter, &tex.cross, close_rect.center(), 14.0, if close_hov { accent } else { Color32::WHITE });
        } else {
            let half = 6.0;
            let c = close_rect.center();
            let stroke = Stroke::new(1.8_f32, if close_hov { accent } else { Color32::WHITE });
            painter.line_segment([c + Vec2::new(-half, -half), c + Vec2::new(half, half)], stroke);
            painter.line_segment([c + Vec2::new(half, -half), c + Vec2::new(-half, half)], stroke);
        }

        if close_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            crate::ipc::clean_overlay_pid();
            std::process::exit(0);
        }
    }

    fn render_main_hud(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let is_recording = self.status.is_recording;
        let rec_dur = self.status.recording_duration_sec;
        let is_replay_active = self.status.is_replay_active;
        let accent = self.accent_color();

        let screen_w = ui.available_width();
        let screen_h = ui.available_height();
        let card_w = 210.0_f32;
        let card_h = 210.0_f32;
        let total_cards_w = 3.0 * card_w; // 630.0, spacing = 0.0!
        let left_pad = ((screen_w - total_cards_w) / 2.0).max(10.0);
        let top_pad = ((screen_h * 0.25) - card_h * 0.5).max(64.0);

        let aux_size = 70.0_f32;
        let aux_gap = 23.0_f32;
        let aux_x = left_pad + total_cards_w + aux_gap;

        let any_dropdown_open = self.replay_dropdown_open || self.record_dropdown_open || self.stream_dropdown_open;
        let hud_rect = egui::Rect::from_min_max(
            egui::pos2(left_pad, top_pad),
            egui::pos2(aux_x + aux_size, top_pad + card_h + if any_dropdown_open { 230.0 } else { 0.0 }),
        );
        self.panel_rect = hud_rect;

        // The 3 seamlessly touching square cards
        let card0_rect = egui::Rect::from_min_size(egui::pos2(left_pad, top_pad), Vec2::new(card_w, card_h));
        let card1_rect = egui::Rect::from_min_size(egui::pos2(left_pad + card_w, top_pad), Vec2::new(card_w, card_h));
        let card2_rect = egui::Rect::from_min_size(egui::pos2(left_pad + card_w * 2.0, top_pad), Vec2::new(card_w, card_h));

        let textures = self.textures.clone();
        let tex_replay = textures.as_ref().map(|t| &t.replay);
        let tex_record = textures.as_ref().map(|t| &t.record);
        let tex_stream = textures.as_ref().map(|t| &t.stream);
        let tex_screenshot = textures.as_ref().map(|t| &t.screenshot);
        let tex_settings_small = textures.as_ref().map(|t| &t.settings_small);
        let tex_settings_xs = textures.as_ref().map(|t| &t.settings_extra_small);
        let tex_play = textures.as_ref().map(|t| &t.play);
        let tex_pause = textures.as_ref().map(|t| &t.pause);
        let tex_stop = textures.as_ref().map(|t| &t.stop);
        let tex_save = textures.as_ref().map(|t| &t.save);

        // Card 0: Instant Replay
        let c0_clicked = render_gsr_card(
            ui,
            card0_rect,
            "Instant Replay",
            tex_replay,
            |painter, center| {
                draw_replay_icon(painter, center, 24.0, is_replay_active, accent);
            },
            if is_replay_active { "Turned on" } else { "Off" },
            is_replay_active,
            self.replay_dropdown_open,
            accent,
        );
        if c0_clicked {
            self.replay_dropdown_open = !self.replay_dropdown_open;
            self.record_dropdown_open = false;
            self.stream_dropdown_open = false;
        }

        // Card 1: Record
        let rec_status_str = if is_recording {
            let mins = rec_dur / 60;
            let secs = rec_dur % 60;
            format!("Recording {:02}:{:02}", mins, secs)
        } else {
            "Not recording".to_string()
        };
        let c1_clicked = render_gsr_card(
            ui,
            card1_rect,
            "Record",
            tex_record,
            |painter, center| {
                draw_record_icon(painter, center, 24.0, is_recording, self.anim_time);
            },
            &rec_status_str,
            is_recording,
            self.record_dropdown_open,
            accent,
        );
        if c1_clicked {
            self.record_dropdown_open = !self.record_dropdown_open;
            self.replay_dropdown_open = false;
            self.stream_dropdown_open = false;
        }

        // Card 2: Livestream
        let c2_clicked = render_gsr_card(
            ui,
            card2_rect,
            "Livestream",
            tex_stream,
            |painter, center| {
                draw_settings_icon(painter, center, 24.0, Color32::WHITE);
            },
            "Not streaming",
            false,
            self.stream_dropdown_open,
            accent,
        );
        if c2_clicked {
            self.stream_dropdown_open = !self.stream_dropdown_open;
            self.replay_dropdown_open = false;
            self.record_dropdown_open = false;
        }

        // Auxiliary buttons (Screenshot & Settings)
        let aux_top_y = top_pad + (card_h - 2.0 * aux_size - 10.0) * 0.5;
        let aux_screenshot_rect = egui::Rect::from_min_size(egui::pos2(aux_x, aux_top_y), Vec2::new(aux_size, aux_size));
        let aux_settings_rect = egui::Rect::from_min_size(egui::pos2(aux_x, aux_top_y + aux_size + 10.0), Vec2::new(aux_size, aux_size));

        // Screenshot auxiliary button
        let sc_resp = ui.allocate_rect(aux_screenshot_rect, egui::Sense::click());
        let sc_hov = sc_resp.hovered();
        let set_resp = ui.allocate_rect(aux_settings_rect, egui::Sense::click());
        let set_hov = set_resp.hovered();

        let p = ui.painter();
        if sc_hov {
            p.rect_filled(aux_screenshot_rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));
            p.rect_stroke(aux_screenshot_rect, CornerRadius::ZERO, Stroke::new(2.0_f32, accent), egui::StrokeKind::Inside);
        } else {
            p.rect_filled(aux_screenshot_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 180));
        }
        if let Some(tex) = tex_screenshot {
            draw_texture_centered(p, tex, aux_screenshot_rect.center(), 32.0, if sc_hov { accent } else { Color32::WHITE });
        }

        if set_hov {
            p.rect_filled(aux_settings_rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));
            p.rect_stroke(aux_settings_rect, CornerRadius::ZERO, Stroke::new(2.0_f32, accent), egui::StrokeKind::Inside);
        } else {
            p.rect_filled(aux_settings_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0, 0, 0, 180));
        }
        if let Some(tex) = tex_settings_small {
            draw_texture_centered(p, tex, aux_settings_rect.center(), 32.0, if set_hov { accent } else { Color32::WHITE });
        }

        if sc_resp.clicked() {
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
            let fname = format!("Screenshot_{}.png", now);
            let out_path = PathBuf::from(&self.output_dir).join(&fname);
            let _ = std::process::Command::new("grim").arg(&out_path).spawn();
            self.show_hud_notification("SCREENSHOT", &format!("Saved to {}", fname), crate::overlay::ToastIcon::Screenshot);
        }

        if set_resp.clicked() {
            self.replay_dropdown_open = false;
            self.record_dropdown_open = false;
            self.stream_dropdown_open = false;
            self.switch_view(ShadowPlayView::GlobalSettings, ctx);
        }

        // Dropdown menus flush beneath cards
        let item_h = 44.0_f32;

        // Instant Replay dropdown
        if self.replay_dropdown_open {
            let drop_y = card0_rect.bottom();
            let r0 = egui::Rect::from_min_size(egui::pos2(card0_rect.left(), drop_y), Vec2::new(card_w, item_h));
            let r1 = egui::Rect::from_min_size(egui::pos2(card0_rect.left(), drop_y + item_h), Vec2::new(card_w, item_h));
            let r2 = egui::Rect::from_min_size(egui::pos2(card0_rect.left(), drop_y + item_h * 2.0), Vec2::new(card_w, item_h));
            let r3 = egui::Rect::from_min_size(egui::pos2(card0_rect.left(), drop_y + item_h * 3.0), Vec2::new(card_w, item_h));
            let r4 = egui::Rect::from_min_size(egui::pos2(card0_rect.left(), drop_y + item_h * 4.0), Vec2::new(card_w, item_h));

            let toggle_title = if is_replay_active { "Turn off" } else { "Turn on" };
            let toggle_icon = if is_replay_active { tex_stop } else { tex_play };
            if render_dropdown_item(ui, r0, toggle_title, None, toggle_icon, accent, false) {
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
            if render_dropdown_item(ui, r1, "Save", save_hk_opt, tex_save, accent, false) {
                if is_replay_active {
                    async_send_command(Command::SaveReplay);
                    self.show_hud_notification("INSTANT REPLAY", "Saved to Videos", crate::overlay::ToastIcon::Replay);
                } else {
                    self.show_hud_notification("INSTANT REPLAY", "Replay is turned off", crate::overlay::ToastIcon::Error);
                }
                self.replay_dropdown_open = false;
            }

            if render_dropdown_item(ui, r2, "Save 1 min", None, tex_save, accent, false) {
                if is_replay_active {
                    async_send_command(Command::SaveReplay);
                    self.show_hud_notification("INSTANT REPLAY", "Saved 1 min replay", crate::overlay::ToastIcon::Replay);
                } else {
                    self.show_hud_notification("INSTANT REPLAY", "Replay is turned off", crate::overlay::ToastIcon::Error);
                }
                self.replay_dropdown_open = false;
            }

            if render_dropdown_item(ui, r3, "Save 10 min", None, tex_save, accent, false) {
                if is_replay_active {
                    async_send_command(Command::SaveReplay);
                    self.show_hud_notification("INSTANT REPLAY", "Saved 10 min replay", crate::overlay::ToastIcon::Replay);
                } else {
                    self.show_hud_notification("INSTANT REPLAY", "Replay is turned off", crate::overlay::ToastIcon::Error);
                }
                self.replay_dropdown_open = false;
            }

            if render_dropdown_item(ui, r4, "Settings", None, tex_settings_xs, accent, true) {
                self.switch_view(ShadowPlayView::ReplaySettings, ctx);
            }
        }

        // Record dropdown
        if self.record_dropdown_open {
            let drop_y = card1_rect.bottom();
            let r0 = egui::Rect::from_min_size(egui::pos2(card1_rect.left(), drop_y), Vec2::new(card_w, item_h));
            let r1 = egui::Rect::from_min_size(egui::pos2(card1_rect.left(), drop_y + item_h), Vec2::new(card_w, item_h));
            let r2 = egui::Rect::from_min_size(egui::pos2(card1_rect.left(), drop_y + item_h * 2.0), Vec2::new(card_w, item_h));

            let rec_toggle_title = if is_recording { "Stop Recording" } else { "Start Recording" };
            let rec_toggle_icon = if is_recording { tex_stop } else { tex_play };
            let rec_hk = self.config.record_hotkey.trim();
            let rec_hk_opt = if rec_hk.is_empty() { None } else { Some(rec_hk) };
            if render_dropdown_item(ui, r0, rec_toggle_title, rec_hk_opt, rec_toggle_icon, accent, false) {
                async_send_command(Command::ToggleRecording);
                if is_recording {
                    self.show_hud_notification("RECORDING", "Recording saved", crate::overlay::ToastIcon::Save);
                } else {
                    self.show_hud_notification("RECORDING", "Recording started", crate::overlay::ToastIcon::Record);
                }
                self.record_dropdown_open = false;
            }

            let pause_hk_opt: Option<&str> = None;
            if render_dropdown_item(ui, r1, "Pause", pause_hk_opt, tex_pause, accent, false) {
                self.record_dropdown_open = false;
            }

            if render_dropdown_item(ui, r2, "Settings", None, tex_settings_xs, accent, true) {
                self.switch_view(ShadowPlayView::RecordSettings, ctx);
            }
        }

        // Livestream dropdown
        if self.stream_dropdown_open {
            let drop_y = card2_rect.bottom();
            let r0 = egui::Rect::from_min_size(egui::pos2(card2_rect.left(), drop_y), Vec2::new(card_w, item_h));
            let r1 = egui::Rect::from_min_size(egui::pos2(card2_rect.left(), drop_y + item_h), Vec2::new(card_w, item_h));

            if render_dropdown_item(ui, r0, "Start Streaming", None, tex_play, accent, false) {
                self.show_hud_notification("LIVESTREAM", "Streaming not configured", crate::overlay::ToastIcon::Info);
                self.stream_dropdown_open = false;
            }

            if render_dropdown_item(ui, r1, "Settings", None, tex_settings_xs, accent, true) {
                self.switch_view(ShadowPlayView::StreamSettings, ctx);
            }
        }
    }
    fn render_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let screen_w = ui.available_width();
        let screen_h = ui.available_height();

        // Exact GsrPage geometry formula from scratch/gsr-ui/src/gui/GsrPage.cpp:
        // content_page_size = (window_size * vec2(0.3333f, 0.70f)).floor();
        let content_w = (screen_w * 0.3333).clamp(580.0, 680.0);
        let content_h = (screen_h * 0.70).clamp(520.0, 780.0);
        let spacing = (screen_w / 50.0).clamp(20.0, 38.0);
        let side_w = (screen_w / 10.0).clamp(160.0, 192.0);

        let content_x = ((screen_w - content_w) * 0.5).floor();
        let content_y = (48.0 + (screen_h - 48.0 - content_h) * 0.5).floor().max(52.0);

        let left_rect = egui::Rect::from_min_size(
            egui::pos2(content_x - spacing - side_w, content_y),
            egui::vec2(side_w, side_w),
        );
        let content_rect = egui::Rect::from_min_size(
            egui::pos2(content_x, content_y),
            egui::vec2(content_w, content_h),
        );
        let right_x = content_x + content_w + spacing;
        let btn_h = (screen_h / 15.0).clamp(52.0, 64.0);
        let btn_spacing = (screen_h * 0.015).clamp(12.0, 16.0);

        // Entire freestanding region bounding box for outside-click dismiss
        self.panel_rect = egui::Rect::from_min_max(
            egui::pos2(left_rect.left(), content_y),
            egui::pos2(right_x + side_w, content_y + content_h),
        );

        let accent = self.accent_color();
        let textures = self.textures.clone();

        let (cat_title, cat_tex) = match self.current_view {
            ShadowPlayView::ReplaySettings => (
                "INSTANT REPLAY",
                textures.as_ref().map(|t| t.replay.clone()),
            ),
            ShadowPlayView::RecordSettings => (
                "RECORD",
                textures.as_ref().map(|t| t.record.clone()),
            ),
            ShadowPlayView::StreamSettings => (
                "LIVESTREAM",
                textures.as_ref().map(|t| t.stream.clone()),
            ),
            ShadowPlayView::GlobalSettings => (
                "SETTINGS",
                textures.as_ref().map(|t| t.settings_large.clone()),
            ),
            ShadowPlayView::ScreenshotSettings => (
                "SCREENSHOT",
                textures.as_ref().map(|t| t.screenshot.clone()),
            ),
            _ => (
                "SETTINGS",
                textures.as_ref().map(|t| t.settings_large.clone()),
            ),
        };

        // 2. RIGHT ELEMENT: Freestanding Button Stack ("Back", "Save", "Defaults")
        // Allocate interactive rects first
        let back_rect = egui::Rect::from_min_size(egui::pos2(right_x, content_y), egui::vec2(side_w, btn_h));
        let back_resp = ui.allocate_rect(back_rect, egui::Sense::click());
        let back_hovered = back_resp.hovered();
        let back_clicked = back_resp.clicked();

        let save_rect = egui::Rect::from_min_size(
            egui::pos2(right_x, content_y + btn_h + btn_spacing),
            egui::vec2(side_w, btn_h),
        );
        let save_resp = ui.allocate_rect(save_rect, egui::Sense::click());
        let save_hovered = save_resp.hovered();
        let save_clicked = save_resp.clicked();

        let def_rect = egui::Rect::from_min_size(
            egui::pos2(right_x, content_y + (btn_h + btn_spacing) * 2.0_f32),
            egui::vec2(side_w, btn_h),
        );
        let def_resp = ui.allocate_rect(def_rect, egui::Sense::click());
        let def_hovered = def_resp.hovered();
        let def_clicked = def_resp.clicked();

        if back_clicked {
            self.save_and_apply_settings();
            self.switch_view(ShadowPlayView::MainHud, ctx);
            return;
        }

        if save_clicked {
            self.save_and_apply_settings();
            self.show_hud_notification("SETTINGS", "Settings Saved & Applied!", crate::overlay::ToastIcon::Info);
            self.switch_view(ShadowPlayView::MainHud, ctx);
            return;
        }

        if def_clicked {
            self.reset_settings_to_defaults();
        }

        let painter = ui.painter();

        // 1. LEFT ELEMENT: Freestanding Square Page Label Card (Pure Black, Centered Icon, Top Title, Bottom "Settings")
        painter.rect_filled(left_rect, CornerRadius::ZERO, Color32::from_rgb(0, 0, 0));
        painter.rect_stroke(
            left_rect,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 15)),
            egui::StrokeKind::Inside,
        );

        let text_margin = left_rect.height() * 0.085_f32;

        // Top title
        painter.text(
            egui::pos2(left_rect.center().x, left_rect.top() + text_margin + 6.0_f32),
            egui::Align2::CENTER_CENTER,
            cat_title,
            FontId::proportional(13.0),
            Color32::WHITE,
        );

        // Center icon
        let icon_h = (left_rect.height() * 0.48_f32).floor();
        if let Some(tex) = &cat_tex {
            draw_texture_centered(painter, tex, left_rect.center(), icon_h, accent);
        } else {
            match self.current_view {
                ShadowPlayView::ReplaySettings => draw_replay_icon(painter, left_rect.center(), icon_h * 0.45_f32, true, accent),
                ShadowPlayView::RecordSettings => draw_record_icon(painter, left_rect.center(), icon_h * 0.45_f32, false, self.anim_time),
                _ => draw_settings_icon(painter, left_rect.center(), icon_h * 0.45_f32, accent),
            }
        }

        // Bottom text: "Settings"
        painter.text(
            egui::pos2(left_rect.center().x, left_rect.bottom() - text_margin - 6.0_f32),
            egui::Align2::CENTER_CENTER,
            "Settings",
            FontId::proportional(12.5),
            Color32::from_rgb(180, 180, 185),
        );

        // Draw Right Element buttons
        // Button 1: "Back" (GSR page_bg_color #262B2F, accent border on hover)
        painter.rect_filled(
            back_rect,
            CornerRadius::ZERO,
            if back_hovered {
                Color32::from_rgb(46, 52, 57)
            } else {
                Color32::from_rgb(38, 43, 47)
            },
        );
        if back_hovered {
            painter.rect_stroke(back_rect, CornerRadius::ZERO, Stroke::new(1.5_f32, accent), egui::StrokeKind::Inside);
        }
        painter.text(
            back_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Back",
            FontId::proportional(14.5),
            Color32::WHITE,
        );

        // Button 2: "Save" (GSR accent background, white border on hover)
        let save_text_color = if accent.r() as u16 + accent.g() as u16 + accent.b() as u16 > 400 {
            Color32::from_rgb(10, 15, 6)
        } else {
            Color32::WHITE
        };
        painter.rect_filled(save_rect, CornerRadius::ZERO, accent);
        if save_hovered {
            painter.rect_stroke(save_rect, CornerRadius::ZERO, Stroke::new(1.5_f32, Color32::WHITE), egui::StrokeKind::Inside);
        }
        painter.text(
            save_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Save",
            FontId::proportional(14.5),
            save_text_color,
        );

        // Button 3: "Defaults" (GSR page_bg_color #262B2F, accent border on hover)
        painter.rect_filled(
            def_rect,
            CornerRadius::ZERO,
            if def_hovered {
                Color32::from_rgb(46, 52, 57)
            } else {
                Color32::from_rgb(38, 43, 47)
            },
        );
        if def_hovered {
            painter.rect_stroke(def_rect, CornerRadius::ZERO, Stroke::new(1.5_f32, accent), egui::StrokeKind::Inside);
        }
        painter.text(
            def_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Defaults",
            FontId::proportional(14.0),
            Color32::from_rgb(190, 195, 200),
        );

        // 3. CENTER ELEMENT: Content Box (#262B2F with top accent border)
        painter.rect_filled(content_rect, CornerRadius::ZERO, Color32::from_rgb(38, 43, 47));
        painter.rect_filled(
            egui::Rect::from_min_size(content_rect.min, egui::vec2(content_w, 4.0)),
            CornerRadius::ZERO,
            accent,
        );

        let has_view_toggle = matches!(
            self.current_view,
            ShadowPlayView::ReplaySettings | ShadowPlayView::RecordSettings | ShadowPlayView::StreamSettings
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
            ui.add_space(14.0);

            // [ Simple view ]   [ Advanced view ] Horizontal Radio Buttons
            if has_view_toggle {
                ui.horizontal(|ui| {
                    let total_radio_w = 260.0;
                    let pad = ((content_w - total_radio_w) * 0.5).max(0.0);
                    ui.add_space(pad);

                    let item_w = 126.0;
                    let item_h = 28.0;

                    // Simple view button
                    let (s_rect, s_resp) = ui.allocate_exact_size(egui::vec2(item_w, item_h), egui::Sense::click());
                    let s_active = !self.settings_view_advanced;
                    let s_hover = s_resp.hovered();

                    ui.painter().rect_filled(
                        s_rect,
                        CornerRadius::ZERO,
                        if s_active { accent } else { Color32::from_rgba_unmultiplied(0, 0, 0, 120) },
                    );
                    if s_hover {
                        ui.painter().rect_stroke(
                            s_rect,
                            CornerRadius::ZERO,
                            Stroke::new(1.5_f32, if s_active { Color32::WHITE } else { accent }),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let s_text_col = if s_active {
                        if accent.r() as u16 + accent.g() as u16 + accent.b() as u16 > 400 {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        }
                    } else {
                        Color32::WHITE
                    };
                    ui.painter().text(
                        s_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Simple view",
                        FontId::proportional(12.5),
                        s_text_col,
                    );
                    if s_resp.clicked() {
                        self.settings_view_advanced = false;
                    }

                    ui.add_space(8.0);

                    // Advanced view button
                    let (a_rect, a_resp) = ui.allocate_exact_size(egui::vec2(item_w, item_h), egui::Sense::click());
                    let a_active = self.settings_view_advanced;
                    let a_hover = a_resp.hovered();

                    ui.painter().rect_filled(
                        a_rect,
                        CornerRadius::ZERO,
                        if a_active { accent } else { Color32::from_rgba_unmultiplied(0, 0, 0, 120) },
                    );
                    if a_hover {
                        ui.painter().rect_stroke(
                            a_rect,
                            CornerRadius::ZERO,
                            Stroke::new(1.5_f32, if a_active { Color32::WHITE } else { accent }),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let a_text_col = if a_active {
                        if accent.r() as u16 + accent.g() as u16 + accent.b() as u16 > 400 {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        }
                    } else {
                        Color32::WHITE
                    };
                    ui.painter().text(
                        a_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Advanced view",
                        FontId::proportional(12.5),
                        a_text_col,
                    );
                    if a_resp.clicked() {
                        self.settings_view_advanced = true;
                    }
                });
                ui.add_space(14.0);
            }

            let scroll_h = content_h - (if has_view_toggle { 70.0 } else { 30.0 });
            egui::ScrollArea::vertical()
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(content_w - 36.0);
                    ui.add_space(2.0);

                    match self.current_view {
                        ShadowPlayView::ReplaySettings => {
                            // 1. File info
                            render_section_card(ui, "File info", accent, |ui| {
                                ui.label(egui::RichText::new("Directory to save replays:").size(12.0).color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(ui.available_width() - 140.0));
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

                                ui.label(egui::RichText::new("Replay buffer duration:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for sec in [15, 30, 60, 120, 300, 600] {
                                        let label = if sec >= 60 { format!("{}m", sec / 60) } else { format!("{}s", sec) };
                                        if squared_button(ui, &label, self.replay_sec == sec, accent) {
                                            self.replay_sec = sec;
                                            self.replay_sec_input_str = sec.to_string();
                                        }
                                    }
                                    if self.settings_view_advanced {
                                        ui.add_space(6.0);
                                        ui.label(egui::RichText::new("Custom:").size(11.0).color(Color32::from_rgb(150, 155, 160)));
                                        let edit_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.replay_sec_input_str)
                                                .desired_width(50.0)
                                                .font(FontId::monospace(11.5))
                                        );
                                        if edit_resp.changed()
                                            && let Ok(parsed) = self.replay_sec_input_str.trim().parse::<u32>()
                                            && (5..=1800).contains(&parsed) {
                                                self.replay_sec = parsed;
                                            }
                                        ui.label(egui::RichText::new("sec").size(10.5).color(Color32::from_rgb(150, 155, 160)));
                                    }
                                });

                                if self.settings_view_advanced {
                                    ui.add_space(6.0);
                                    let est_mb = ((self.replay_sec as f64) * (self.bitrate_mbps as f64 * 1000.0)) / 8192.0;
                                    ui.label(
                                        egui::RichText::new(format!("Estimated video max file size in RAM: {:.1} MB", est_mb))
                                            .size(11.0)
                                            .color(Color32::from_rgb(160, 165, 170)),
                                    );
                                }
                            });

                            ui.add_space(12.0);

                            // 2. Video
                            render_section_card(ui, "Video", accent, |ui| {
                                ui.label(egui::RichText::new("Target framerate (FPS):").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for fps in [30, 60, 120, 144, 240] {
                                        if squared_button(ui, &fps.to_string(), self.target_fps == fps, accent) {
                                            self.target_fps = fps;
                                            self.fps_input_str = fps.to_string();
                                        }
                                    }
                                    if self.settings_view_advanced {
                                        ui.add_space(6.0);
                                        ui.label(egui::RichText::new("Custom:").size(11.0).color(Color32::from_rgb(150, 155, 160)));
                                        let edit_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.fps_input_str)
                                                .desired_width(50.0)
                                                .font(FontId::monospace(11.5))
                                        );
                                        if edit_resp.changed()
                                            && let Ok(parsed) = self.fps_input_str.trim().parse::<u32>()
                                            && (15..=360).contains(&parsed) {
                                                self.target_fps = parsed;
                                            }
                                    }
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(egui::RichText::new("Video bitrate:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for mbps in [10, 20, 35, 50, 80] {
                                        if squared_button(ui, &format!("{}M", mbps), self.bitrate_mbps == mbps, accent) {
                                            self.bitrate_mbps = mbps;
                                            self.bitrate_input_str = mbps.to_string();
                                        }
                                    }
                                    if self.settings_view_advanced {
                                        ui.add_space(6.0);
                                        let mut br = self.bitrate_mbps;
                                        if ui.add(egui::Slider::new(&mut br, 5..=150).suffix(" Mbps").step_by(5.0)).changed() {
                                            self.bitrate_mbps = br;
                                            self.bitrate_input_str = br.to_string();
                                        }
                                    }
                                });

                                if self.settings_view_advanced {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    ui.label(egui::RichText::new("Encoder codec:").size(12.0).color(Color32::WHITE));
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        for (codec_key, label) in [("h264", "H.264"), ("hevc", "HEVC / H.265"), ("av1", "AV1")] {
                                            if squared_button(ui, label, self.video_codec == codec_key, accent) {
                                                self.video_codec = codec_key.to_string();
                                            }
                                        }
                                    });

                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Record mouse cursor").size(12.0).color(Color32::WHITE));
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
                                }
                            });

                            ui.add_space(12.0);

                            // 3. Audio
                            render_section_card(ui, "Audio", accent, |ui| {
                                ui.label(egui::RichText::new("Audio source:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let modes = [("system", "System"), ("mic", "Microphone"), ("both", "Both"), ("muted", "Muted")];
                                    for (idx, (_, m_label)) in modes.iter().enumerate() {
                                        if squared_button(ui, m_label, self.audio_mode_idx == idx, accent) {
                                            self.audio_mode_idx = idx;
                                        }
                                    }
                                });

                                if self.settings_view_advanced {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Mic Vol:").size(12.0).color(Color32::WHITE));
                                        let mut mv = self.mic_volume_pct;
                                        if ui.add(egui::Slider::new(&mut mv, 0..=200).suffix("%")).changed() {
                                            self.mic_volume_pct = mv;
                                        }
                                        ui.add_space(6.0);
                                        render_vu_meter(ui, self.mic_vu, 65.0, 18.0, "MIC");
                                    });

                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Sys Vol:").size(12.0).color(Color32::WHITE));
                                        let mut sv = self.system_volume_pct;
                                        if ui.add(egui::Slider::new(&mut sv, 0..=200).suffix("%")).changed() {
                                            self.system_volume_pct = sv;
                                        }
                                        ui.add_space(6.0);
                                        render_vu_meter(ui, self.sys_vu, 65.0, 18.0, "SYS");
                                    });
                                }
                            });

                            ui.add_space(12.0);

                            // 4. Autostart
                            render_section_card(ui, "Autostart", accent, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new("Start replay automatically on login").size(12.0).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("Launch the background engine so replay buffer is active immediately.").size(10.5).color(Color32::from_rgb(150, 155, 160)));
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
                            });
                        }
                        ShadowPlayView::RecordSettings => {
                            // 1. File info
                            render_section_card(ui, "File info", accent, |ui| {
                                ui.label(egui::RichText::new("Directory to save recordings:").size(12.0).color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(ui.available_width() - 140.0));
                                    if squared_button(ui, "Change", false, accent) {
                                        pick_folder(&self.output_dir, self.folder_tx.clone(), self.folder_picking_active.clone());
                                    }
                                    if squared_button(ui, "Open", false, accent) {
                                        open_folder(&ScytheConfig::expand_tilde(&self.output_dir));
                                    }
                                });
                            });

                            ui.add_space(12.0);

                            // 2. Video
                            render_section_card(ui, "Video", accent, |ui| {
                                ui.label(egui::RichText::new("Target framerate (FPS):").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for fps in [30, 60, 120, 144, 240] {
                                        if squared_button(ui, &fps.to_string(), self.target_fps == fps, accent) {
                                            self.target_fps = fps;
                                            self.fps_input_str = fps.to_string();
                                        }
                                    }
                                    if self.settings_view_advanced {
                                        ui.add_space(6.0);
                                        ui.label(egui::RichText::new("Custom:").size(11.0).color(Color32::from_rgb(150, 155, 160)));
                                        let edit_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.fps_input_str)
                                                .desired_width(50.0)
                                                .font(FontId::monospace(11.5))
                                        );
                                        if edit_resp.changed()
                                            && let Ok(parsed) = self.fps_input_str.trim().parse::<u32>()
                                            && (15..=360).contains(&parsed) {
                                                self.target_fps = parsed;
                                            }
                                    }
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(egui::RichText::new("Video bitrate:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for mbps in [10, 20, 35, 50, 80] {
                                        if squared_button(ui, &format!("{}M", mbps), self.bitrate_mbps == mbps, accent) {
                                            self.bitrate_mbps = mbps;
                                            self.bitrate_input_str = mbps.to_string();
                                        }
                                    }
                                    if self.settings_view_advanced {
                                        ui.add_space(6.0);
                                        let mut br = self.bitrate_mbps;
                                        if ui.add(egui::Slider::new(&mut br, 5..=150).suffix(" Mbps").step_by(5.0)).changed() {
                                            self.bitrate_mbps = br;
                                            self.bitrate_input_str = br.to_string();
                                        }
                                    }
                                });

                                if self.settings_view_advanced {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    ui.label(egui::RichText::new("Encoder codec:").size(12.0).color(Color32::WHITE));
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        for (codec_key, label) in [("h264", "H.264"), ("hevc", "HEVC / H.265"), ("av1", "AV1")] {
                                            if squared_button(ui, label, self.video_codec == codec_key, accent) {
                                                self.video_codec = codec_key.to_string();
                                            }
                                        }
                                    });

                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Record mouse cursor").size(12.0).color(Color32::WHITE));
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
                                }
                            });

                            ui.add_space(12.0);

                            // 3. Audio
                            render_section_card(ui, "Audio", accent, |ui| {
                                ui.label(egui::RichText::new("Audio source:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let modes = [("system", "System"), ("mic", "Microphone"), ("both", "Both"), ("muted", "Muted")];
                                    for (idx, (_, m_label)) in modes.iter().enumerate() {
                                        if squared_button(ui, m_label, self.audio_mode_idx == idx, accent) {
                                            self.audio_mode_idx = idx;
                                        }
                                    }
                                });

                                if self.settings_view_advanced {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Mic Vol:").size(12.0).color(Color32::WHITE));
                                        let mut mv = self.mic_volume_pct;
                                        if ui.add(egui::Slider::new(&mut mv, 0..=200).suffix("%")).changed() {
                                            self.mic_volume_pct = mv;
                                        }
                                        ui.add_space(6.0);
                                        render_vu_meter(ui, self.mic_vu, 65.0, 18.0, "MIC");
                                    });

                                    ui.add_space(8.0);

                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Sys Vol:").size(12.0).color(Color32::WHITE));
                                        let mut sv = self.system_volume_pct;
                                        if ui.add(egui::Slider::new(&mut sv, 0..=200).suffix("%")).changed() {
                                            self.system_volume_pct = sv;
                                        }
                                        ui.add_space(6.0);
                                        render_vu_meter(ui, self.sys_vu, 65.0, 18.0, "SYS");
                                    });
                                }
                            });
                        }
                        ShadowPlayView::StreamSettings => {
                            // 1. Streaming info
                            render_section_card(ui, "Streaming info", accent, |ui| {
                                ui.label(egui::RichText::new("Streaming service:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let services = ["Twitch", "YouTube", "Custom RTMP"];
                                    for (idx, serv) in services.iter().enumerate() {
                                        if squared_button(ui, serv, self.stream_service_idx == idx, accent) {
                                            self.stream_service_idx = idx;
                                            self.stream_url = match idx {
                                                0 => "rtmp://live.twitch.tv/app/".to_string(),
                                                1 => "rtmp://a.rtmp.youtube.com/live2".to_string(),
                                                _ => self.stream_url.clone(),
                                            };
                                        }
                                    }
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(egui::RichText::new("Server URL:").size(12.0).color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.add(egui::TextEdit::singleline(&mut self.stream_url).desired_width(ui.available_width()));

                                ui.add_space(8.0);

                                ui.label(egui::RichText::new("Stream key:").size(12.0).color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.add(egui::TextEdit::singleline(&mut self.stream_key).password(true).desired_width(ui.available_width()));
                            });

                            ui.add_space(12.0);

                            // 2. Video
                            render_section_card(ui, "Video", accent, |ui| {
                                ui.label(egui::RichText::new("Target framerate:").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for fps in [30, 60] {
                                        if squared_button(ui, &format!("{} FPS", fps), self.target_fps == fps, accent) {
                                            self.target_fps = fps;
                                            self.fps_input_str = fps.to_string();
                                        }
                                    }
                                });

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.label(egui::RichText::new("Video bitrate (Kbps):").size(12.0).color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for kbps in [3000, 4500, 6000, 8000] {
                                        let mbps = kbps / 1000;
                                        if squared_button(ui, &format!("{}K", kbps), self.bitrate_mbps == mbps, accent) {
                                            self.bitrate_mbps = mbps;
                                            self.bitrate_input_str = mbps.to_string();
                                        }
                                    }
                                });
                            });
                        }
                        ShadowPlayView::GlobalSettings => {
                            // 1. Startup
                            render_section_card(ui, "Startup", accent, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new("Autostart Instant Replay").size(12.0).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("Launch the background recording engine on system login.").size(10.5).color(Color32::from_rgb(150, 155, 160)));
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

                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new("Autostart HUD Overlay").size(12.0).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("Automatically open the HUD overlay menu when logging into desktop.").size(10.5).color(Color32::from_rgb(150, 155, 160)));
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

                            ui.add_space(12.0);

                            // 2. Keyboard hotkeys
                            render_section_card(ui, "Keyboard hotkeys", accent, |ui| {
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
                                                .color(if is_listening { accent } else { Color32::from_rgb(220, 225, 230) }),
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
                                        egui::RichText::new("Listening... Press desired key combination or Esc to cancel.")
                                            .size(11.0)
                                            .strong()
                                            .color(accent),
                                    );
                                }

                                ui.add_space(6.0);
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

                            ui.add_space(12.0);

                            // 3. Appearance
                            render_section_card(ui, "Appearance", accent, |ui| {
                                ui.label(egui::RichText::new("Interface Accent Color:").size(12.0).color(Color32::WHITE));
                                ui.add_space(8.0);
                                ui.horizontal_wrapped(|ui| {
                                    let palettes = [
                                        ("amd", "AMD Radeon Red", Color32::from_rgb(221, 0, 49)),
                                        ("nvidia", "NVIDIA GeForce Green", Color32::from_rgb(118, 185, 0)),
                                        ("intel", "Intel Arc Blue", Color32::from_rgb(8, 109, 183)),
                                        ("blue", "Charming Blue", Color32::from_rgb(56, 189, 248)),
                                        ("green", "Emerald Green", Color32::from_rgb(34, 197, 94)),
                                        ("yellow", "Solar Yellow", Color32::from_rgb(250, 204, 21)),
                                        ("purple", "Royal Purple", Color32::from_rgb(168, 85, 247)),
                                        ("pink", "Neon Pink", Color32::from_rgb(244, 63, 94)),
                                    ];

                                    for (id, name, col) in palettes {
                                        let is_sel = self.config.accent_color.to_lowercase() == id;
                                        let bg = if is_sel {
                                            col
                                        } else {
                                            Color32::from_rgba_unmultiplied(0, 0, 0, 120)
                                        };
                                        let stroke = if is_sel {
                                            Stroke::new(1.5_f32, Color32::WHITE)
                                        } else {
                                            Stroke::new(1.0_f32, col)
                                        };
                                        let text_col = if is_sel {
                                            if col.r() as u16 + col.g() as u16 + col.b() as u16 > 400 {
                                                Color32::BLACK
                                            } else {
                                                Color32::WHITE
                                            }
                                        } else {
                                            Color32::from_rgb(225, 225, 230)
                                        };

                                        let btn = egui::Button::new(
                                            egui::RichText::new(name)
                                                .size(11.5)
                                                .strong()
                                                .color(text_col),
                                        )
                                        .fill(bg)
                                        .stroke(stroke)
                                        .corner_radius(CornerRadius::ZERO)
                                        .min_size(Vec2::new(132.0, 30.0));

                                        if ui.add(btn).clicked() {
                                            self.config.accent_color = id.to_string();
                                            let _ = self.config.save();
                                            self.show_hud_notification("THEME ACCENT", &format!("Selected: {}", name), crate::overlay::ToastIcon::Info);
                                        }
                                    }
                                });
                            });

                            ui.add_space(12.0);

                            // 4. Application info
                            render_section_card(ui, "Application info", accent, |ui| {
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
                                                egui::RichText::new(format!("Up to date (v{})", version))
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
                                        .color(Color32::from_rgb(220, 225, 230)),
                                );
                            });
                        }
                        ShadowPlayView::ScreenshotSettings => {
                            // 1. File info
                            render_section_card(ui, "File info", accent, |ui| {
                                ui.label(egui::RichText::new("Directory to save screenshots:").size(12.0).color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(ui.available_width() - 140.0));
                                    if squared_button(ui, "Change", false, accent) {
                                        pick_folder(&self.output_dir, self.folder_tx.clone(), self.folder_picking_active.clone());
                                    }
                                    if squared_button(ui, "Open", false, accent) {
                                        open_folder(&ScytheConfig::expand_tilde(&self.output_dir));
                                    }
                                });
                            });

                            ui.add_space(12.0);

                            // 2. Image
                            render_section_card(ui, "Image", accent, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Image Format:").size(12.0).color(Color32::WHITE));
                                    ui.label(egui::RichText::new("PNG (Lossless)").size(11.5).color(accent).strong());
                                });
                            });

                            ui.add_space(12.0);

                            // 3. General
                            render_section_card(ui, "General", accent, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Capture mouse cursor").size(12.0).color(Color32::WHITE));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut cur = self.show_cursor;
                                        if toggle_switch(ui, &mut cur, accent).changed() {
                                            self.show_cursor = cur;
                                            self.config.show_cursor = cur;
                                            let _ = self.config.save();
                                        }
                                    });
                                });
                            });
                        }
                        _ => {}
                    }
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
                            self.switch_view(ShadowPlayView::MainHud, ctx);
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
            let total_dur = notif.duration_secs;
            if elapsed < total_dur {
                ctx.request_repaint(); // 60 FPS animation

                let card_w = 340.0_f32;
                let card_h = 56.0_f32;

                // Silky smooth quintic decel entrance & cubic exit slide
                let slide_x = if elapsed < 0.38 {
                    let t = (elapsed / 0.38).min(1.0);
                    let ease = 1.0 - (1.0 - t).powi(4);
                    (1.0 - ease) * card_w
                } else if elapsed < total_dur - 0.35 {
                    0.0
                } else {
                    let t = ((elapsed - (total_dur - 0.35)) / 0.35).min(1.0);
                    let ease = t.powi(3);
                    ease * card_w
                };

                // Dynamic fade transition
                let fade_in = (elapsed / 0.22).clamp(0.0, 1.0);
                let fade_out = ((total_dur - elapsed) / 0.32).clamp(0.0, 1.0);
                let anim_alpha = fade_in.min(fade_out);

                let screen_w = ui.available_width();
                let toast_rect = egui::Rect::from_min_size(
                    egui::pos2(screen_w - card_w + slide_x, 16.0),
                    egui::vec2(card_w, card_h),
                );

                render_scythe_notification_card(
                    ui.painter(),
                    toast_rect,
                    &notif.title,
                    &notif.subtitle,
                    notif.icon,
                    accent,
                    elapsed,
                    total_dur,
                    anim_alpha,
                );
            } else {
                self.hud_notification = None;
            }
        }
    }
}

fn render_scythe_notification_card(
    painter: &egui::Painter,
    rect: egui::Rect,
    title: &str,
    subtitle: &str,
    icon: crate::overlay::ToastIcon,
    accent: Color32,
    elapsed: f32,
    total_dur: f32,
    anim_alpha: f32,
) {
    let active_color = if icon == crate::overlay::ToastIcon::Record || icon == crate::overlay::ToastIcon::Error {
        Color32::from_rgb(239, 68, 68)
    } else {
        accent
    };

    // 1. Entrance accent bloom / pulse highlight during first 0.5s
    let bloom = if elapsed < 0.50 { (1.0 - elapsed / 0.50).powi(2) } else { 0.0 };
    if bloom > 0.01 {
        painter.rect_stroke(
            rect.expand(2.0),
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (bloom * 80.0 * anim_alpha) as u8)),
            egui::StrokeKind::Outside,
        );
    }

    // 2. Deep obsidian dark glass card fill (pitch-black aesthetic)
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(10, 11, 14, (246.0 * anim_alpha) as u8),
    );

    // 3. 1.0px sharp border framing the notification card
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(42, 45, 52, (180.0 * anim_alpha) as u8)),
        egui::StrokeKind::Inside,
    );

    // 4. Signature top solid 3.0px accent bar across the entire card
    let top_bar_rect = egui::Rect::from_min_size(rect.left_top(), Vec2::new(rect.width(), 3.0));
    painter.rect_filled(
        top_bar_rect,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (255.0 * anim_alpha) as u8),
    );

    // 5. Specular sheen line directly beneath the top accent line
    painter.line_segment(
        [rect.left_top() + Vec2::new(1.0, 3.5), rect.right_top() + Vec2::new(-1.0, 3.5)],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, (25.0 * anim_alpha) as u8)),
    );

    // 6. Left square icon badge (38x38 obsidian container with subtle accent border)
    let badge_size = 38.0_f32;
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 9.0, rect.top() + 9.0),
        Vec2::new(badge_size, badge_size),
    );
    painter.rect_filled(
        badge_rect,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(6, 7, 9, (255.0 * anim_alpha) as u8),
    );
    painter.rect_stroke(
        badge_rect,
        CornerRadius::ZERO,
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (110.0 * anim_alpha) as u8)),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [badge_rect.left_top() + Vec2::new(1.0, 1.0), badge_rect.right_top() + Vec2::new(-1.0, 1.0)],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, (20.0 * anim_alpha) as u8)),
    );

    // 7. Render crisp icon centered inside badge
    let icon_center = badge_rect.center();
    let icon_tint = Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (255.0 * anim_alpha) as u8);
    match icon {
        crate::overlay::ToastIcon::Replay => {
            draw_replay_icon(painter, icon_center, 11.5, true, active_color);
        }
        crate::overlay::ToastIcon::Record => {
            draw_record_icon(painter, icon_center, 11.0, true, elapsed);
        }
        crate::overlay::ToastIcon::Save => {
            painter.add(egui::epaint::PathShape::line(
                vec![
                    icon_center + Vec2::new(-6.0, 0.5),
                    icon_center + Vec2::new(-2.0, 4.5),
                    icon_center + Vec2::new(6.0, -4.5),
                ],
                Stroke::new(2.0_f32, icon_tint),
            ));
        }
        crate::overlay::ToastIcon::Cursor => {
            painter.add(egui::epaint::PathShape::line(
                vec![
                    icon_center + Vec2::new(-5.0, -7.0),
                    icon_center + Vec2::new(-5.0, 5.0),
                    icon_center + Vec2::new(-1.5, 2.0),
                    icon_center + Vec2::new(1.5, 7.0),
                    icon_center + Vec2::new(3.5, 6.0),
                    icon_center + Vec2::new(0.5, 1.0),
                    icon_center + Vec2::new(5.0, 1.0),
                    icon_center + Vec2::new(-5.0, -7.0),
                ],
                Stroke::new(1.8_f32, icon_tint),
            ));
        }
        crate::overlay::ToastIcon::Screenshot => {
            let half_w = 7.0;
            let half_h = 5.0;
            let cam_rect = egui::Rect::from_center_size(icon_center + Vec2::new(0.0, 1.0), Vec2::new(half_w * 2.0, half_h * 2.0));
            painter.rect_stroke(cam_rect, CornerRadius::ZERO, Stroke::new(1.8_f32, icon_tint), egui::StrokeKind::Inside);
            painter.circle_stroke(icon_center + Vec2::new(0.0, 1.0), 2.5, Stroke::new(1.6_f32, icon_tint));
            let notch = egui::Rect::from_min_size(icon_center + Vec2::new(-3.5, -half_h - 1.5), Vec2::new(4.0, 2.5));
            painter.rect_filled(notch, CornerRadius::ZERO, icon_tint);
        }
        crate::overlay::ToastIcon::Error => {
            let stroke = Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(239, 68, 68, (255.0 * anim_alpha) as u8));
            painter.line_segment([icon_center + Vec2::new(-5.0, -5.0), icon_center + Vec2::new(5.0, 5.0)], stroke);
            painter.line_segment([icon_center + Vec2::new(5.0, -5.0), icon_center + Vec2::new(-5.0, 5.0)], stroke);
        }
        crate::overlay::ToastIcon::Info => {
            draw_scythe_icon(painter, icon_center, 20.0, active_color);
        }
    }

    // 8. Typography Stack
    let text_left = badge_rect.right() + 12.0;
    painter.text(
        egui::pos2(text_left, rect.top() + 19.0),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        Color32::from_rgba_unmultiplied(250, 250, 252, (255.0 * anim_alpha) as u8),
    );
    painter.text(
        egui::pos2(text_left, rect.top() + 37.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(11.0),
        Color32::from_rgba_unmultiplied(170, 175, 185, (235.0 * anim_alpha) as u8),
    );

    // 9. Animated Countdown Timer Line (bottom edge - vibrant accent glow)
    let progress = 1.0 - (elapsed / total_dur).clamp(0.0, 1.0);
    let bar_h = 3.0_f32;
    let bar_w = (rect.width() * progress).max(0.0);

    // Baseline track
    let bar_track = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - bar_h), egui::vec2(rect.width(), bar_h));
    painter.rect_filled(bar_track, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (35.0 * anim_alpha) as u8));

    // Ambient vibrant glow above progress bar
    let glow_h = 2.5_f32;
    let glow_rect = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - bar_h - glow_h), egui::vec2(bar_w, glow_h));
    painter.rect_filled(glow_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (55.0 * anim_alpha) as u8));

    // Solid vibrant core bar
    let bar_rect = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.bottom() - bar_h), egui::vec2(bar_w, bar_h));
    painter.rect_filled(bar_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(active_color.r(), active_color.g(), active_color.b(), (255.0 * anim_alpha) as u8));

    // Top neon specular edge highlight for intense vibrancy
    painter.line_segment(
        [egui::pos2(rect.left(), rect.bottom() - bar_h), egui::pos2(rect.left() + bar_w, rect.bottom() - bar_h)],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(
            (active_color.r() as u16 + 80).min(255) as u8,
            (active_color.g() as u16 + 80).min(255) as u8,
            (active_color.b() as u16 + 80).min(255) as u8,
            (240.0 * anim_alpha) as u8,
        )),
    );

    // Laser-sharp white leading tip indicator
    if bar_w > 2.0 {
        painter.line_segment(
            [egui::pos2(rect.left() + bar_w, rect.bottom() - bar_h - 1.0), egui::pos2(rect.left() + bar_w, rect.bottom())],
            Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, (255.0 * anim_alpha) as u8)),
        );
    }
}

impl eframe::App for ScytheOverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_textures(ctx);
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
                if self.replay_dropdown_open || self.record_dropdown_open || self.stream_dropdown_open {
                    self.replay_dropdown_open = false;
                    self.record_dropdown_open = false;
                    self.stream_dropdown_open = false;
                } else if self.current_view != ShadowPlayView::MainHud {
                    self.switch_view(ShadowPlayView::MainHud, ctx);
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
                        if self.replay_dropdown_open || self.record_dropdown_open || self.stream_dropdown_open {
                            self.replay_dropdown_open = false;
                            self.record_dropdown_open = false;
                            self.stream_dropdown_open = false;
                        } else if self.current_view != ShadowPlayView::MainHud {
                            self.switch_view(ShadowPlayView::MainHud, ctx);
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
            .frame(egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(0, 0, 0, 80)).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                self.render_top_bar(ctx, ui);
                match self.current_view {
                    ShadowPlayView::MainHud => self.render_main_hud(ctx, ui),
                    ShadowPlayView::ReplaySettings
                    | ShadowPlayView::RecordSettings
                    | ShadowPlayView::StreamSettings
                    | ShadowPlayView::GlobalSettings
                    | ShadowPlayView::ScreenshotSettings => self.render_settings_view(ctx, ui),
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
                let toast_w = 340.0_f32;
                let target_x = (mon_size.x - toast_w).max(0.0);
                let target_y = 16.0;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(toast_w, 56.0)));
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

        let card_w = 340.0_f32;
        let card_h = 56.0_f32;

        let slide_x = if elapsed < 0.38 {
            let t = (elapsed / 0.38).min(1.0);
            let ease = 1.0 - (1.0 - t).powi(4);
            (1.0 - ease) * card_w
        } else if elapsed < total_dur - 0.35 {
            0.0
        } else {
            let t = ((elapsed - (total_dur - 0.35)) / 0.35).min(1.0);
            let ease = t.powi(3);
            ease * card_w
        };

        // Dynamic fade transition
        let fade_in = (elapsed / 0.22).clamp(0.0, 1.0);
        let fade_out = ((total_dur - elapsed) / 0.32).clamp(0.0, 1.0);
        let anim_alpha = fade_in.min(fade_out);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::TRANSPARENT;
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(slide_x, 0.0),
                    Vec2::new(card_w, card_h),
                );
                render_scythe_notification_card(
                    ui.painter(),
                    rect,
                    &self.title,
                    &self.subtitle,
                    self.icon,
                    self.accent,
                    elapsed,
                    total_dur,
                    anim_alpha,
                );
            });
    }
}

pub fn run_egui_toast(title: &str, subtitle: &str, icon: crate::overlay::ToastIcon) {
    let cfg = ScytheConfig::load();
    let accent = resolve_accent_color(&cfg.accent_color);

    let toast_w: f32 = 340.0;
    let toast_h: f32 = 56.0;

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

    let pos_x = (mon_x + screen_w - toast_w).max(0.0);
    let pos_y = 16.0;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gsr_3_column_geometry_non_overlapping() {
        for (screen_w, screen_h) in [(1366.0_f32, 768.0_f32), (1920.0, 1080.0), (2560.0, 1440.0), (3840.0, 2160.0)] {
            let content_w = (screen_w * 0.3333).clamp(580.0, 680.0);
            let content_h = (screen_h * 0.70).clamp(520.0, 780.0);
            let spacing = (screen_w / 50.0).clamp(20.0, 38.0);
            let side_w = (screen_w / 10.0).clamp(160.0, 192.0);

            let content_x = ((screen_w - content_w) * 0.5).floor();
            let content_y = (48.0 + (screen_h - 48.0 - content_h) * 0.5).floor().max(52.0);

            let left_x = content_x - spacing - side_w;
            let right_x = content_x + content_w + spacing;

            // Assert elements do not overlap horizontally
            assert!(left_x + side_w <= content_x - spacing);
            assert!(content_x + content_w + spacing <= right_x);

            // Assert top of left card and right buttons align with content box top
            assert_eq!(content_y, content_y);

            // Left card is a perfect square
            let left_rect = egui::Rect::from_min_size(egui::pos2(left_x, content_y), egui::vec2(side_w, side_w));
            assert_eq!(left_rect.width(), left_rect.height());

            // Check bounding box fits inside the screen width
            assert!(left_x >= 0.0);
            assert!(right_x + side_w <= screen_w);
        }
    }

    #[test]
    fn test_estimated_replay_file_size_formula() {
        // Formula from GPU Screen Recorder: ((replay_time_seconds * video_bitrate_kbps) / 8192.0) MB
        let calc_mb = |sec: u32, mbps: u32| -> f64 {
            ((sec as f64) * (mbps as f64 * 1000.0)) / 8192.0
        };

        // 60s at 20Mbps = 146.48 MB
        let mb_60s_20m = calc_mb(60, 20);
        assert!((mb_60s_20m - 146.484).abs() < 0.01);

        // 300s (5m) at 50Mbps = 1831.05 MB
        let mb_300s_50m = calc_mb(300, 50);
        assert!((mb_300s_50m - 1831.054).abs() < 0.01);

        // 30s at 10Mbps = 36.62 MB
        let mb_30s_10m = calc_mb(30, 10);
        assert!((mb_30s_10m - 36.621).abs() < 0.01);
    }

    #[test]
    fn test_palette_accent_colors() {
        assert_eq!(resolve_accent_color("amd"), Color32::from_rgb(221, 0, 49));
        assert_eq!(resolve_accent_color("nvidia"), Color32::from_rgb(118, 185, 0));
        assert_eq!(resolve_accent_color("intel"), Color32::from_rgb(8, 109, 183));
        assert_eq!(resolve_accent_color("green"), Color32::from_rgb(118, 185, 0));
        assert_eq!(resolve_accent_color("yellow"), Color32::from_rgb(250, 204, 21));
        assert_eq!(resolve_accent_color("purple"), Color32::from_rgb(168, 85, 247));
        assert_eq!(resolve_accent_color("pink"), Color32::from_rgb(244, 63, 94));
        // Hex code parsing
        assert_eq!(resolve_accent_color("#76b900"), Color32::from_rgb(118, 185, 0));
        // Default fallback to AMD red
        assert_eq!(resolve_accent_color("unknown"), Color32::from_rgb(221, 0, 49));
    }
}
