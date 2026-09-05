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

// Pill button selector
fn pill_button(ui: &mut egui::Ui, text: &str, active: bool) -> bool {
    let fill = if active {
        Color32::from_rgb(118, 185, 0)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 14)
    };
    let stroke = if active {
        Stroke::new(1.0_f32, Color32::from_rgb(140, 224, 0))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28))
    };
    let text_color = if active {
        Color32::from_rgb(11, 18, 4)
    } else {
        Color32::from_rgb(203, 213, 225)
    };
    let btn = egui::Button::new(egui::RichText::new(text).size(11.0).strong().color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6_u8));
    ui.add(btn).clicked()
}

// Sleek Switch Toggle (iOS / Modern ShadowPlay Style)
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired_size = egui::vec2(42.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, *on);
        let bg_color = if *on {
            Color32::from_rgb(118, 185, 0)
        } else {
            Color32::from_rgb(30, 40, 56)
        };
        let stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 50));
        let radius = 0.5 * rect.height();
        ui.painter().rect(rect, CornerRadius::same(radius as u8), bg_color, stroke, egui::StrokeKind::Inside);
        let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let center = egui::pos2(circle_x, rect.center().y);
        ui.painter().circle_filled(center, radius - 2.5, Color32::WHITE);
    }
    response
}

// Geometric Centered Vector Icon Renderers
fn draw_replay_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_active: bool) {
    use std::f32::consts::PI;
    let color = if is_active {
        Color32::from_rgb(118, 185, 0)
    } else {
        Color32::from_rgb(148, 163, 184)
    };
    let stroke = Stroke::new(3.8_f32, color);
    let start_angle = 0.25 * PI;
    let end_angle = 1.80 * PI;
    let steps = 32;
    let mut points = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = start_angle + t * (end_angle - start_angle);
        points.push(center + Vec2::new(angle.cos() * radius, angle.sin() * radius));
    }
    for w in points.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }

    // Arrowhead at start of arc
    let arrow_tip = points[0];
    let tangent = Vec2::new(-start_angle.sin(), start_angle.cos());
    let normal = Vec2::new(start_angle.cos(), start_angle.sin());
    let p1 = arrow_tip - tangent * 8.5 + normal * 5.5;
    let p2 = arrow_tip - tangent * 8.5 - normal * 5.5;
    painter.add(egui::Shape::convex_polygon(
        vec![arrow_tip, p1, p2],
        color,
        Stroke::NONE,
    ));

    // Centered play triangle
    let tri_r = radius * 0.36;
    let tri_center = center + Vec2::new(1.0, 0.0);
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
        let glow_color = Color32::from_rgba_unmultiplied(239, 68, 68, 60);
        painter.circle_filled(center, radius + 6.0 + pulse, glow_color);
        painter.circle_stroke(center, radius + 2.0, Stroke::new(3.5_f32, Color32::from_rgb(239, 68, 68)));
        painter.circle_filled(center, radius * 0.50, Color32::from_rgb(239, 68, 68));
    } else {
        painter.circle_stroke(center, radius + 1.0, Stroke::new(3.2_f32, Color32::from_rgb(148, 163, 184)));
        painter.circle_filled(center, radius * 0.48, Color32::from_rgb(226, 232, 240));
    }
}

fn draw_gear_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    let stroke = Stroke::new(3.0_f32, color);
    painter.circle_stroke(center, radius * 0.75, stroke);
    painter.circle_filled(center, radius * 0.32, Color32::from_rgb(18, 20, 24));
    painter.circle_stroke(center, radius * 0.32, stroke);

    use std::f32::consts::PI;
    for i in 0..8 {
        let angle = i as f32 * (PI / 4.0);
        let p_in = center + Vec2::new(angle.cos() * (radius * 0.68), angle.sin() * (radius * 0.68));
        let p_out = center + Vec2::new(angle.cos() * (radius * 1.08), angle.sin() * (radius * 1.08));
        painter.line_segment([p_in, p_out], Stroke::new(4.2_f32, color));
    }
}

