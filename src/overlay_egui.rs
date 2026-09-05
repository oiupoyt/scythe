use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Margin, Stroke, Vec2};
use std::path::PathBuf;
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
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output();
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
    
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
    let out_name = format!("{}_trim_{}.{}", stem, now, ext);
    let out_path = parent.join(&out_name);

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
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", &p.to_string_lossy()])
                .creation_flags(0x08000000)
                .spawn();
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

    let bg_color = Color32::from_rgb(18, 24, 34);
    let border_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 30));
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
            FontId::monospace(8.0),
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
                        let is_replay = filename.starts_with("replay_");
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
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("explorer")
                .arg(&p)
                .creation_flags(0x08000000)
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&p).spawn();
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&p).spawn();
        }
    });
}

// Cross-platform folder picker dialog
fn pick_folder(current_dir: &str, tx: Sender<String>) {
    let cur = current_dir.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let script = format!(
                "[System.Reflection.Assembly]::LoadWithPartialName(\x27System.windows.forms\x27) | Out-Null; \
                 $f = New-Object System.Windows.Forms.FolderBrowserDialog; \
                 $f.SelectedPath = \x27{}\x27; \
                 if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Host $f.SelectedPath }}",
                cur.replace("\x27", "\x27\x27")
            );
            if let Ok(out) = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .creation_flags(0x08000000)
                .output()
            {
                let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sel.is_empty() {
                    let _ = tx.send(sel);
                    return;
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(out) = std::process::Command::new("kdialog")
                .args(["--getexistingdirectory", &cur])
                .output()
            {
                let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sel.is_empty() {
                    let _ = tx.send(sel);
                    return;
                }
            }
            if let Ok(out) = std::process::Command::new("zenity")
                .args(["--file-selection", "--directory"])
                .output()
            {
                let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !sel.is_empty() {
                    let _ = tx.send(sel);
                }
            }
        }
    });
}

// Helper to render mechanical keyboard keycap badges
#[allow(dead_code)]
fn render_keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(18, 22, 30))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 60, 78)))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(6_i8, 2_i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(FontId::monospace(9.5))
                    .strong()
                    .color(Color32::from_rgb(203, 213, 225)),
            );
        });
}

pub fn resolve_accent_color(accent: &str) -> Color32 {
    match accent.to_lowercase().as_str() {
        "green" | "emerald" => Color32::from_rgb(34, 197, 94),
        "cyan" | "ice" => Color32::from_rgb(6, 182, 212),
        "purple" | "violet" => Color32::from_rgb(168, 85, 247),
        "amber" | "orange" => Color32::from_rgb(245, 158, 11),
        "red" | "crimson" => Color32::from_rgb(239, 68, 68),
        "blue" | "sapphire" | _ => Color32::from_rgb(56, 189, 248), // Charming and comforting sky sapphire blue
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
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 45),
            Stroke::new(1.5_f32, accent),
            accent,
            "PRESS KEYS...".to_string(),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(20, 26, 38, 220),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 35)),
            Color32::from_rgb(226, 232, 240),
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

// Minimal squared button selector
fn squared_button(ui: &mut egui::Ui, text: &str, active: bool, accent: Color32) -> bool {
    let fill = if active {
        accent
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 12)
    };
    let stroke = if active {
        Stroke::new(1.0_f32, accent)
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25))
    };
    let text_color = if active {
        Color32::from_rgb(10, 14, 22)
    } else {
        Color32::from_rgb(203, 213, 225)
    };
    let btn = egui::Button::new(egui::RichText::new(text).size(11.0).strong().color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::ZERO);
    ui.add(btn).clicked()
}

#[allow(dead_code)]
fn pill_button(ui: &mut egui::Ui, text: &str, active: bool, accent: Color32) -> bool {
    squared_button(ui, text, active, accent)
}

// Sleek Squared Switch Toggle
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
            Color32::from_rgb(30, 41, 59)
        };
        let stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 40));
        ui.painter().rect(rect, CornerRadius::ZERO, bg_color, stroke, egui::StrokeKind::Inside);
        let knob_w = rect.height() - 4.0;
        let knob_x = egui::lerp((rect.left() + 2.0)..=(rect.right() - knob_w - 2.0), how_on);
        let knob_rect = egui::Rect::from_min_size(egui::pos2(knob_x, rect.top() + 2.0), egui::vec2(knob_w, knob_w));
        ui.painter().rect_filled(knob_rect, CornerRadius::ZERO, Color32::WHITE);
    }
    response
}

