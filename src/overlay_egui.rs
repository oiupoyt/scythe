use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Margin, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};
use crate::config::VrecConfig;
use crate::ipc::{self, Command, DaemonStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowPlayView {
    MainHud,
    Settings,
    Gallery,
    AudioMixer,
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
    let dir = VrecConfig::expand_tilde(dir_str);
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

fn format_file_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_system_time(time: SystemTime) -> String {
    if let Ok(dur) = SystemTime::now().duration_since(time) {
        let secs = dur.as_secs();
        if secs < 60 {
            "Just now".to_string()
        } else if secs < 3600 {
            format!("{} min ago", secs / 60)
        } else if secs < 86400 {
            format!("{} hr ago", secs / 3600)
        } else {
            format!("{} days ago", secs / 86400)
        }
    } else {
        "Recent".to_string()
    }
}

// Cross-platform helper to reveal or open directories
fn open_folder(path: &std::path::Path) {
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer").arg(&p).spawn();
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

// Cross-platform helper to launch media files with default player
fn open_file(path: &std::path::Path) {
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", &p.to_string_lossy()])
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
            let script = format!(
                "[System.Reflection.Assembly]::LoadWithPartialName('System.windows.forms') | Out-Null; \
                 $f = New-Object System.Windows.Forms.FolderBrowserDialog; \
                 $f.SelectedPath = '{}'; \
                 if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Host $f.SelectedPath }}",
                cur.replace('\'', "''")
            );
            if let Ok(out) = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &script])
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
fn render_keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(18, 22, 30))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 60, 78)))
        .corner_radius(CornerRadius::same(4_u8))
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

// Helper pill button
fn pill(ui: &mut egui::Ui, text: &str, active: bool) -> bool {
    let fill = if active { Color32::from_rgb(118, 185, 0) } else { Color32::from_rgb(24, 28, 38) };
    let stroke = if active { Stroke::new(1.0_f32, Color32::from_rgb(150, 220, 20)) } else { Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 72)) };
    let text_color = if active { Color32::from_rgb(10, 16, 8) } else { Color32::from_rgb(203, 213, 225) };
    let btn = egui::Button::new(egui::RichText::new(text).size(11.0).strong().color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(5_u8));
    ui.add(btn).clicked()
}

// Studio level VU Meter
fn render_vu_meter(ui: &mut egui::Ui, level_pct: f32, active: bool, anim_phase: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), egui::Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, CornerRadius::same(3_u8), Color32::from_rgb(12, 16, 22));
    painter.rect_stroke(rect, CornerRadius::same(3_u8), Stroke::new(1.0_f32, Color32::from_rgb(28, 36, 48)), egui::StrokeKind::Inside);

    if !active {
        return;
    }

    let wave = ((anim_phase * 6.0).sin() * 0.15 + (anim_phase * 14.0).cos() * 0.10).max(-0.2);
    let fill_ratio = ((level_pct / 100.0) * (0.65 + wave)).clamp(0.02, 1.0);
    let filled_width = rect.width() * fill_ratio;

    let num_segments = 24;
    let seg_w = (rect.width() - (num_segments as f32 - 1.0) * 2.0) / num_segments as f32;
    for i in 0..num_segments {
        let seg_x = rect.min.x + i as f32 * (seg_w + 2.0);
        if seg_x > rect.min.x + filled_width {
            break;
        }
        let seg_rect = egui::Rect::from_min_size(egui::pos2(seg_x, rect.min.y + 2.0), Vec2::new(seg_w, rect.height() - 4.0));
        let pos_frac = i as f32 / num_segments as f32;
        let seg_color = if pos_frac < 0.65 {
            Color32::from_rgb(118, 185, 0) // NVIDIA Green
        } else if pos_frac < 0.85 {
            Color32::from_rgb(234, 179, 8) // Amber
        } else {
            Color32::from_rgb(239, 68, 68) // Red
        };
        painter.rect_filled(seg_rect, CornerRadius::same(2_u8), seg_color);
    }
}

// Geometric Icon Renderers
fn draw_replay_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    use std::f32::consts::PI;
    let stroke = Stroke::new(3.0_f32, color);
    let start_angle = 0.25 * PI;
    let end_angle = 1.80 * PI;
    let steps = 24;
    let mut points = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = start_angle + t * (end_angle - start_angle);
        points.push(center + Vec2::new(angle.cos() * radius, angle.sin() * radius));
    }
    for window in points.windows(2) {
        painter.line_segment([window[0], window[1]], stroke);
    }

    // Arrowhead pointing counter-clockwise
    let arrow_tip = points[0];
    let tangent = Vec2::new(-start_angle.sin(), start_angle.cos());
    let normal = Vec2::new(start_angle.cos(), start_angle.sin());
    let p1 = arrow_tip - tangent * 7.0 + normal * 5.0;
    let p2 = arrow_tip - tangent * 7.0 - normal * 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![arrow_tip, p1, p2],
        color,
        Stroke::NONE,
    ));

    // Centered play triangle
    let tri_r = radius * 0.35;
    let tri_center = center + Vec2::new(1.5, 0.0);
    let t_tip = tri_center + Vec2::new(tri_r, 0.0);
    let t_top = tri_center + Vec2::new(-tri_r * 0.6, -tri_r * 0.86);
    let t_bot = tri_center + Vec2::new(-tri_r * 0.6, tri_r * 0.86);
    painter.add(egui::Shape::convex_polygon(
        vec![t_tip, t_top, t_bot],
        color,
        Stroke::NONE,
    ));
}

fn draw_record_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_recording: bool, anim_time: f32) {
    if is_recording {
        let pulse = ((anim_time * 5.0).sin() * 0.5 + 0.5) * 4.0;
        let glow_color = Color32::from_rgba_unmultiplied(239, 68, 68, 55);
        painter.circle_filled(center, radius + 4.0 + pulse, glow_color);
        painter.circle_stroke(center, radius, Stroke::new(3.0_f32, Color32::from_rgb(239, 68, 68)));
        painter.circle_filled(center, radius * 0.45, Color32::from_rgb(239, 68, 68));
    } else {
        painter.circle_stroke(center, radius, Stroke::new(2.5_f32, Color32::from_rgb(100, 116, 139)));
        painter.circle_filled(center, radius * 0.42, Color32::from_rgb(226, 232, 240));
    }
}