// Frosted Glass Action Card Renderer (Matches GTK overlay exact layout)
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
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let hovered = response.hovered();

    let bg = if dropdown_open {
        Color32::from_rgba_unmultiplied(26, 38, 56, 230)
    } else if hovered {
        Color32::from_rgba_unmultiplied(26, 38, 56, 210)
    } else {
        Color32::from_rgba_unmultiplied(20, 28, 42, 175)
    };

    let border = if dropdown_open || hovered {
        Color32::from_rgb(118, 185, 0)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 38)
    };

    let corner_radius = if dropdown_open {
        CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }
    } else {
        CornerRadius::same(14_u8)
    };

    let painter = ui.painter();
    // Drop shadow
    painter.rect_filled(
        rect.translate(Vec2::new(0.0, 8.0)),
        corner_radius,
        Color32::from_rgba_unmultiplied(0, 0, 0, 90),
    );
    // Card background
    painter.rect(rect, corner_radius, bg, Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);

    // Top specular highlight line
    let p_left = egui::pos2(rect.left() + 10.0, rect.top() + 1.0);
    let p_right = egui::pos2(rect.right() - 10.0, rect.top() + 1.0);
    painter.line_segment([p_left, p_right], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 45)));

    // Card Title
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 26.0),
        egui::Align2::CENTER_CENTER,
        title,
        FontId::proportional(11.5),
        if is_active { Color32::from_rgb(118, 185, 0) } else { Color32::WHITE },
    );

    // Centered Vector Icon
    let icon_center = egui::pos2(rect.center().x, rect.top() + 94.0);
    draw_icon(painter, icon_center);

    // Status label
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 150.0),
        egui::Align2::CENTER_CENTER,
        status_text,
        FontId::proportional(11.5),
        status_color,
    );

    // Subtitle label
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 170.0),
        egui::Align2::CENTER_CENTER,
        sub_text,
        FontId::proportional(10.0),
        Color32::from_rgb(100, 116, 139),
    );

    response.clicked()
}

// Attached Dropdown Menu Container
fn render_dropdown_menu(
    ui: &mut egui::Ui,
    width: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(16, 22, 34, 230))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 38)))
        .corner_radius(CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 })
        .inner_margin(Margin::same(8_i8))
        .show(ui, |ui| {
            ui.set_width(width - 16.0);
            add_contents(ui);
        });
}

// Sleek Dropdown Action Menu Item
fn render_menu_item(ui: &mut egui::Ui, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), egui::Sense::click());
    let hovered = response.hovered();

    let bg = if hovered {
        Color32::from_rgba_unmultiplied(118, 185, 0, 38)
    } else {
        Color32::TRANSPARENT
    };
    let border = if hovered {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(118, 185, 0, 120))
    } else {
        Stroke::NONE
    };
    let text_color = if hovered {
        Color32::from_rgb(118, 185, 0)
    } else {
        Color32::from_rgb(226, 232, 240)
    };

    ui.painter().rect(rect, CornerRadius::same(6_u8), bg, border, egui::StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.5),
        text_color,
    );

    response.clicked()
}