// Geometric Centered Vector Icon Renderers (Upgraded High-Tech & Smooth)
fn draw_replay_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_active: bool, accent: Color32) {
    use std::f32::consts::PI;
    let color = if is_active {
        accent
    } else {
        Color32::from_rgb(148, 163, 184)
    };
    let subtle_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 65);

    // 1. Concentric guide track / telemetry dots
    let inner_r = radius * 0.72;
    for i in 0..12 {
        let angle = (i as f32) * (2.0 * PI / 12.0);
        let pip_center = center + Vec2::new(angle.cos() * inner_r, angle.sin() * inner_r);
        painter.rect_filled(
            egui::Rect::from_center_size(pip_center, Vec2::splat(1.5)),
            CornerRadius::ZERO,
            subtle_color,
        );
    }

    // 2. High-precision outer curved sweep arc (270 degrees)
    let start_angle = 0.22 * PI;
    let end_angle = 1.78 * PI;
    let steps = 48;
    let mut points = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = start_angle + t * (end_angle - start_angle);
        points.push(center + Vec2::new(angle.cos() * radius, angle.sin() * radius));
    }
    let stroke = Stroke::new(2.8_f32, color);
    for w in points.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }

    // 3. Sharp aerodynamic directional chevron arrow tip at start of arc
    let tip = points[0];
    let tangent = Vec2::new(-start_angle.sin(), start_angle.cos()).normalized();
    let normal = Vec2::new(start_angle.cos(), start_angle.sin()).normalized();
    let p_back_1 = tip - tangent * 9.0 + normal * 5.5;
    let p_back_2 = tip - tangent * 9.0 - normal * 5.5;
    let p_inner = tip - tangent * 6.5;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, p_back_1, p_inner, p_back_2],
        color,
        Stroke::NONE,
    ));

    // 4. Subtle outer cardinal ticks
    for &angle in &[0.0 * PI, 0.5 * PI, 1.0 * PI, 1.5 * PI] {
        if angle < start_angle || angle > end_angle {
            continue;
        }
        let p_in = center + Vec2::new(angle.cos() * (radius - 2.5), angle.sin() * (radius - 2.5));
        let p_out = center + Vec2::new(angle.cos() * (radius + 3.0), angle.sin() * (radius + 3.0));
        painter.line_segment([p_in, p_out], Stroke::new(1.2_f32, subtle_color));
    }

    // 5. Centered high-tech instant replay glyph: Vertical step bar + Play triangle (|◁)
    let glyph_center = center + Vec2::new(1.0, 0.0);
    let bar_x = glyph_center.x - radius * 0.38;
    let bar_half_h = radius * 0.34;
    painter.line_segment(
        [egui::pos2(bar_x, glyph_center.y - bar_half_h), egui::pos2(bar_x, glyph_center.y + bar_half_h)],
        Stroke::new(2.2_f32, color),
    );

    let tri_w = radius * 0.44;
    let tri_h = radius * 0.35;
    let t_left = egui::pos2(bar_x + 2.5, glyph_center.y);
    let t_top = egui::pos2(bar_x + 2.5 + tri_w, glyph_center.y - tri_h);
    let t_bot = egui::pos2(bar_x + 2.5 + tri_w, glyph_center.y + tri_h);
    painter.add(egui::Shape::convex_polygon(
        vec![t_left, t_top, t_bot],
        color,
        Stroke::NONE,
    ));
}