fn draw_gallery_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    let w = radius * 1.45;
    let h = radius * 1.05;
    let rect = egui::Rect::from_center_size(center, Vec2::new(w, h));
    painter.rect_stroke(rect, CornerRadius::same(4_u8), Stroke::new(2.5_f32, color), egui::StrokeKind::Inside);

    let tri_r = radius * 0.35;
    let tri_center = center + Vec2::new(1.0, 0.0);
    let t_tip = tri_center + Vec2::new(tri_r, 0.0);
    let t_top = tri_center + Vec2::new(-tri_r * 0.6, -tri_r * 0.86);
    let t_bot = tri_center + Vec2::new(-tri_r * 0.6, tri_r * 0.86);
    painter.add(egui::Shape::convex_polygon(
        vec![t_tip, t_top, t_bot],
        color,
        Stroke::NONE,
    ));

    let notch_color = Color32::from_rgb(100, 116, 139);
    for dy in [-h * 0.28, h * 0.28] {
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(rect.min.x + 4.0, center.y + dy), Vec2::new(3.0, 3.0)),
            CornerRadius::same(1_u8),
            notch_color,
        );
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(rect.max.x - 4.0, center.y + dy), Vec2::new(3.0, 3.0)),
            CornerRadius::same(1_u8),
            notch_color,
        );
    }
}

fn draw_mic_icon(painter: &egui::Painter, center: egui::Pos2, size: f32, color: Color32, muted: bool) {
    let body_w = size * 0.38;
    let body_h = size * 0.60;
    let body_rect = egui::Rect::from_center_size(center - Vec2::new(0.0, size * 0.12), Vec2::new(body_w, body_h));
    painter.rect_filled(body_rect, CornerRadius::same((body_w / 2.0) as u8), color);

    let cradle_r = size * 0.36;
    let cradle_center = center - Vec2::new(0.0, size * 0.06);
    let stroke = Stroke::new(2.0_f32, color);
    use std::f32::consts::PI;
    let steps = 12;
    let mut arc = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = 0.0 + t * PI;
        arc.push(cradle_center + Vec2::new(angle.cos() * cradle_r, angle.sin() * cradle_r));
    }
    for window in arc.windows(2) {
        painter.line_segment([window[0], window[1]], stroke);
    }

    let stem_top = cradle_center + Vec2::new(0.0, cradle_r);
    let stem_bot = stem_top + Vec2::new(0.0, size * 0.20);
    painter.line_segment([stem_top, stem_bot], stroke);
    painter.line_segment(
        [stem_bot - Vec2::new(size * 0.22, 0.0), stem_bot + Vec2::new(size * 0.22, 0.0)],
        stroke,
    );

    if muted {
        let slash_stroke = Stroke::new(2.5_f32, Color32::from_rgb(239, 68, 68));
        let p1 = center - Vec2::new(size * 0.5, size * 0.5);
        let p2 = center + Vec2::new(size * 0.5, size * 0.5);
        painter.line_segment([p1, p2], slash_stroke);
    }
}

fn draw_cursor_icon(painter: &egui::Painter, center: egui::Pos2, size: f32, color: Color32, visible: bool) {
    let p0 = center + Vec2::new(-size * 0.35, -size * 0.45);
    let p1 = p0 + Vec2::new(0.0, size * 0.90);
    let p2 = p0 + Vec2::new(size * 0.28, size * 0.65);
    let p3 = p0 + Vec2::new(size * 0.58, size * 0.90);
    let p4 = p0 + Vec2::new(size * 0.72, size * 0.78);
    let p5 = p0 + Vec2::new(size * 0.42, size * 0.52);
    let p6 = p0 + Vec2::new(size * 0.72, size * 0.52);

    let poly = vec![p0, p1, p2, p3, p4, p5, p6];
    let fill = if visible { color } else { Color32::from_rgb(71, 85, 105) };
    let border = if visible { Color32::BLACK } else { Color32::from_rgb(30, 41, 59) };
    painter.add(egui::Shape::convex_polygon(poly, fill, Stroke::new(1.5_f32, border)));

    if !visible {
        let slash_stroke = Stroke::new(2.0_f32, Color32::from_rgb(239, 68, 68));
        painter.line_segment(
            [center - Vec2::new(size * 0.45, size * 0.45), center + Vec2::new(size * 0.45, size * 0.45)],
            slash_stroke,
        );
    }
}

fn draw_gear_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    let stroke = Stroke::new(2.0_f32, color);
    painter.circle_stroke(center, radius * 0.75, stroke);
    painter.circle_filled(center, radius * 0.30, Color32::from_rgb(18, 20, 24));
    painter.circle_stroke(center, radius * 0.30, stroke);

    use std::f32::consts::PI;
    for i in 0..8 {
        let angle = i as f32 * (PI / 4.0);
        let p_in = center + Vec2::new(angle.cos() * (radius * 0.68), angle.sin() * (radius * 0.68));
        let p_out = center + Vec2::new(angle.cos() * (radius * 1.05), angle.sin() * (radius * 1.05));
        painter.line_segment([p_in, p_out], Stroke::new(2.5_f32, color));
    }
}

fn draw_close_icon(painter: &egui::Painter, center: egui::Pos2, size: f32, color: Color32) {
    let half = size * 0.5;
    let stroke = Stroke::new(2.0_f32, color);
    painter.line_segment([center - Vec2::new(half, half), center + Vec2::new(half, half)], stroke);
    painter.line_segment([center - Vec2::new(half, -half), center + Vec2::new(half, -half)], stroke);
}

fn icon_button(ui: &mut egui::Ui, size: Vec2, hover_desc: &str) -> (egui::Response, egui::Painter, egui::Rect) {
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter();
    let bg_color = if response.is_pointer_button_down_on() {
        Color32::from_rgb(34, 44, 60)
    } else if response.hovered() {
        Color32::from_rgb(26, 34, 46)
    } else {
        Color32::from_rgb(16, 20, 28)
    };
    let border_color = if response.hovered() {
        Color32::from_rgb(80, 95, 120)
    } else {
        Color32::from_rgb(36, 46, 62)
    };
    painter.rect_filled(rect, CornerRadius::same(6_u8), bg_color);
    painter.rect_stroke(rect, CornerRadius::same(6_u8), Stroke::new(1.0_f32, border_color), egui::StrokeKind::Inside);
    if !hover_desc.is_empty() {
        response = response.on_hover_text(hover_desc);
    }
    (response, painter.clone(), rect)
}

pub struct VrecOverlayApp {
    config: VrecConfig,
    status: DaemonStatus,
    daemon_connected: bool,
    current_view: ShadowPlayView,
    output_dir: String,
    replay_sec: u32,
    bitrate_mbps: u32,
    target_fps: u32,
    audio_mode_idx: usize,
    show_cursor: bool,
    mic_volume_pct: u32,
    system_volume_pct: u32,
    anim_time: f32,
    status_msg: Option<(String, Instant)>,
    status_rx: Receiver<DaemonStatus>,
    folder_tx: Sender<String>,
    folder_rx: Receiver<String>,
    clips: Vec<VideoClipInfo>,
    gallery_filter_idx: usize,
    last_clip_scan: Instant,
}