// Section card helper for Settings view
fn render_section_card(ui: &mut egui::Ui, header: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(24, 32, 46, 140))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 20)))
        .corner_radius(CornerRadius::same(12_u8))
        .inner_margin(Margin::symmetric(16_i8, 12_i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(header)
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(118, 185, 0)),
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
    anim_time: f32,
    status_msg: Option<(String, Instant)>,
    status_rx: Receiver<DaemonStatus>,
    folder_tx: Sender<String>,
    folder_rx: Receiver<String>,
    clips: Vec<VideoClipInfo>,
    initial_pos_set: bool,
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
            anim_time: 0.0,
            status_msg: None,
            status_rx,
            folder_tx,
            folder_rx,
            clips,
            initial_pos_set: false,
        }
    }

    pub fn refresh_clips(&mut self) {
        self.clips = scan_recordings(&self.output_dir);
    }

    fn poll_async_events(&mut self) {
        while let Ok(s) = self.status_rx.try_recv() {
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

    pub fn update_window_size(&self, ctx: &egui::Context) {
        if self.current_view == ShadowPlayView::MainHud {
            let target_h = if self.replay_dropdown_open || self.record_dropdown_open {
                385.0
            } else {
                270.0
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(760.0, target_h)));
        }
    }

    pub fn switch_view(&mut self, view: ShadowPlayView, ctx: &egui::Context) {
        self.current_view = view;
        let target_h = match view {
            ShadowPlayView::MainHud => {
                if self.replay_dropdown_open || self.record_dropdown_open { 385.0 } else { 270.0 }
            }
            ShadowPlayView::Settings => 580.0,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(760.0, target_h)));
    }

    fn render_main_hud(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let anim_time = self.anim_time;
        let is_recording = self.status.is_recording;
        let rec_dur = self.status.recording_duration_sec;
        let is_replay_active = self.status.is_replay_active;

        ui.vertical_centered(|ui| {
            // TOP FLOATING HEADER BAR
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(18, 26, 38, 185))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 38)))
                .corner_radius(CornerRadius::same(12_u8))
                .inner_margin(Margin::symmetric(18_i8, 8_i8))
                .show(ui, |ui| {
                    ui.set_width(730.0);
                    ui.horizontal(|ui| {
                        // SCYTHE Badge
                        let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(56.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(badge_rect, CornerRadius::same(4_u8), Color32::from_rgb(118, 185, 0));
                        ui.painter().text(
                            badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "SCYTHE",
                            FontId::proportional(11.0),
                            Color32::from_rgb(11, 18, 4),
                        );

                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("SHADOWPLAY OVERLAY")
                                .font(FontId::proportional(12.0))
                                .strong()
                                .color(Color32::from_rgb(241, 245, 249)),
                        );

                        ui.add_space(12.0);
                        if let Some((msg, ts)) = &self.status_msg {
                            if ts.elapsed() < Duration::from_secs(3) {
                                ui.label(
                                    egui::RichText::new(msg)
                                        .size(11.5)
                                        .strong()
                                        .color(Color32::from_rgb(118, 185, 0)),
                                );
                            }
                        } else if is_recording {
                            let pulse = ((anim_time * 5.0).sin() * 0.5 + 0.5) * 2.5;
                            let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot_rect.center(), 4.0 + pulse, Color32::from_rgba_unmultiplied(239, 68, 68, 90));
                            ui.painter().circle_filled(dot_rect.center(), 3.5, Color32::from_rgb(239, 68, 68));
                            let mins = rec_dur / 60;
                            let secs = rec_dur % 60;
                            ui.label(
                                egui::RichText::new(format!("RECORDING {:02}:{:02}", mins, secs))
                                    .size(11.5)
                                    .strong()
                                    .color(Color32::from_rgb(239, 68, 68)),
                            );
                        }

                        // Right side Esc hint badge (No close X button)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::NONE
                                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 20))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 35)))
                                .corner_radius(CornerRadius::same(6_u8))
                                .inner_margin(Margin::symmetric(8_i8, 3_i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("Esc to Close")
                                            .size(10.5)
                                            .strong()
                                            .color(Color32::from_rgb(148, 163, 184)),
                                    );
                                });
                        });
                    });
                });

            ui.add_space(10.0);

            // ROW OF 3 CARDS
            let card_w = 230.0;
            let card_h = 195.0;
            let card_gap = 16.0;

            ui.horizontal(|ui| {
                let left_pad = ((ui.available_width() - (3.0 * card_w + 2.0 * card_gap)) / 2.0).max(0.0);
                ui.add_space(left_pad);

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
                            draw_replay_icon(painter, center, 24.0, is_replay_active);
                        },
                        if is_replay_active { "Turned on" } else { "Turned off" },
                        if is_replay_active { Color32::from_rgb(118, 185, 0) } else { Color32::from_rgb(148, 163, 184) },
                        "(Alt+Shift+F10 to toggle)",
                    );

                    if card1_clicked {
                        self.replay_dropdown_open = !self.replay_dropdown_open;
                        self.record_dropdown_open = false;
                        self.update_window_size(ctx);
                    }

                    if self.replay_dropdown_open {
                        render_dropdown_menu(ui, card_w, |ui| {
                            let toggle_text = if is_replay_active { "Turn off" } else { "Turn on" };
                            if render_menu_item(ui, toggle_text) {
                                let mut cfg = ScytheConfig::load();
                                cfg.replay_enabled = !cfg.replay_enabled;
                                let _ = cfg.save();
                                ScytheConfig::notify_daemon_reload();
                                self.config.replay_enabled = cfg.replay_enabled;
                                self.status.is_replay_active = cfg.replay_enabled;
                                self.replay_dropdown_open = false;
                                self.update_window_size(ctx);
                            }
                            if render_menu_item(ui, "Save") {
                                let _ = ipc::send_command(Command::SaveReplay);
                                self.status_msg = Some(("Replay Saved!".to_string(), Instant::now()));
                                self.replay_dropdown_open = false;
                                self.update_window_size(ctx);
                            }
                            if render_menu_item(ui, "Settings") {
                                self.replay_dropdown_open = false;
                                self.switch_view(ShadowPlayView::Settings, ctx);
                            }
                        });
                    }
                });

                ui.add_space(card_gap);

                // =========================================================================
                // CARD 2: RECORD
                // =========================================================================
                ui.vertical(|ui| {
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
                        if is_recording { "Recording..." } else { "Idle" },
                        if is_recording { Color32::from_rgb(239, 68, 68) } else { Color32::from_rgb(148, 163, 184) },
                        "(Alt+F9 to record)",
                    );

                    if card2_clicked {
                        self.record_dropdown_open = !self.record_dropdown_open;
                        self.replay_dropdown_open = false;
                        self.update_window_size(ctx);
                    }

                    if self.record_dropdown_open {
                        render_dropdown_menu(ui, card_w, |ui| {
                            let rec_toggle_text = if is_recording { "Stop" } else { "Start" };
                            if render_menu_item(ui, rec_toggle_text) {
                                let _ = ipc::send_command(Command::ToggleRecording);
                                self.record_dropdown_open = false;
                                self.update_window_size(ctx);
                            }
                            if render_menu_item(ui, "Settings") {
                                self.record_dropdown_open = false;
                                self.switch_view(ShadowPlayView::Settings, ctx);
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
                        "(Click to configure)",
                    );

                    if card3_clicked {
                        self.replay_dropdown_open = false;
                        self.record_dropdown_open = false;
                        self.switch_view(ShadowPlayView::Settings, ctx);
                    }
                });
            });
        });
    }

    fn render_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(15, 22, 32, 230))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 40)))
                .corner_radius(CornerRadius::same(16_u8))
                .inner_margin(Margin::symmetric(22_i8, 16_i8))
                .show(ui, |ui| {
                    ui.set_width(720.0);

                    // Header with Back Button (No X button!)
                    ui.horizontal(|ui| {
                        let back_btn = egui::Button::new(
                            egui::RichText::new("< Back to Overlay")
                                .size(11.5)
                                .strong()
                                .color(Color32::from_rgb(118, 185, 0)),
                        )
                        .fill(Color32::from_rgba_unmultiplied(118, 185, 0, 30))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(118, 185, 0, 115)))
                        .corner_radius(CornerRadius::same(8_u8));

                        if ui.add(back_btn).clicked() {
                            self.switch_view(ShadowPlayView::MainHud, ctx);
                            return;
                        }

                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("RECORDER SETTINGS")
                                .font(FontId::proportional(13.5))
                                .strong()
                                .color(Color32::WHITE),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::NONE
                                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 20))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 35)))
                                .corner_radius(CornerRadius::same(6_u8))
                                .inner_margin(Margin::symmetric(8_i8, 3_i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("Esc to Close")
                                            .size(10.5)
                                            .strong()
                                            .color(Color32::from_rgb(148, 163, 184)),
                                    );
                                });
                        });
                    });

                    ui.add_space(10.0);

                    egui::ScrollArea::vertical()
                        .max_height(450.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // -----------------------------------------------------------------
                            // SECTION 1: DISPLAY & CAPTURE
                            // -----------------------------------------------------------------
                            render_section_card(ui, "DISPLAY & CAPTURE", |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new("Record Mouse Cursor").size(13.0).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("Capture the mouse pointer in gameplay recordings and instant replays").size(10.5).color(Color32::from_rgb(148, 163, 184)));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut cur = self.show_cursor;
                                        if toggle_switch(ui, &mut cur).changed() {
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

                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Target Framerate:").size(11.5).strong().color(Color32::WHITE));
                                    for fps in [30, 60, 120, 144] {
                                        if pill_button(ui, &format!("{} FPS", fps), self.target_fps == fps) {
                                            self.target_fps = fps;
                                        }
                                    }
                                });

                                ui.add_space(8.0);

                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Video Bitrate:").size(11.5).strong().color(Color32::WHITE));
                                    for mbps in [10, 20, 30, 50] {
                                        if pill_button(ui, &format!("{}M", mbps), self.bitrate_mbps == mbps) {
                                            self.bitrate_mbps = mbps;
                                        }
                                    }
                                    ui.add_space(6.0);
                                    let mut br = self.bitrate_mbps;
                                    if ui.add(egui::Slider::new(&mut br, 5..=100).text("Mbps").step_by(5.0)).changed() {
                                        self.bitrate_mbps = br;
                                    }
                                });

                                ui.add_space(8.0);

                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Encoder Codec:").size(11.5).strong().color(Color32::WHITE));
                                    for (codec_key, label) in [("h264", "H.264 / AVC"), ("hevc", "HEVC / H.265"), ("av1", "AV1")] {
                                        if pill_button(ui, label, self.video_codec == codec_key) {
                                            self.video_codec = codec_key.to_string();
                                        }
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 2: INSTANT REPLAY BUFFER
                            // -----------------------------------------------------------------
                            render_section_card(ui, "INSTANT REPLAY BUFFER", |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Buffer Duration:").size(11.5).strong().color(Color32::WHITE));
                                    for sec in [15, 30, 60, 120, 300] {
                                        let label = if sec >= 60 { format!("{}m", sec / 60) } else { format!("{}s", sec) };
                                        if pill_button(ui, &label, self.replay_sec == sec) {
                                            self.replay_sec = sec;
                                        }
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 3: AUDIO CONFIGURATION
                            // -----------------------------------------------------------------
                            render_section_card(ui, "AUDIO CONFIGURATION", |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Audio Source:").size(11.5).strong().color(Color32::WHITE));
                                    let modes = [("system", "System Audio"), ("mic", "Microphone"), ("both", "Combined Both"), ("muted", "Muted")];
                                    for (idx, (_, m_label)) in modes.iter().enumerate() {
                                        if pill_button(ui, m_label, self.audio_mode_idx == idx) {
                                            self.audio_mode_idx = idx;
                                        }
                                    }
                                });

                                ui.add_space(8.0);

                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Mic Volume:").size(11.5).strong().color(Color32::WHITE));
                                    let mut mv = self.mic_volume_pct;
                                    if ui.add(egui::Slider::new(&mut mv, 0..=200).suffix("%")).changed() {
                                        self.mic_volume_pct = mv;
                                    }
                                });

                                ui.add_space(6.0);

                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("System Audio:").size(11.5).strong().color(Color32::WHITE));
                                    let mut sv = self.system_volume_pct;
                                    if ui.add(egui::Slider::new(&mut sv, 0..=200).suffix("%")).changed() {
                                        self.system_volume_pct = sv;
                                    }
                                });
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 4: STORAGE & DESTINATION
                            // -----------------------------------------------------------------
                            render_section_card(ui, "STORAGE & DESTINATION", |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Save Location:").size(11.5).strong().color(Color32::WHITE));
                                    ui.text_edit_singleline(&mut self.output_dir);
                                    if ui.button(egui::RichText::new("Change").size(11.0)).clicked() {
                                        pick_folder(&self.output_dir, self.folder_tx.clone());
                                    }
                                    if ui.button(egui::RichText::new("Open").size(11.0)).clicked() {
                                        open_folder(&ScytheConfig::expand_tilde(&self.output_dir));
                                    }
                                });
                                ui.label(egui::RichText::new(format!("{} video recordings in destination folder.", self.clips.len())).size(10.5).color(Color32::from_rgb(148, 163, 184)));
                            });

                            ui.add_space(10.0);

                            // -----------------------------------------------------------------
                            // SECTION 5: KEYBOARD SHORTCUTS
                            // -----------------------------------------------------------------
                            render_section_card(ui, "GLOBAL SHORTCUTS", |ui| {
                                let shortcuts = [
                                    ("Alt + Z", "Open / Close Overlay"),
                                    ("Ctrl + Shift + R", "Save Instant Replay Clip"),
                                    ("Ctrl + Shift + F9", "Start / Stop Recording"),
                                    ("Ctrl + Shift + F10", "Toggle Mouse Cursor In Recordings"),
                                ];
                                for (keys, action) in shortcuts {
                                    ui.horizontal(|ui| {
                                        render_keycap(ui, keys);
                                        ui.add_space(8.0);
                                        ui.label(egui::RichText::new(action).size(11.0).color(Color32::from_rgb(203, 213, 225)));
                                    });
                                    ui.add_space(3.0);
                                }
                            });

                            ui.add_space(12.0);

                            // APPLY & SAVE BUTTON
                            let apply_btn = egui::Button::new(
                                egui::RichText::new("Apply & Save Settings")
                                    .size(13.0)
                                    .strong()
                                    .color(Color32::from_rgb(11, 18, 4)),
                            )
                            .fill(Color32::from_rgb(118, 185, 0))
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(8_u8))
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
                                let _ = ipc::send_command(Command::ReloadConfig);
                                self.status_msg = Some(("Settings Saved & Applied!".to_string(), Instant::now()));
                                self.switch_view(ShadowPlayView::MainHud, ctx);
                            }
                        });
                });
        });
    }
}