fn draw_record_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_recording: bool, anim_time: f32) {
    let frame_half = radius * 0.95;
    let bracket_len = radius * 0.36;

    if is_recording {
        let pulse = (anim_time * 5.5).sin() * 0.5 + 0.5;
        let red_bright = Color32::from_rgb(239, 68, 68);
        let red_glow = Color32::from_rgba_unmultiplied(239, 68, 68, (40.0 + pulse * 60.0) as u8);

        // Ambient pulsing glow
        let glow_size = (radius + pulse * 3.5) * 2.2;
        painter.rect_filled(
            egui::Rect::from_center_size(center, Vec2::splat(glow_size)),
            CornerRadius::ZERO,
            red_glow,
        );

        // 4 Viewfinder Corner Brackets
        let stroke_bracket = Stroke::new(1.8_f32, red_bright);
        let min = center - Vec2::splat(frame_half);
        let max = center + Vec2::splat(frame_half);

        // Top-left bracket
        painter.line_segment([min, egui::pos2(min.x + bracket_len, min.y)], stroke_bracket);
        painter.line_segment([min, egui::pos2(min.x, min.y + bracket_len)], stroke_bracket);
        // Top-right bracket
        painter.line_segment([egui::pos2(max.x, min.y), egui::pos2(max.x - bracket_len, min.y)], stroke_bracket);
        painter.line_segment([egui::pos2(max.x, min.y), egui::pos2(max.x, min.y + bracket_len)], stroke_bracket);
        // Bottom-left bracket
        painter.line_segment([egui::pos2(min.x, max.y), egui::pos2(min.x + bracket_len, max.y)], stroke_bracket);
        painter.line_segment([egui::pos2(min.x, max.y), egui::pos2(min.x, max.y - bracket_len)], stroke_bracket);
        // Bottom-right bracket
        painter.line_segment([max, egui::pos2(max.x - bracket_len, max.y)], stroke_bracket);
        painter.line_segment([max, egui::pos2(max.x, max.y - bracket_len)], stroke_bracket);

        // Center pulsing recording core
        let core_size = (radius * 0.70) + (pulse * 2.0);
        painter.rect_filled(
            egui::Rect::from_center_size(center, Vec2::splat(core_size)),
            CornerRadius::ZERO,
            red_bright,
        );

        // Mini REC status badge above center
        painter.text(
            egui::pos2(center.x, min.y - 4.0),
            egui::Align2::CENTER_BOTTOM,
            "REC",
            FontId::monospace(8.0),
            red_bright,
        );
    } else {
        let frame_color = Color32::from_rgb(148, 163, 184);
        let stroke_bracket = Stroke::new(1.6_f32, frame_color);
        let min = center - Vec2::splat(frame_half);
        let max = center + Vec2::splat(frame_half);

        // 4 Viewfinder Corner Brackets
        painter.line_segment([min, egui::pos2(min.x + bracket_len, min.y)], stroke_bracket);
        painter.line_segment([min, egui::pos2(min.x, min.y + bracket_len)], stroke_bracket);
        painter.line_segment([egui::pos2(max.x, min.y), egui::pos2(max.x - bracket_len, min.y)], stroke_bracket);
        painter.line_segment([egui::pos2(max.x, min.y), egui::pos2(max.x, min.y + bracket_len)], stroke_bracket);
        painter.line_segment([egui::pos2(min.x, max.y), egui::pos2(min.x + bracket_len, max.y)], stroke_bracket);
        painter.line_segment([egui::pos2(min.x, max.y), egui::pos2(min.x, max.y - bracket_len)], stroke_bracket);
        painter.line_segment([max, egui::pos2(max.x - bracket_len, max.y)], stroke_bracket);
        painter.line_segment([max, egui::pos2(max.x, max.y - bracket_len)], stroke_bracket);

        // Subtle crosshair tick marks pointing inward
        let tick_len = radius * 0.22;
        let tick_stroke = Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(148, 163, 184, 140));
        painter.line_segment([egui::pos2(center.x, min.y), egui::pos2(center.x, min.y + tick_len)], tick_stroke);
        painter.line_segment([egui::pos2(center.x, max.y), egui::pos2(center.x, max.y - tick_len)], tick_stroke);
        painter.line_segment([egui::pos2(min.x, center.y), egui::pos2(min.x + tick_len, center.y)], tick_stroke);
        painter.line_segment([egui::pos2(max.x, center.y), egui::pos2(max.x - tick_len, center.y)], tick_stroke);

        // Center standby optic reticle
        painter.rect_stroke(
            egui::Rect::from_center_size(center, Vec2::splat(radius * 0.85)),
            CornerRadius::ZERO,
            Stroke::new(1.4_f32, Color32::from_rgba_unmultiplied(226, 232, 240, 180)),
            egui::StrokeKind::Inside,
        );
        painter.rect_filled(
            egui::Rect::from_center_size(center, Vec2::splat(radius * 0.38)),
            CornerRadius::ZERO,
            Color32::from_rgb(226, 232, 240),
        );
    }
}