impl Default for VrecOverlayApp {
    fn default() -> Self {
        Self::new()
    }
}

impl VrecOverlayApp {
    pub fn new() -> Self {
        let config = VrecConfig::load();
        let replay_sec = config.replay_duration_sec;
        let bitrate_mbps = (config.record_bitrate_kbps / 1000).max(1);
        let target_fps = config.fps;
        let output_dir = config.output_directory.clone();
        let show_cursor = config.show_cursor;
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
                std::thread::sleep(Duration::from_millis(250));
            }
        });

        let (folder_tx, folder_rx) = channel::<String>();
        let clips = scan_recordings(&output_dir);

        Self {
            config,
            status: DaemonStatus::default(),
            daemon_connected: false,
            current_view: ShadowPlayView::MainHud,
            output_dir,
            replay_sec,
            bitrate_mbps,
            target_fps,
            audio_mode_idx,
            show_cursor,
            mic_volume_pct,
            system_volume_pct,
            anim_time: 0.0,
            status_msg: None,
            status_rx,
            folder_tx,
            folder_rx,
            clips,
            gallery_filter_idx: 0,
            last_clip_scan: Instant::now(),
        }
    }

    pub fn refresh_clips(&mut self) {
        self.clips = scan_recordings(&self.output_dir);
        self.last_clip_scan = Instant::now();
    }

    fn poll_async_events(&mut self) {
        while let Ok(s) = self.status_rx.try_recv() {
            if self.current_view == ShadowPlayView::MainHud {
                self.show_cursor = s.show_cursor;
            }
            self.status = s;
            self.daemon_connected = true;
        }

        if let Ok(chosen_dir) = self.folder_rx.try_recv() {
            let trimmed = chosen_dir.trim();
            if !trimmed.is_empty() {
                self.output_dir = trimmed.to_string();
                self.refresh_clips();
                self.set_msg("Save directory updated!");
            }
        }

        if self.current_view == ShadowPlayView::Gallery && self.last_clip_scan.elapsed() > Duration::from_secs(2) {
            self.refresh_clips();
        }
    }

    fn set_msg(&mut self, text: &str) {
        self.status_msg = Some((text.to_string(), Instant::now()));
    }
}