impl eframe::App for ScytheOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async_events();
        self.anim_time += 0.033;
        ctx.request_repaint_after(Duration::from_millis(50));

        // Position window a little at the top (50px margin) like an overlay
        if !self.initial_pos_set
            && let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size)
            && monitor_size.x > 100.0
            && monitor_size.y > 100.0
        {
            let win_w = 760.0;
            let x = ((monitor_size.x - win_w) / 2.0).max(10.0);
            let y = 50.0;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
            self.initial_pos_set = true;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.replay_dropdown_open || self.record_dropdown_open {
                self.replay_dropdown_open = false;
                self.record_dropdown_open = false;
                self.update_window_size(ctx);
            } else if self.current_view != ShadowPlayView::MainHud {
                self.switch_view(ShadowPlayView::MainHud, ctx);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::TRANSPARENT;
        visuals.window_fill = Color32::TRANSPARENT;
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
            .show(ctx, |ui| match self.current_view {
                ShadowPlayView::MainHud => self.render_main_hud(ctx, ui),
                ShadowPlayView::Settings => self.render_settings_view(ctx, ui),
            });
    }
}

pub fn run_egui_overlay() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Scythe")
            .with_app_id("scythe-overlay")
            .with_position([400.0, 50.0])
            .with_inner_size([760.0, 270.0])
            .with_min_inner_size([740.0, 250.0])
            .with_max_inner_size([1100.0, 800.0])
            .with_resizable(false)
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
}