fn draw_gear_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    use std::f32::consts::PI;
    let stroke = Stroke::new(1.8_f32, color);

    // 8 precision chamfered mechanical teeth
    let num_teeth = 8;
    let outer_r = radius * 1.05;
    let root_r = radius * 0.78;
    let tooth_half_angle = (2.0 * PI / num_teeth as f32) * 0.28;
    let tooth_tip_half_angle = tooth_half_angle * 0.65;

    for i in 0..num_teeth {
        let angle = (i as f32) * (2.0 * PI / num_teeth as f32);
        let a_tip1 = angle - tooth_tip_half_angle;
        let a_tip2 = angle + tooth_tip_half_angle;
        let a_root1 = angle - tooth_half_angle;
        let a_root2 = angle + tooth_half_angle;

        let p_root1 = center + Vec2::new(a_root1.cos() * root_r, a_root1.sin() * root_r);
        let p_tip1 = center + Vec2::new(a_tip1.cos() * outer_r, a_tip1.sin() * outer_r);
        let p_tip2 = center + Vec2::new(a_tip2.cos() * outer_r, a_tip2.sin() * outer_r);
        let p_root2 = center + Vec2::new(a_root2.cos() * root_r, a_root2.sin() * root_r);

        painter.add(egui::Shape::convex_polygon(
            vec![p_root1, p_tip1, p_tip2, p_root2],
            color,
            Stroke::NONE,
        ));
    }

    // Outer gear body ring
    let steps = 32;
    let mut ring_pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = t * 2.0 * PI;
        ring_pts.push(center + Vec2::new(a.cos() * root_r, a.sin() * root_r));
    }
    for w in ring_pts.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }

    // Concentric recessed aperture ring
    let aperture_r = radius * 0.54;
    let mut ap_pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = t * 2.0 * PI;
        ap_pts.push(center + Vec2::new(a.cos() * aperture_r, a.sin() * aperture_r));
    }
    let ap_stroke = Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 130));
    for w in ap_pts.windows(2) {
        painter.line_segment([w[0], w[1]], ap_stroke);
    }

    // 4 radial hub spokes
    let spoke_stroke = Stroke::new(1.6_f32, color);
    let spoke_in = radius * 0.28;
    let spoke_out = radius * 0.74;
    for i in 0..4 {
        let a = (i as f32) * (PI / 2.0);
        let p1 = center + Vec2::new(a.cos() * spoke_in, a.sin() * spoke_in);
        let p2 = center + Vec2::new(a.cos() * spoke_out, a.sin() * spoke_out);
        painter.line_segment([p1, p2], spoke_stroke);
    }

    // Central hub with precision bore hole
    let hub_r = radius * 0.30;
    painter.rect_filled(
        egui::Rect::from_center_size(center, Vec2::splat(hub_r * 2.0)),
        CornerRadius::ZERO,
        color,
    );
    let bore_r = radius * 0.14;
    painter.rect_filled(
        egui::Rect::from_center_size(center, Vec2::splat(bore_r * 2.0)),
        CornerRadius::ZERO,
        Color32::from_rgb(12, 16, 24),
    );
}

// Minimal Squared Action Card Renderer
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
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let hovered = response.hovered();

    let bg = if dropdown_open {
        Color32::from_rgba_unmultiplied(20, 28, 42, 245)
    } else if hovered {
        Color32::from_rgba_unmultiplied(18, 25, 38, 235)
    } else {
        Color32::from_rgba_unmultiplied(12, 16, 24, 225)
    };

    let border = if dropdown_open || hovered {
        accent
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 30)
    };

    let painter = ui.painter();
    // Drop shadow
    painter.rect_filled(
        rect.translate(Vec2::new(0.0, 6.0)),
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(0, 0, 0, 90),
    );
    // Card background - SQUARED
    painter.rect(rect, CornerRadius::ZERO, bg, Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);

    // Card Title
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 24.0),
        egui::Align2::CENTER_CENTER,
        title,
        FontId::proportional(12.0),
        if is_active { accent } else { Color32::WHITE },
    );

    // Centered Vector Icon
    let icon_center = egui::pos2(rect.center().x, rect.top() + 85.0);
    draw_icon(painter, icon_center);

    // Status label
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 138.0),
        egui::Align2::CENTER_CENTER,
        status_text,
        FontId::proportional(11.5),
        status_color,
    );

    // Subtitle label
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 158.0),
        egui::Align2::CENTER_CENTER,
        sub_text,
        FontId::proportional(10.0),
        Color32::from_rgb(100, 116, 139),
    );

    response.clicked()
}

// Attached Squared Dropdown Menu Container
fn render_dropdown_menu(
    ui: &mut egui::Ui,
    width: f32,
    accent: Color32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(12, 16, 24, 250))
        .stroke(Stroke::new(1.0_f32, accent))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(6_i8))
        .show(ui, |ui| {
            ui.set_width(width);
            add_contents(ui);
        });
}

// Sleek Squared Dropdown Action Menu Item
fn render_menu_item(ui: &mut egui::Ui, label: &str, accent: Color32) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::click());
    let hovered = response.hovered();

    let bg = if hovered {
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 35)
    } else {
        Color32::TRANSPARENT
    };
    let border = if hovered {
        Stroke::new(1.0_f32, accent)
    } else {
        Stroke::NONE
    };
    let text_color = if hovered {
        accent
    } else {
        Color32::from_rgb(226, 232, 240)
    };

    ui.painter().rect(rect, CornerRadius::ZERO, bg, border, egui::StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        text_color,
    );

    response.clicked()
}