impl eframe::App for VrecOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async_events();
        self.anim_time += 0.033;
        ctx.request_repaint_after(Duration::from_millis(50));

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.current_view != ShadowPlayView::MainHud {
                self.current_view = ShadowPlayView::MainHud;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(12, 15, 20);
        visuals.window_fill = Color32::from_rgb(12, 15, 20);
        visuals.window_stroke = Stroke::new(1.5_f32, Color32::from_rgb(36, 44, 58));
        visuals.window_corner_radius = CornerRadius::same(14_u8);
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgb(12, 15, 20))
                    .stroke(Stroke::new(1.5_f32, Color32::from_rgb(36, 44, 58)))
                    .corner_radius(CornerRadius::same(14_u8))
                    .inner_margin(Margin::same(18_i8)),
            )
            .show(ctx, |ui| {
                // TOP HEADER BAR
                ui.horizontal(|ui| {
                    // NVIDIA Green Brand Badge
                    let (brand_rect, _) = ui.allocate_exact_size(Vec2::new(50.0, 24.0), egui::Sense::hover());
                    ui.painter().rect_filled(brand_rect, CornerRadius::same(4_u8), Color32::from_rgb(118, 185, 0));
                    ui.painter().text(
                        brand_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "VREC",
                        FontId::proportional(12.0),
                        Color32::from_rgb(10, 16, 8),
                    );

                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("SHADOWPLAY")
                                .font(FontId::proportional(13.0))
                                .strong()
                                .color(Color32::WHITE),
                        );
                    });

                    ui.add_space(16.0);

                    // Central Status Capsule
                    if self.status.is_recording {
                        let dur = self.status.recording_duration_sec;
                        let text = format!("● RECORDING {:02}:{:02}", dur / 60, dur % 60);
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(239, 68, 68, 35))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68)))
                            .corner_radius(CornerRadius::same(12_u8))
                            .inner_margin(Margin::symmetric(12_i8, 4_i8))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(text).font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(248, 113, 113)));
                            });
                    } else if self.status.is_replay_active {
                        let text = format!("● INSTANT REPLAY ACTIVE ({}s)", self.replay_sec);
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(118, 185, 0, 30))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(118, 185, 0)))
                            .corner_radius(CornerRadius::same(12_u8))
                            .inner_margin(Margin::symmetric(12_i8, 4_i8))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(text).font(FontId::monospace(10.5)).strong().color(Color32::from_rgb(163, 230, 53)));
                            });
                    } else {
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(20, 26, 36))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 72)))
                            .corner_radius(CornerRadius::same(12_u8))
                            .inner_margin(Margin::symmetric(12_i8, 4_i8))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("STANDBY").font(FontId::monospace(10.5)).color(Color32::from_rgb(148, 163, 184)));
                            });
                    }

                    // Toast message inline if active
                    if let Some((ref msg, ts)) = self.status_msg {
                        if ts.elapsed() < Duration::from_secs(3) {
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(format!("✓ {}", msg)).font(FontId::proportional(11.0)).color(Color32::from_rgb(118, 185, 0)));
                        } else {
                            self.status_msg = None;
                        }
                    }

                    // Right quick controls
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Close button
                        let (btn_close, p_close, r_close) = icon_button(ui, Vec2::new(32.0, 32.0), "Close Overlay (Esc)");
                        draw_close_icon(&p_close, r_close.center(), 12.0, Color32::from_rgb(148, 163, 184));
                        if btn_close.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        // Settings gear button
                        let (btn_gear, p_gear, r_gear) = icon_button(ui, Vec2::new(32.0, 32.0), "Capture Settings");
                        let gear_col = if self.current_view == ShadowPlayView::Settings { Color32::from_rgb(118, 185, 0) } else { Color32::from_rgb(203, 213, 225) };
                        draw_gear_icon(&p_gear, r_gear.center(), 8.5, gear_col);
                        if btn_gear.clicked() {
                            self.current_view = if self.current_view == ShadowPlayView::Settings { ShadowPlayView::MainHud } else { ShadowPlayView::Settings };
                        }

                        // Gallery button
                        let (btn_gal, p_gal, r_gal) = icon_button(ui, Vec2::new(32.0, 32.0), "Recordings Gallery");
                        let gal_col = if self.current_view == ShadowPlayView::Gallery { Color32::from_rgb(14, 165, 233) } else { Color32::from_rgb(203, 213, 225) };
                        draw_gallery_icon(&p_gal, r_gal.center(), 9.5, gal_col);
                        if btn_gal.clicked() {
                            self.refresh_clips();
                            self.current_view = if self.current_view == ShadowPlayView::Gallery { ShadowPlayView::MainHud } else { ShadowPlayView::Gallery };
                        }

                        // Cursor toggle button
                        let cursor_tip = if self.show_cursor { "Cursor: Visible (Click to Hide)" } else { "Cursor: Hidden (Click to Show)" };
                        let (btn_cur, p_cur, r_cur) = icon_button(ui, Vec2::new(32.0, 32.0), cursor_tip);
                        let cur_col = if self.show_cursor { Color32::from_rgb(118, 185, 0) } else { Color32::from_rgb(100, 116, 139) };
                        draw_cursor_icon(&p_cur, r_cur.center(), 17.0, cur_col, self.show_cursor);
                        if btn_cur.clicked() {
                            self.show_cursor = !self.show_cursor;
                            self.config.show_cursor = self.show_cursor;
                            let _ = self.config.save();
                            let _ = ipc::send_command(Command::ToggleCursor);
                            self.set_msg(if self.show_cursor { "Cursor: Visible in recording" } else { "Cursor: Hidden from recording" });
                        }

                        // Audio Mixer button
                        let audio_tip = format!("Audio Mode: {} (Click to configure)", self.status.audio_mode);
                        let (btn_mic, p_mic, r_mic) = icon_button(ui, Vec2::new(32.0, 32.0), &audio_tip);
                        let mic_col = if self.current_view == ShadowPlayView::AudioMixer {
                            Color32::from_rgb(118, 185, 0)
                        } else if self.status.audio_muted {
                            Color32::from_rgb(239, 68, 68)
                        } else {
                            Color32::from_rgb(203, 213, 225)
                        };
                        draw_mic_icon(&p_mic, r_mic.center(), 17.0, mic_col, self.status.audio_muted);
                        if btn_mic.clicked() {
                            self.current_view = if self.current_view == ShadowPlayView::AudioMixer { ShadowPlayView::MainHud } else { ShadowPlayView::AudioMixer };
                        }
                    });
                });

                ui.add_space(14.0);

                // VIEW ROUTING
                match self.current_view {
                    ShadowPlayView::MainHud => {
                        // THE 3 ICONIC SHADOWPLAY HERO TILES
                        ui.horizontal(|ui| {
                            let available_w = ui.available_width();
                            let card_w = ((available_w - 32.0) / 3.0).clamp(240.0, 270.0);
                            let card_h = 285.0;

                            // CARD 1: INSTANT REPLAY
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 27))
                                .stroke(Stroke::new(1.2_f32, if self.status.is_replay_active { Color32::from_rgb(55, 85, 25) } else { Color32::from_rgb(34, 44, 58) }))
                                .corner_radius(CornerRadius::same(12_u8))
                                .inner_margin(Margin::symmetric(14_i8, 16_i8))
                                .show(ui, |ui| {
                                    ui.set_width(card_w);
                                    ui.set_height(card_h);
                                    ui.vertical_centered(|ui| {
                                        let (btn, p, r) = icon_button(ui, Vec2::new(82.0, 82.0), "Save Instant Replay (Ctrl+Shift+R)");
                                        let circle_center = r.center();
                                        let fill_color = if self.status.is_replay_active { Color32::from_rgb(24, 44, 16) } else { Color32::from_rgb(20, 26, 36) };
                                        let stroke_color = if btn.hovered() { Color32::from_rgb(150, 220, 20) } else if self.status.is_replay_active { Color32::from_rgb(118, 185, 0) } else { Color32::from_rgb(60, 75, 95) };
                                        p.circle_filled(circle_center, 38.0, fill_color);
                                        p.circle_stroke(circle_center, 38.0, Stroke::new(2.5_f32, stroke_color));
                                        draw_replay_icon(&p, circle_center, 22.0, stroke_color);

                                        if btn.clicked() {
                                            let _ = ipc::send_command(Command::SaveReplay);
                                            self.set_msg("Instant Replay saved!");
                                            self.refresh_clips();
                                        }

                                        ui.add_space(10.0);
                                        ui.label(
                                            egui::RichText::new("Instant Replay")
                                                .font(FontId::proportional(15.0))
                                                .strong()
                                                .color(Color32::WHITE),
                                        );

                                        if self.status.is_replay_active {
                                            ui.label(egui::RichText::new("● ON").font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(118, 185, 0)));
                                        } else {
                                            ui.label(egui::RichText::new("OFF").font(FontId::monospace(11.0)).color(Color32::from_rgb(100, 116, 139)));
                                        }

                                        ui.add_space(12.0);
                                        let save_btn = egui::Button::new(egui::RichText::new("Save Replay").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(10, 16, 8)))
                                            .fill(Color32::from_rgb(118, 185, 0))
                                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(150, 220, 20)))
                                            .corner_radius(CornerRadius::same(6_u8));
                                        if ui.add_sized([card_w - 28.0, 32.0], save_btn).clicked() {
                                            let _ = ipc::send_command(Command::SaveReplay);
                                            self.set_msg("Instant Replay saved!");
                                            self.refresh_clips();
                                        }

                                        ui.add_space(6.0);
                                        let buffer_lbl = egui::Button::new(egui::RichText::new(format!("Buffer: {}s", self.replay_sec)).font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)))
                                            .fill(Color32::from_rgb(22, 28, 38))
                                            .corner_radius(CornerRadius::same(5_u8));
                                        if ui.add_sized([card_w - 28.0, 24.0], buffer_lbl).clicked() {
                                            self.current_view = ShadowPlayView::Settings;
                                        }
                                    });
                                });

                            ui.add_space(16.0);

                            // CARD 2: RECORD
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 27))
                                .stroke(Stroke::new(1.2_f32, if self.status.is_recording { Color32::from_rgb(140, 35, 35) } else { Color32::from_rgb(34, 44, 58) }))
                                .corner_radius(CornerRadius::same(12_u8))
                                .inner_margin(Margin::symmetric(14_i8, 16_i8))
                                .show(ui, |ui| {
                                    ui.set_width(card_w);
                                    ui.set_height(card_h);
                                    ui.vertical_centered(|ui| {
                                        let rec_tip = if self.status.is_recording { "Stop Recording (Ctrl+Shift+F9)" } else { "Start Recording (Ctrl+Shift+F9)" };
                                        let (btn, p, r) = icon_button(ui, Vec2::new(82.0, 82.0), rec_tip);
                                        let circle_center = r.center();
                                        let fill_color = if self.status.is_recording { Color32::from_rgb(45, 12, 14) } else { Color32::from_rgb(20, 26, 36) };
                                        let stroke_color = if self.status.is_recording { Color32::from_rgb(239, 68, 68) } else if btn.hovered() { Color32::from_rgb(203, 213, 225) } else { Color32::from_rgb(71, 85, 105) };
                                        p.circle_filled(circle_center, 38.0, fill_color);
                                        p.circle_stroke(circle_center, 38.0, Stroke::new(2.5_f32, stroke_color));
                                        draw_record_icon(&p, circle_center, 22.0, self.status.is_recording, self.anim_time);

                                        if btn.clicked() {
                                            let _ = ipc::send_command(Command::ToggleRecording);
                                            if self.status.is_recording {
                                                self.set_msg("Recording stopped and saved!");
                                                self.refresh_clips();
                                            } else {
                                                self.set_msg("Recording started");
                                            }
                                        }

                                        ui.add_space(10.0);
                                        ui.label(
                                            egui::RichText::new("Record")
                                                .font(FontId::proportional(15.0))
                                                .strong()
                                                .color(Color32::WHITE),
                                        );

                                        if self.status.is_recording {
                                            let dur = self.status.recording_duration_sec;
                                            ui.label(egui::RichText::new(format!("● {:02}:{:02}", dur / 60, dur % 60)).font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(239, 68, 68)));
                                        } else {
                                            ui.label(egui::RichText::new("OFF").font(FontId::monospace(11.0)).color(Color32::from_rgb(100, 116, 139)));
                                        }

                                        ui.add_space(12.0);
                                        if self.status.is_recording {
                                            let stop_btn = egui::Button::new(egui::RichText::new("Stop Recording").font(FontId::proportional(12.0)).strong().color(Color32::WHITE))
                                                .fill(Color32::from_rgb(239, 68, 68))
                                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(248, 113, 113)))
                                                .corner_radius(CornerRadius::same(6_u8));
                                            if ui.add_sized([card_w - 28.0, 32.0], stop_btn).clicked() {
                                                let _ = ipc::send_command(Command::StopRecording);
                                                self.set_msg("Recording saved to disk!");
                                                self.refresh_clips();
                                            }
                                        } else {
                                            let start_btn = egui::Button::new(egui::RichText::new("Start Recording").font(FontId::proportional(12.0)).strong().color(Color32::WHITE))
                                                .fill(Color32::from_rgb(30, 41, 59))
                                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(71, 85, 105)))
                                                .corner_radius(CornerRadius::same(6_u8));
                                            if ui.add_sized([card_w - 28.0, 32.0], start_btn).clicked() {
                                                let _ = ipc::send_command(Command::StartRecording);
                                                self.set_msg("Recording started");
                                            }
                                        }

                                        ui.add_space(6.0);
                                        let q_lbl = egui::Button::new(egui::RichText::new(format!("{} fps • {} Mbps", self.target_fps, self.bitrate_mbps)).font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)))
                                            .fill(Color32::from_rgb(22, 28, 38))
                                            .corner_radius(CornerRadius::same(5_u8));
                                        if ui.add_sized([card_w - 28.0, 24.0], q_lbl).clicked() {
                                            self.current_view = ShadowPlayView::Settings;
                                        }
                                    });
                                });

                            ui.add_space(16.0);

                            // CARD 3: GALLERY
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 27))
                                .stroke(Stroke::new(1.2_f32, Color32::from_rgb(34, 44, 58)))
                                .corner_radius(CornerRadius::same(12_u8))
                                .inner_margin(Margin::symmetric(14_i8, 16_i8))
                                .show(ui, |ui| {
                                    ui.set_width(card_w);
                                    ui.set_height(card_h);
                                    ui.vertical_centered(|ui| {
                                        let (btn, p, r) = icon_button(ui, Vec2::new(82.0, 82.0), "Browse Recordings & Clips");
                                        let circle_center = r.center();
                                        let fill_color = Color32::from_rgb(14, 26, 38);
                                        let stroke_color = if btn.hovered() { Color32::from_rgb(56, 189, 248) } else { Color32::from_rgb(14, 165, 233) };
                                        p.circle_filled(circle_center, 38.0, fill_color);
                                        p.circle_stroke(circle_center, 38.0, Stroke::new(2.5_f32, stroke_color));
                                        draw_gallery_icon(&p, circle_center, 20.0, stroke_color);

                                        if btn.clicked() {
                                            self.refresh_clips();
                                            self.current_view = ShadowPlayView::Gallery;
                                        }

                                        ui.add_space(10.0);
                                        ui.label(
                                            egui::RichText::new("Gallery")
                                                .font(FontId::proportional(15.0))
                                                .strong()
                                                .color(Color32::WHITE),
                                        );

                                        ui.label(egui::RichText::new(format!("{} CLIPS", self.clips.len())).font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(56, 189, 248)));

                                        ui.add_space(12.0);
                                        let gal_btn = egui::Button::new(egui::RichText::new("View Clips").font(FontId::proportional(12.0)).strong().color(Color32::WHITE))
                                            .fill(Color32::from_rgb(2, 132, 199))
                                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(56, 189, 248)))
                                            .corner_radius(CornerRadius::same(6_u8));
                                        if ui.add_sized([card_w - 28.0, 32.0], gal_btn).clicked() {
                                            self.refresh_clips();
                                            self.current_view = ShadowPlayView::Gallery;
                                        }

                                        ui.add_space(6.0);
                                        let folder_btn = egui::Button::new(egui::RichText::new("Open Folder").font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)))
                                            .fill(Color32::from_rgb(22, 28, 38))
                                            .corner_radius(CornerRadius::same(5_u8));
                                        if ui.add_sized([card_w - 28.0, 24.0], folder_btn).clicked() {
                                            let resolved = VrecConfig::expand_tilde(&self.output_dir);
                                            let _ = std::fs::create_dir_all(&resolved);
                                            open_folder(&resolved);
                                        }
                                    });
                                });
                        });

                        ui.add_space(14.0);

                        // BOTTOM STATUS BAR
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Shortcuts:").font(FontId::proportional(11.0)).color(Color32::from_rgb(100, 116, 139)));
                            render_keycap(ui, &self.config.menu_hotkey);
                            ui.label(egui::RichText::new("Menu").font(FontId::proportional(10.0)).color(Color32::from_rgb(148, 163, 184)));

                            ui.add_space(8.0);
                            render_keycap(ui, &self.config.save_hotkey);
                            ui.label(egui::RichText::new("Save").font(FontId::proportional(10.0)).color(Color32::from_rgb(148, 163, 184)));

                            ui.add_space(8.0);
                            render_keycap(ui, &self.config.record_hotkey);
                            ui.label(egui::RichText::new("Record").font(FontId::proportional(10.0)).color(Color32::from_rgb(148, 163, 184)));

                            ui.add_space(8.0);
                            render_keycap(ui, &self.config.cursor_hotkey);
                            ui.label(egui::RichText::new("Cursor").font(FontId::proportional(10.0)).color(Color32::from_rgb(148, 163, 184)));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if self.daemon_connected {
                                    let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 3.5, Color32::from_rgb(118, 185, 0));
                                    ui.label(egui::RichText::new("Daemon Online").font(FontId::monospace(9.5)).color(Color32::from_rgb(118, 185, 0)));
                                } else {
                                    let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 3.5, Color32::from_rgb(239, 68, 68));
                                    ui.label(egui::RichText::new("Daemon Offline").font(FontId::monospace(9.5)).color(Color32::from_rgb(239, 68, 68)));
                                }

                                ui.add_space(10.0);
                                #[cfg(target_os = "windows")]
                                let codec_str = "D3D11 / NVENC H.264";
                                #[cfg(not(target_os = "windows"))]
                                let codec_str = "Hardware VAAPI H.264";
                                ui.label(egui::RichText::new(codec_str).font(FontId::monospace(9.5)).color(Color32::from_rgb(100, 116, 139)));
                            });
                        });
                    }

                    ShadowPlayView::Settings => {
                        // SUBVIEW: SETTINGS
                        ui.horizontal(|ui| {
                            let back_btn = egui::Button::new(egui::RichText::new("< Back to Overlay").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(118, 185, 0)))
                                .fill(Color32::from_rgb(20, 32, 16))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 95, 25)))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add(back_btn).clicked() {
                                self.current_view = ShadowPlayView::MainHud;
                            }

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("SHADOWPLAY SETTINGS").font(FontId::proportional(14.0)).strong().color(Color32::WHITE));
                        });

                        ui.add_space(10.0);

                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            // Save Directory Card
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 27))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 58)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("SAVE DESTINATION DIRECTORY").font(FontId::proportional(11.5)).strong().color(Color32::from_rgb(118, 185, 0)));
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(520.0));

                                        let browse_btn = egui::Button::new(egui::RichText::new("Browse...").color(Color32::WHITE))
                                            .fill(Color32::from_rgb(37, 99, 235))
                                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)))
                                            .corner_radius(CornerRadius::same(5_u8));
                                        if ui.add(browse_btn).clicked() {
                                            let cur = VrecConfig::expand_tilde(&self.output_dir).to_string_lossy().to_string();
                                            pick_folder(&cur, self.folder_tx.clone());
                                        }

                                        let open_btn = egui::Button::new(egui::RichText::new("Open").color(Color32::from_rgb(203, 213, 225)))
                                            .fill(Color32::from_rgb(30, 41, 59))
                                            .corner_radius(CornerRadius::same(5_u8));
                                        if ui.add(open_btn).clicked() {
                                            let dir = VrecConfig::expand_tilde(&self.output_dir);
                                            let _ = std::fs::create_dir_all(&dir);
                                            open_folder(&dir);
                                        }
                                    });

                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Quick Presets:").font(FontId::proportional(10.0)).color(Color32::from_rgb(100, 116, 139)));
                                        let v_base = dirs::video_dir().or_else(|| dirs::home_dir().map(|h| h.join("Videos")));
                                        let d_base = dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads")));
                                        if let Some(ref vb) = v_base {
                                            let v_dir = vb.join("vrec").to_string_lossy().to_string();
                                            let c_dir = vb.join("Captures").to_string_lossy().to_string();
                                            if pill(ui, "Videos/vrec", self.output_dir == v_dir) { self.output_dir = v_dir; }
                                            if pill(ui, "Videos/Captures", self.output_dir == c_dir) { self.output_dir = c_dir; }
                                        }
                                        if let Some(ref db) = d_base {
                                            let d_dir = db.join("vrec").to_string_lossy().to_string();
                                            if pill(ui, "Downloads/vrec", self.output_dir == d_dir) { self.output_dir = d_dir; }
                                        }
                                    });
                                });

                            ui.add_space(8.0);

                            // Two-column Settings
                            ui.columns(2, |cols| {
                                // Column 1: Video Quality & Cursor
                                egui::Frame::NONE
                                    .fill(Color32::from_rgb(16, 20, 27))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 58)))
                                    .corner_radius(CornerRadius::same(8_u8))
                                    .inner_margin(Margin::same(12_i8))
                                    .show(&mut cols[0], |ui| {
                                        ui.label(egui::RichText::new("VIDEO QUALITY & ENCODING").font(FontId::proportional(11.5)).strong().color(Color32::from_rgb(118, 185, 0)));
                                        ui.add_space(6.0);

                                        ui.label(egui::RichText::new("Target Framerate").font(FontId::proportional(10.5)).color(Color32::from_rgb(203, 213, 225)));
                                        ui.horizontal(|ui| {
                                            ui.add(egui::DragValue::new(&mut self.target_fps).range(15..=240).suffix(" fps"));
                                            if pill(ui, "60", self.target_fps == 60) { self.target_fps = 60; }
                                            if pill(ui, "120", self.target_fps == 120) { self.target_fps = 120; }
                                            if pill(ui, "144", self.target_fps == 144) { self.target_fps = 144; }
                                        });

                                        ui.add_space(8.0);
                                        ui.label(egui::RichText::new("Video Bitrate").font(FontId::proportional(10.5)).color(Color32::from_rgb(203, 213, 225)));
                                        ui.horizontal(|ui| {
                                            ui.add(egui::DragValue::new(&mut self.bitrate_mbps).range(2..=120).suffix(" Mbps"));
                                            if pill(ui, "10M", self.bitrate_mbps == 10) { self.bitrate_mbps = 10; }
                                            if pill(ui, "20M", self.bitrate_mbps == 20) { self.bitrate_mbps = 20; }
                                            if pill(ui, "30M", self.bitrate_mbps == 30) { self.bitrate_mbps = 30; }
                                        });

                                        ui.add_space(8.0);
                                        ui.label(egui::RichText::new("Mouse Cursor").font(FontId::proportional(10.5)).color(Color32::from_rgb(203, 213, 225)));
                                        ui.horizontal(|ui| {
                                            if pill(ui, "Show Cursor", self.show_cursor) { self.show_cursor = true; }
                                            if pill(ui, "Hide Cursor", !self.show_cursor) { self.show_cursor = false; }
                                        });

                                        ui.add_space(6.0);
                                        #[cfg(target_os = "windows")]
                                        let codec_info = "Codec: Hardware D3D11 / NVENC H.264";
                                        #[cfg(not(target_os = "windows"))]
                                        let codec_info = "Codec: Hardware VAAPI H.264 (NV12 Direct Buffer Sharing)";
                                        ui.label(egui::RichText::new(codec_info).font(FontId::monospace(9.0)).color(Color32::from_rgb(100, 116, 139)));
                                    });

                                // Column 2: Replay Buffer & Hotkeys
                                egui::Frame::NONE
                                    .fill(Color32::from_rgb(16, 20, 27))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 58)))
                                    .corner_radius(CornerRadius::same(8_u8))
                                    .inner_margin(Margin::same(12_i8))
                                    .show(&mut cols[1], |ui| {
                                        ui.label(egui::RichText::new("REPLAY BUFFER & SHORTCUTS").font(FontId::proportional(11.5)).strong().color(Color32::from_rgb(168, 85, 247)));
                                        ui.add_space(6.0);

                                        ui.label(egui::RichText::new("Buffer Duration").font(FontId::proportional(10.5)).color(Color32::from_rgb(203, 213, 225)));
                                        ui.horizontal(|ui| {
                                            ui.add(egui::DragValue::new(&mut self.replay_sec).range(5..=600).suffix(" s"));
                                            if pill(ui, "30s", self.replay_sec == 30) { self.replay_sec = 30; }
                                            if pill(ui, "60s", self.replay_sec == 60) { self.replay_sec = 60; }
                                            if pill(ui, "120s", self.replay_sec == 120) { self.replay_sec = 120; }
                                        });

                                        ui.add_space(10.0);
                                        ui.label(egui::RichText::new("Configured Global Hotkeys").font(FontId::proportional(10.5)).color(Color32::from_rgb(203, 213, 225)));
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Menu:").font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)));
                                            render_keycap(ui, &self.config.menu_hotkey);
                                            ui.add_space(4.0);
                                            ui.label(egui::RichText::new("Save:").font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)));
                                            render_keycap(ui, &self.config.save_hotkey);
                                        });
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Record:").font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)));
                                            render_keycap(ui, &self.config.record_hotkey);
                                            ui.add_space(4.0);
                                            ui.label(egui::RichText::new("Cursor:").font(FontId::monospace(10.0)).color(Color32::from_rgb(148, 163, 184)));
                                            render_keycap(ui, &self.config.cursor_hotkey);
                                        });
                                    });
                            });

                            ui.add_space(10.0);

                            // Apply Button
                            let apply_btn = egui::Button::new(egui::RichText::new("Save & Apply All Settings").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(10, 16, 8)))
                                .fill(Color32::from_rgb(118, 185, 0))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(150, 220, 20)))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add_sized([260.0, 34.0], apply_btn).clicked() {
                                self.config.output_directory = self.output_dir.clone();
                                let resolved = VrecConfig::expand_tilde(&self.output_dir);
                                let _ = std::fs::create_dir_all(&resolved);
                                self.config.replay_duration_sec = self.replay_sec;
                                self.config.record_bitrate_kbps = self.bitrate_mbps * 1000;
                                self.config.replay_bitrate_kbps = self.bitrate_mbps * 1000;
                                self.config.fps = self.target_fps;
                                self.config.show_cursor = self.show_cursor;
                                self.config.mic_volume = self.mic_volume_pct as f32 / 100.0;
                                self.config.system_volume = self.system_volume_pct as f32 / 100.0;
                                self.config.audio_mode = match self.audio_mode_idx {
                                    1 => "mic",
                                    2 => "both",
                                    3 => "muted",
                                    _ => "system",
                                }.to_string();
                                let _ = self.config.save();
                                let _ = ipc::send_command(Command::ReloadConfig);
                                self.refresh_clips();
                                self.set_msg("Settings saved and reloaded!");
                            }
                        });
                    }

                    ShadowPlayView::Gallery => {
                        // SUBVIEW: RECORDINGS GALLERY
                        ui.horizontal(|ui| {
                            let back_btn = egui::Button::new(egui::RichText::new("< Back to Overlay").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(14, 165, 233)))
                                .fill(Color32::from_rgb(16, 28, 42))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 60, 90)))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add(back_btn).clicked() {
                                self.current_view = ShadowPlayView::MainHud;
                            }

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("RECORDINGS GALLERY").font(FontId::proportional(14.0)).strong().color(Color32::WHITE));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let open_btn = egui::Button::new(egui::RichText::new("Open Folder").size(11.0).color(Color32::WHITE))
                                    .fill(Color32::from_rgb(37, 99, 235))
                                    .corner_radius(CornerRadius::same(5_u8));
                                if ui.add_sized([90.0, 24.0], open_btn).clicked() {
                                    let resolved = VrecConfig::expand_tilde(&self.output_dir);
                                    let _ = std::fs::create_dir_all(&resolved);
                                    open_folder(&resolved);
                                }

                                let refresh_btn = egui::Button::new(egui::RichText::new("Refresh").size(11.0).color(Color32::from_rgb(203, 213, 225)))
                                    .fill(Color32::from_rgb(30, 41, 59))
                                    .corner_radius(CornerRadius::same(5_u8));
                                if ui.add_sized([70.0, 24.0], refresh_btn).clicked() {
                                    self.refresh_clips();
                                }

                                if pill(ui, "Recordings", self.gallery_filter_idx == 2) { self.gallery_filter_idx = 2; }
                                if pill(ui, "Replays", self.gallery_filter_idx == 1) { self.gallery_filter_idx = 1; }
                                if pill(ui, "All", self.gallery_filter_idx == 0) { self.gallery_filter_idx = 0; }
                            });
                        });

                        ui.add_space(10.0);

                        let filtered: Vec<&VideoClipInfo> = self.clips.iter().filter(|c| {
                            match self.gallery_filter_idx {
                                1 => c.is_replay,
                                2 => !c.is_replay,
                                _ => true,
                            }
                        }).collect();

                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            if filtered.is_empty() {
                                egui::Frame::NONE
                                    .fill(Color32::from_rgb(16, 20, 27))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 58)))
                                    .corner_radius(CornerRadius::same(10_u8))
                                    .inner_margin(Margin::same(24_i8))
                                    .show(ui, |ui| {
                                        ui.vertical_centered(|ui| {
                                            ui.label(egui::RichText::new("NO RECORDINGS FOUND").font(FontId::monospace(13.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                            ui.add_space(4.0);
                                            ui.label(egui::RichText::new("Capture your first instant replay or manual recording to see it here.").font(FontId::proportional(11.0)).color(Color32::from_rgb(100, 116, 139)));
                                        });
                                    });
                            } else {
                                for clip in filtered {
                                    egui::Frame::NONE
                                        .fill(Color32::from_rgb(16, 20, 28))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 38, 52)))
                                        .corner_radius(CornerRadius::same(6_u8))
                                        .inner_margin(Margin::symmetric(12_i8, 8_i8))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                // Type badge
                                                if clip.is_replay {
                                                    egui::Frame::NONE
                                                        .fill(Color32::from_rgba_unmultiplied(118, 185, 0, 30))
                                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(118, 185, 0)))
                                                        .corner_radius(CornerRadius::same(4_u8))
                                                        .inner_margin(Margin::symmetric(6_i8, 2_i8))
                                                        .show(ui, |ui| {
                                                            ui.label(egui::RichText::new("REPLAY").font(FontId::monospace(9.5)).strong().color(Color32::from_rgb(163, 230, 53)));
                                                        });
                                                } else {
                                                    egui::Frame::NONE
                                                        .fill(Color32::from_rgba_unmultiplied(239, 68, 68, 30))
                                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68)))
                                                        .corner_radius(CornerRadius::same(4_u8))
                                                        .inner_margin(Margin::symmetric(6_i8, 2_i8))
                                                        .show(ui, |ui| {
                                                            ui.label(egui::RichText::new("REC").font(FontId::monospace(9.5)).strong().color(Color32::from_rgb(248, 113, 113)));
                                                        });
                                                }

                                                ui.add_space(8.0);
                                                ui.vertical(|ui| {
                                                    ui.label(egui::RichText::new(&clip.filename).font(FontId::monospace(11.5)).strong().color(Color32::from_rgb(241, 245, 249)));
                                                    ui.label(egui::RichText::new(format!("{} • {}", format_file_size(clip.size_bytes), format_system_time(clip.modified))).font(FontId::proportional(10.0)).color(Color32::from_rgb(148, 163, 184)));
                                                });

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let play_btn = egui::Button::new(egui::RichText::new("Play Video").size(11.0).color(Color32::WHITE))
                                                        .fill(Color32::from_rgb(16, 185, 129))
                                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(52, 211, 153)))
                                                        .corner_radius(CornerRadius::same(5_u8));
                                                    if ui.add_sized([85.0, 26.0], play_btn).clicked() {
                                                        open_file(&clip.path);
                                                    }
                                                });
                                            });
                                        });
                                    ui.add_space(4.0);
                                }
                            }
                        });
                    }

                    ShadowPlayView::AudioMixer => {
                        // SUBVIEW: AUDIO MIXER
                        ui.horizontal(|ui| {
                            let back_btn = egui::Button::new(egui::RichText::new("< Back to Overlay").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(118, 185, 0)))
                                .fill(Color32::from_rgb(20, 32, 16))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 95, 25)))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add(back_btn).clicked() {
                                self.current_view = ShadowPlayView::MainHud;
                            }

                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("AUDIO MIXER & ROUTING").font(FontId::proportional(14.0)).strong().color(Color32::WHITE));
                        });

                        ui.add_space(10.0);

                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            // Audio Routing Mode Card
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 27))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 58)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("ACTIVE AUDIO ROUTING MODE").font(FontId::proportional(11.5)).strong().color(Color32::from_rgb(118, 185, 0)));
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        if pill(ui, "System Desktop Audio", self.audio_mode_idx == 0) { self.audio_mode_idx = 0; }
                                        if pill(ui, "Microphone Only", self.audio_mode_idx == 1) { self.audio_mode_idx = 1; }
                                        if pill(ui, "Combined (Stereo)", self.audio_mode_idx == 2) { self.audio_mode_idx = 2; }
                                        if pill(ui, "Mute All", self.audio_mode_idx == 3) { self.audio_mode_idx = 3; }
                                    });
                                });

                            ui.add_space(10.0);

                            // Sliders & VU Meters
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 27))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 58)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(ui, |ui| {
                                    // Microphone Section
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Microphone Input Gain").font(FontId::proportional(11.0)).strong().color(Color32::from_rgb(203, 213, 225)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{}%", self.mic_volume_pct)).font(FontId::monospace(11.0)).color(Color32::from_rgb(118, 185, 0)));
                                        });
                                    });
                                    ui.add(egui::Slider::new(&mut self.mic_volume_pct, 0..=200).suffix("%"));
                                    ui.add_space(4.0);
                                    let mic_active = self.audio_mode_idx == 1 || self.audio_mode_idx == 2;
                                    render_vu_meter(ui, self.mic_volume_pct as f32, mic_active, self.anim_time);

                                    ui.add_space(12.0);

                                    // System Audio Section
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("System Desktop Audio Gain").font(FontId::proportional(11.0)).strong().color(Color32::from_rgb(203, 213, 225)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{}%", self.system_volume_pct)).font(FontId::monospace(11.0)).color(Color32::from_rgb(118, 185, 0)));
                                        });
                                    });
                                    ui.add(egui::Slider::new(&mut self.system_volume_pct, 0..=200).suffix("%"));
                                    ui.add_space(4.0);
                                    let sys_active = self.audio_mode_idx == 0 || self.audio_mode_idx == 2;
                                    render_vu_meter(ui, self.system_volume_pct as f32, sys_active, self.anim_time + 1.5);
                                });

                            ui.add_space(10.0);

                            let apply_btn = egui::Button::new(egui::RichText::new("Save & Apply Audio Settings").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(10, 16, 8)))
                                .fill(Color32::from_rgb(118, 185, 0))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(150, 220, 20)))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add_sized([260.0, 34.0], apply_btn).clicked() {
                                self.config.mic_volume = self.mic_volume_pct as f32 / 100.0;
                                self.config.system_volume = self.system_volume_pct as f32 / 100.0;
                                self.config.audio_mode = match self.audio_mode_idx {
                                    1 => "mic",
                                    2 => "both",
                                    3 => "muted",
                                    _ => "system",
                                }.to_string();
                                let _ = self.config.save();
                                let _ = ipc::send_command(Command::ReloadConfig);
                                self.set_msg("Audio settings applied!");
                            }
                        });
                    }
                }
            });
    }
}

pub fn run_egui_overlay() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("vrec ShadowPlay")
            .with_app_id("vrec-overlay")
            .with_inner_size([880.0, 420.0])
            .with_min_inner_size([800.0, 380.0])
            .with_max_inner_size([1100.0, 600.0])
            .with_resizable(true)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top(),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "vrec-overlay",
        options,
        Box::new(|_cc| Ok(Box::new(VrecOverlayApp::new()))),
    );
}