// Section card helper for Settings view
fn render_section_card(ui: &mut egui::Ui, header: &str, accent: Color32, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(16, 22, 32, 200))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 18)))
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
    status_msg: Option<(String, Instant)>,
    status_rx: Receiver<DaemonStatus>,
    folder_tx: Sender<String>,
    folder_rx: Receiver<String>,
    clips: Vec<VideoClipInfo>,
    initial_pos_set: bool,
    listening_keybind: Option<KeybindAction>,
    fps_input_str: String,
    panel_rect: egui::Rect,
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
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        let (folder_tx, folder_rx) = channel::<String>();
        let clips = scan_recordings(&output_dir);
        let fps_input_str = target_fps.to_string();

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
            status_msg: None,
            status_rx,
            folder_tx,
            folder_rx,
            clips,
            initial_pos_set: false,
            listening_keybind: None,
            fps_input_str,
            panel_rect: egui::Rect::NOTHING,
        }
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
                let _ = ipc::send_command(Command::ReloadConfig);
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
                if self.replay_dropdown_open || self.record_dropdown_open { card_h + 95.0 } else { card_h + 40.0 },
            ),
        );
        self.panel_rect = hud_rect;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(hud_rect), |ui| {
            ui.vertical_centered(|ui| {
                // Sleek status toast if present
                if let Some((msg, ts)) = &self.status_msg {
                    if ts.elapsed() < Duration::from_secs(3) {
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(12, 16, 24, 240))
                            .stroke(Stroke::new(1.0_f32, accent))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::symmetric(14_i8, 5_i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(msg)
                                        .font(FontId::monospace(11.0))
                                        .strong()
                                        .color(accent),
                                );
                            });
                        ui.add_space(8.0);
                    }
                }

                ui.horizontal(|ui| {
                    // =========================================================================
                    // CARD 1: INSTANT REPLAY
                    // =========================================================================
                    ui.vertical(|ui| {
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
                            if is_replay_active { accent } else { Color32::from_rgb(148, 163, 184) },
                            "(Click for menu)",
                            accent,
                        );

                        if card1_clicked {
                            self.replay_dropdown_open = !self.replay_dropdown_open;
                            self.record_dropdown_open = false;
                        }

                        if self.replay_dropdown_open {
                            render_dropdown_menu(ui, card_w, accent, |ui| {
                                let toggle_text = if is_replay_active { "Turn off" } else { "Turn on" };
                                if render_menu_item(ui, toggle_text, accent) {
                                    let mut cfg = ScytheConfig::load();
                                    cfg.replay_enabled = !cfg.replay_enabled;
                                    let _ = cfg.save();
                                    ScytheConfig::notify_daemon_reload();
                                    self.config.replay_enabled = cfg.replay_enabled;
                                    self.status.is_replay_active = cfg.replay_enabled;
                                    self.replay_dropdown_open = false;
                                }
                                if render_menu_item(ui, "Save Replay", accent) {
                                    let _ = ipc::send_command(Command::SaveReplay);
                                    self.status_msg = Some(("Replay Saved!".to_string(), Instant::now()));
                                    self.replay_dropdown_open = false;
                                }
                            });
                        }
                    });

                    ui.add_space(card_gap);

                    // =========================================================================
                    // CARD 2: RECORD
                    // =========================================================================
                    ui.vertical(|ui| {
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
                            if is_recording { Color32::from_rgb(239, 68, 68) } else { Color32::from_rgb(148, 163, 184) },
                            "(Click for menu)",
                            accent,
                        );

                        if card2_clicked {
                            self.record_dropdown_open = !self.record_dropdown_open;
                            self.replay_dropdown_open = false;
                        }

                        if self.record_dropdown_open {
                            render_dropdown_menu(ui, card_w, accent, |ui| {
                                let rec_toggle_text = if is_recording { "Stop Recording" } else { "Start Recording" };
                                if render_menu_item(ui, rec_toggle_text, accent) {
                                    let _ = ipc::send_command(Command::ToggleRecording);
                                    self.record_dropdown_open = false;
                                }
                            });
                        }
                    });

                    ui.add_space(card_gap);

                    // =========================================================================
                    // CARD 3: SETTINGS
                    // =========================================================================
                    ui.vertical(|ui| {
                        let card3_clicked = render_action_card(
                            ui,
                            card_w,
                            card_h,
                            "SETTINGS",
                            false,
                            false,
                            |painter, center| {
                                draw_gear_icon(painter, center, 24.0, Color32::from_rgb(148, 163, 184));
                            },
                            "Hardware & Tuning",
                            Color32::from_rgb(148, 163, 184),
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

                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new("ESC or click outside to close")
                        .font(FontId::monospace(10.0))
                        .color(Color32::from_rgba_unmultiplied(203, 213, 225, 120)),
                );
            });
        });
    }
    fn render_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let screen_w = ui.available_width();
        let screen_h = ui.available_height();
        let modal_w = 520.0_f32;
        let modal_h = (screen_h - 60.0).clamp(580.0, 800.0);
        let left_pad = ((screen_w - modal_w) / 2.0).max(10.0);
        let top_pad = ((screen_h - modal_h) / 2.0).max(20.0);
        let accent = self.accent_color();

        let modal_rect = egui::Rect::from_min_size(egui::pos2(left_pad, top_pad), egui::vec2(modal_w, modal_h));
        self.panel_rect = modal_rect;

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(modal_rect), |ui| {
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(12, 16, 24, 250))
                .stroke(Stroke::new(1.0_f32, accent))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::symmetric(22_i8, 18_i8))
                .show(ui, |ui| {
                    ui.set_width(modal_w - 44.0);

                    // Portrait Header: Back Button, Title, Close Hint (No X button)
                    ui.horizontal(|ui| {
                        let back_btn = egui::Button::new(
                            egui::RichText::new("< BACK")
                                .font(FontId::monospace(11.5))
                                .strong()
                                .color(accent),
                        )
                        .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30))
                        .stroke(Stroke::new(1.0_f32, accent))
                        .corner_radius(CornerRadius::ZERO);

                        if ui.add(back_btn).clicked() {
                            self.switch_view(ShadowPlayView::MainHud, ctx);
                            return;
                        }

                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("SETTINGS")
                                .font(FontId::proportional(15.0))
                                .strong()
                                .color(Color32::WHITE),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::NONE
                                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 12))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25)))
                                .corner_radius(CornerRadius::ZERO)
                                .inner_margin(Margin::symmetric(8_i8, 4_i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("ESC TO CLOSE")
                                            .font(FontId::monospace(9.5))
                                            .strong()
                                            .color(Color32::from_rgb(148, 163, 184)),
                                    );
                                });
                        });
                    });

                    ui.add_space(12.0);

                    let scroll_h = modal_h - 110.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // -----------------------------------------------------------------
                            // SECTION: ACCENT COLOR THEME
                            // -----------------------------------------------------------------
                            render_section_card(ui, "ACCENT COLOR THEME", accent, |ui| {
                                ui.label(egui::RichText::new("Select Interface Accent Color:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(6.0);
                                ui.horizontal_wrapped(|ui| {
                                    let palettes = [
                                        ("blue", "Charming Blue", Color32::from_rgb(56, 189, 248)),
                                        ("cyan", "Cyber Cyan", Color32::from_rgb(6, 182, 212)),
                                        ("green", "Emerald Green", Color32::from_rgb(34, 197, 94)),
                                        ("purple", "Royal Purple", Color32::from_rgb(168, 85, 247)),
                                        ("amber", "Sunset Amber", Color32::from_rgb(245, 158, 11)),
                                        ("red", "Crimson Red", Color32::from_rgb(239, 68, 68)),
                                    ];

                                    for (id, name, col) in palettes {
                                        let is_sel = self.config.accent_color.to_lowercase() == id;
                                        let bg = if is_sel {
                                            Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 45)
                                        } else {
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 12)
                                        };
                                        let stroke = if is_sel {
                                            Stroke::new(1.5_f32, col)
                                        } else {
                                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25))
                                        };

                                        let btn = egui::Button::new(
                                            egui::RichText::new(format!("[#]  {}", name))
                                                .size(11.5)
                                                .strong()
                                                .color(if is_sel { col } else { Color32::from_rgb(226, 232, 240) }),
                                        )
                                        .fill(bg)
                                        .stroke(stroke)
                                        .corner_radius(CornerRadius::ZERO)
                                        .min_size(Vec2::new(138.0, 28.0));

                                        if ui.add(btn).clicked() {
                                            self.config.accent_color = id.to_string();
                                            let _ = self.config.save();
                                            self.status_msg = Some((format!("Accent: {}", name), Instant::now()));
                                        }
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 1: DISPLAY & CAPTURE
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
                                            let _ = ipc::send_command(Command::ToggleCursor);
                                            self.status_msg = Some((format!("Cursor {}", if cur { "Visible" } else { "Hidden" }), Instant::now()));
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
                                    ui.label(egui::RichText::new("Custom:").size(11.0).color(Color32::from_rgb(148, 163, 184)));
                                    let edit_resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.fps_input_str)
                                            .desired_width(50.0)
                                            .font(FontId::monospace(11.5))
                                    );
                                    if edit_resp.changed() {
                                        if let Ok(parsed) = self.fps_input_str.trim().parse::<u32>() {
                                            if (15..=360).contains(&parsed) {
                                                self.target_fps = parsed;
                                            }
                                        }
                                    }

                                    ui.add_space(6.0);
                                    for fps in [30, 60, 120, 144, 240] {
                                        if squared_button(ui, &fps.to_string(), self.target_fps == fps, accent) {
                                            self.target_fps = fps;
                                            self.fps_input_str = fps.to_string();
                                        }
                                    }
                                });

                                ui.add_space(8.0);

                                // Video Bitrate
                                ui.label(egui::RichText::new("Video Bitrate:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for mbps in [10, 20, 30, 50] {
                                        if squared_button(ui, &format!("{}M", mbps), self.bitrate_mbps == mbps, accent) {
                                            self.bitrate_mbps = mbps;
                                        }
                                    }
                                    ui.add_space(6.0);
                                    let mut br = self.bitrate_mbps;
                                    if ui.add(egui::Slider::new(&mut br, 5..=100).suffix(" Mbps").step_by(5.0)).changed() {
                                        self.bitrate_mbps = br;
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

                                ui.add_space(8.0);

                                // Replay Duration
                                ui.label(egui::RichText::new("Instant Replay Buffer Duration:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    for sec in [15, 30, 60, 120, 300] {
                                        let label = if sec >= 60 { format!("{}m", sec / 60) } else { format!("{}s", sec) };
                                        if squared_button(ui, &label, self.replay_sec == sec, accent) {
                                            self.replay_sec = sec;
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
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 4: STORAGE & DESTINATION
                            // -----------------------------------------------------------------
                            render_section_card(ui, "STORAGE & DESTINATION", accent, |ui| {
                                ui.label(egui::RichText::new("Save Folder:").size(12.0).strong().color(Color32::WHITE));
                                ui.add_space(3.0);
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(260.0));
                                    if squared_button(ui, "Change", false, accent) {
                                        pick_folder(&self.output_dir, self.folder_tx.clone());
                                    }
                                    if squared_button(ui, "Open", false, accent) {
                                        open_folder(&ScytheConfig::expand_tilde(&self.output_dir));
                                    }
                                });

                                ui.add_space(8.0);
                                if squared_button(ui, &format!("OPEN RECORDINGS GALLERY & TRIMMER ({})", self.clips.len()), false, accent) {
                                    self.switch_view(ShadowPlayView::Gallery, ctx);
                                }
                            });

                            ui.add_space(14.0);

                            // APPLY & SAVE BUTTON
                            let apply_btn = egui::Button::new(
                                egui::RichText::new("APPLY & SAVE SETTINGS")
                                    .font(FontId::monospace(13.0))
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
                                let _ = self.config.save();
                                crate::hyprland_binds::register_hyprland_binds(&self.config);
                                crate::config::ScytheConfig::notify_daemon_reload();
                                self.status_msg = Some(("Settings Saved & Applied!".to_string(), Instant::now()));
                                self.switch_view(ShadowPlayView::MainHud, ctx);
                            }
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
                .fill(Color32::from_rgba_unmultiplied(12, 16, 24, 250))
                .stroke(Stroke::new(1.0_f32, accent))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::symmetric(20_i8, 16_i8))
                .show(ui, |ui| {
                    ui.set_width(modal_w - 40.0);

                    // Header with Back Button and Refresh
                    ui.horizontal(|ui| {
                        let back_btn = egui::Button::new(
                            egui::RichText::new("< BACK TO SETTINGS")
                                .font(FontId::monospace(11.0))
                                .strong()
                                .color(accent),
                        )
                        .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30))
                        .stroke(Stroke::new(1.0_f32, accent))
                        .corner_radius(CornerRadius::ZERO);

                        if ui.add(back_btn).clicked() {
                            self.switch_view(ShadowPlayView::Settings, ctx);
                            return;
                        }

                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("GALLERY & CLIP TRIMMER")
                                .font(FontId::proportional(14.0))
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

                    if let Some((msg, ts)) = &self.trim_status_msg {
                        if ts.elapsed() < Duration::from_secs(4) {
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
                    }

                    // Main two-column split
                    ui.horizontal(|ui| {
                        // Left Column: Clips List
                        ui.vertical(|ui| {
                            ui.set_width(280.0);
                            ui.label(egui::RichText::new(format!("RECORDED CLIPS ({})", self.clips.len())).size(11.0).strong().color(Color32::from_rgb(148, 163, 184)));
                            ui.add_space(4.0);

                            egui::ScrollArea::vertical()
                                .max_height(modal_h - 120.0)
                                .id_salt("gallery_clips_scroll")
                                .show(ui, |ui| {
                                    if self.clips.is_empty() {
                                        ui.label(egui::RichText::new("No recordings found yet.\nPress hotkeys to capture clips.").size(11.0).color(Color32::from_rgb(100, 116, 139)));
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
                                                        ui.label(egui::RichText::new(format!("{:.1} MB", size_mb)).size(10.0).color(Color32::from_rgb(148, 163, 184)));
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
                                            ui.label(egui::RichText::new(format!("{:.2} MB ({} bytes)", size_mb, clip.size_bytes)).size(10.5).color(Color32::from_rgb(148, 163, 184)));
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
                                        ui.label(egui::RichText::new(format!("Trimmed output length: {:.1}s (Instant lossless copy)", trimmed_dur)).size(10.5).color(Color32::from_rgb(148, 163, 184)));

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
                                        ui.label(egui::RichText::new("No clip selected").font(FontId::proportional(13.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                        ui.add_space(6.0);
                                        ui.label(egui::RichText::new("Select a recording or instant replay from the list on the left to inspect, play, or losslessly trim.").size(10.5).color(Color32::from_rgb(100, 116, 139)));
                                    });
                                    ui.add_space(40.0);
                                });
                            }
                        });
                    });
                });
        });
    }
}

impl eframe::App for ScytheOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async_events();
        self.mic_vu = (self.mic_vu * 0.94).max(0.0);
        self.sys_vu = (self.sys_vu * 0.94).max(0.0);
        self.anim_time += 0.033;
        ctx.request_repaint_after(Duration::from_millis(50));

        // Auto-position and size to monitor on launch
        if !self.initial_pos_set {
            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                if monitor_size.x > 100.0 && monitor_size.y > 100.0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(monitor_size));
                    self.initial_pos_set = true;
                }
            }
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
                    let action_name = match action {
                        KeybindAction::Menu => {
                            self.config.menu_hotkey = combo.clone();
                            "Menu Overlay"
                        }
                        KeybindAction::SaveReplay => {
                            self.config.save_hotkey = combo.clone();
                            "Instant Replay"
                        }
                        KeybindAction::ToggleRecord => {
                            self.config.record_hotkey = combo.clone();
                            "Record Toggle"
                        }
                        KeybindAction::ToggleCursor => {
                            self.config.cursor_hotkey = combo.clone();
                            "Cursor Toggle"
                        }
                    };
                    let _ = self.config.save();
                    crate::hyprland_binds::register_hyprland_binds(&self.config);
                    crate::config::ScytheConfig::notify_daemon_reload();
                    self.status_msg = Some((format!("Bound {}: {}", action_name, combo), Instant::now()));
                    self.listening_keybind = None;
                }
            }
        } else {
            // Normal Escape handling
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
            if ctx.input(|i| i.pointer.primary_clicked()) {
                if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                    if self.panel_rect.width() > 10.0 && !self.panel_rect.expand(6.0).contains(pos) {
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
            }
        }

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::TRANSPARENT;
        ctx.set_visuals(visuals);

        // Background screen darkening scrim (rgba 0, 0, 0, 115 gives ~45% dimming of background)
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgba_unmultiplied(0, 0, 0, 115)))
            .show(ctx, |ui| match self.current_view {
                ShadowPlayView::MainHud => self.render_main_hud(ctx, ui),
                ShadowPlayView::Settings => self.render_settings_view(ctx, ui),
                ShadowPlayView::Gallery => self.render_gallery_view(ctx, ui),
            });
    }
}

pub fn run_egui_overlay() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scythe")
            .with_app_id("scythe-overlay")
            .with_position([0.0, 0.0])
            .with_inner_size([1920.0, 1080.0])
            .with_maximized(true)
            .with_resizable(true)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top(),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "scythe-overlay",
        options,
        Box::new(|_cc| Ok(Box::new(ScytheOverlayApp::new()))),
    );

    crate::ipc::clean_overlay_pid();
}
