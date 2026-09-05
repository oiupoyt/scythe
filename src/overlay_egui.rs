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
        Color32::from_rgba_unmultiplied(28, 36, 48, 180)
    };
    let stroke = if active {
        Stroke::new(1.0_f32, Color32::from_rgb(150, 220, 20))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 20))
    };
    let text_color = if active {
        Color32::from_rgb(10, 16, 8)
    } else {
        Color32::from_rgb(203, 213, 225)
    };
    let btn = egui::Button::new(egui::RichText::new(text).size(11.0).strong().color(text_color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(5_u8));
    ui.add(btn).clicked()
}

// Card bottom action button
fn card_action_button(ui: &mut egui::Ui, text: &str, highlight: bool, danger: bool) -> bool {
    let fill = if danger {
        Color32::from_rgb(220, 38, 38)
    } else if highlight {
        Color32::from_rgb(118, 185, 0)
    } else {
        Color32::from_rgba_unmultiplied(32, 42, 58, 220)
    };
    let stroke = if danger {
        Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68))
    } else if highlight {
        Stroke::new(1.0_f32, Color32::from_rgb(150, 220, 20))
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28))
    };
    let text_color = if danger || !highlight {
        Color32::WHITE
    } else {
        Color32::from_rgb(10, 16, 8)
    };
    let btn = egui::Button::new(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(text_color),
    )
    .fill(fill)
    .stroke(stroke)
    .corner_radius(CornerRadius::same(7_u8))
    .min_size(Vec2::new(ui.available_width(), 32.0));

    ui.add(btn).clicked()
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
    let p1 = arrow_tip - tangent * 6.5 + normal * 4.5;
    let p2 = arrow_tip - tangent * 6.5 - normal * 4.5;
    painter.add(egui::Shape::convex_polygon(
        vec![arrow_tip, p1, p2],
        color,
        Stroke::NONE,
    ));

    // Centered play triangle
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
}

fn draw_record_icon(painter: &egui::Painter, center: egui::Pos2, radius: f32, is_recording: bool, anim_time: f32) {
    if is_recording {
        let pulse = ((anim_time * 5.0).sin() * 0.5 + 0.5) * 4.0;
        let glow_color = Color32::from_rgba_unmultiplied(239, 68, 68, 60);
        painter.circle_filled(center, radius + 4.0 + pulse, glow_color);
        painter.circle_stroke(center, radius, Stroke::new(3.0_f32, Color32::from_rgb(239, 68, 68)));
        painter.circle_filled(center, radius * 0.45, Color32::from_rgb(239, 68, 68));
    } else {
        painter.circle_stroke(center, radius, Stroke::new(2.5_f32, Color32::from_rgb(100, 116, 139)));
        painter.circle_filled(center, radius * 0.42, Color32::from_rgb(226, 232, 240));
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

pub struct VrecOverlayApp {
    config: VrecConfig,
    status: DaemonStatus,
    daemon_connected: bool,
    current_view: ShadowPlayView,
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

    pub fn switch_view(&mut self, view: ShadowPlayView, ctx: &egui::Context) {
        self.current_view = view;
        let target_h = match view {
            ShadowPlayView::MainHud => 240.0,
            ShadowPlayView::Settings => 560.0,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(780.0, target_h)));
    }

    fn render_main_hud(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let anim_time = self.anim_time;
        let is_recording = self.status.is_recording;
        let rec_dur = self.status.recording_duration_sec;
        let is_replay_active = self.status.is_replay_active;

        // Top Header bar
        ui.horizontal(|ui| {
            // VREC Green badge
            let (brand_rect, _) = ui.allocate_exact_size(Vec2::new(44.0, 22.0), egui::Sense::hover());
            ui.painter().rect_filled(brand_rect, CornerRadius::same(4_u8), Color32::from_rgb(118, 185, 0));
            ui.painter().text(
                brand_rect.center(),
                egui::Align2::CENTER_CENTER,
                "VREC",
                FontId::proportional(11.5),
                Color32::from_rgb(10, 16, 8),
            );

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("SHADOWPLAY OVERLAY")
                    .font(FontId::proportional(12.0))
                    .strong()
                    .color(Color32::from_rgb(226, 232, 240)),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Close button
                let (close_rect, close_resp) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::click());
                let close_color = if close_resp.hovered() { Color32::from_rgb(239, 68, 68) } else { Color32::from_rgb(148, 163, 184) };
                draw_close_icon(ui.painter(), close_rect.center(), 12.0, close_color);
                if close_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                ui.add_space(10.0);

                // Status message or notification banner
                if let Some((msg, ts)) = &self.status_msg {
                    if ts.elapsed() < Duration::from_secs(3) {
                        ui.label(
                            egui::RichText::new(msg)
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(118, 185, 0)),
                        );
                    }
                } else if is_recording {
                    let pulse = ((anim_time * 5.0).sin() * 0.5 + 0.5) * 2.0;
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.0 + pulse, Color32::from_rgba_unmultiplied(239, 68, 68, 80));
                    ui.painter().circle_filled(dot_rect.center(), 3.5, Color32::from_rgb(239, 68, 68));
                    let mins = rec_dur / 60;
                    let secs = rec_dur % 60;
                    ui.label(
                        egui::RichText::new(format!("REC {:02}:{:02}", mins, secs))
                            .size(11.5)
                            .strong()
                            .color(Color32::from_rgb(239, 68, 68)),
                    );
                } else {
                    let status_text = if self.daemon_connected { "READY" } else { "CONNECTING..." };
                    let status_color = if self.daemon_connected { Color32::from_rgb(74, 222, 128) } else { Color32::from_rgb(100, 116, 139) };
                    ui.label(
                        egui::RichText::new(status_text)
                            .size(10.0)
                            .strong()
                            .color(status_color),
                    );
                }
            });
        });

        ui.add_space(10.0);

        // 3 Transparent Cards
        let total_width = ui.available_width();
        let card_gap = 12.0;
        let card_w = ((total_width - 2.0 * card_gap) / 3.0).floor();
        let card_h = 165.0;

        ui.horizontal(|ui| {
            // CARD 1: INSTANT REPLAY
            let replay_border = if is_replay_active {
                Color32::from_rgba_unmultiplied(118, 185, 0, 120)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 22)
            };
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(18, 24, 34, 185))
                .stroke(Stroke::new(1.0_f32, replay_border))
                .corner_radius(CornerRadius::same(12_u8))
                .inner_margin(Margin::symmetric(14_i8, 12_i8))
                .show(ui, |ui| {
                    ui.set_width(card_w - 28.0);
                    ui.set_height(card_h - 24.0);

                    // Top row
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("INSTANT REPLAY").size(11.5).strong().color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (txt, col) = if is_replay_active { ("ACTIVE", Color32::from_rgb(118, 185, 0)) } else { ("OFF", Color32::from_rgb(148, 163, 184)) };
                            ui.label(egui::RichText::new(txt).size(10.0).strong().color(col));
                        });
                    });

                    ui.add_space(6.0);

                    // Icon & subtitle
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::hover());
                    let replay_color = if is_replay_active { Color32::from_rgb(118, 185, 0) } else { Color32::from_rgb(148, 163, 184) };
                    draw_replay_icon(ui.painter(), icon_rect.center(), 16.0, replay_color);

                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(format!("{}s Rolling Buffer", self.replay_sec)).size(10.5).color(Color32::from_rgb(148, 163, 184)));
                    });

                    ui.add_space(8.0);

                    // Action button
                    if card_action_button(ui, "Save Replay", true, false) {
                        let _ = ipc::send_command(Command::SaveReplay);
                        self.status_msg = Some(("Replay Saved!".to_string(), Instant::now()));
                    }
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Ctrl + Shift + R").size(9.0).color(Color32::from_rgb(100, 116, 139)));
                    });
                });

            ui.add_space(card_gap);

            // CARD 2: RECORD
            let record_border = if is_recording {
                Color32::from_rgba_unmultiplied(239, 68, 68, 180)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 22)
            };
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(18, 24, 34, 185))
                .stroke(Stroke::new(1.0_f32, record_border))
                .corner_radius(CornerRadius::same(12_u8))
                .inner_margin(Margin::symmetric(14_i8, 12_i8))
                .show(ui, |ui| {
                    ui.set_width(card_w - 28.0);
                    ui.set_height(card_h - 24.0);

                    // Top row
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("RECORD").size(11.5).strong().color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (txt, col) = if is_recording { ("RECORDING", Color32::from_rgb(239, 68, 68)) } else { ("READY", Color32::from_rgb(148, 163, 184)) };
                            ui.label(egui::RichText::new(txt).size(10.0).strong().color(col));
                        });
                    });

                    ui.add_space(6.0);

                    // Icon & subtitle
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::hover());
                    draw_record_icon(ui.painter(), icon_rect.center(), 16.0, is_recording, anim_time);

                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(format!("{} FPS • {} Mbps", self.target_fps, self.bitrate_mbps)).size(10.5).color(Color32::from_rgb(148, 163, 184)));
                    });

                    ui.add_space(8.0);

                    // Action button
                    if is_recording {
                        if card_action_button(ui, "Stop Recording", false, true) {
                            let _ = ipc::send_command(Command::StopRecording);
                            self.status_msg = Some(("Recording Saved!".to_string(), Instant::now()));
                        }
                    } else {
                        if card_action_button(ui, "Start Recording", false, false) {
                            let _ = ipc::send_command(Command::StartRecording);
                            self.status_msg = Some(("Recording Started".to_string(), Instant::now()));
                        }
                    }
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Ctrl + Shift + F9").size(9.0).color(Color32::from_rgb(100, 116, 139)));
                    });
                });

            ui.add_space(card_gap);

            // CARD 3: SETTINGS
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(18, 24, 34, 185))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 22)))
                .corner_radius(CornerRadius::same(12_u8))
                .inner_margin(Margin::symmetric(14_i8, 12_i8))
                .show(ui, |ui| {
                    ui.set_width(card_w - 28.0);
                    ui.set_height(card_h - 24.0);

                    // Top row
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("SETTINGS").size(11.5).strong().color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("CONFIG").size(10.0).strong().color(Color32::from_rgb(96, 165, 250)));
                        });
                    });

                    ui.add_space(6.0);

                    // Icon & subtitle
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::hover());
                    draw_gear_icon(ui.painter(), icon_rect.center(), 16.0, Color32::from_rgb(203, 213, 225));

                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Cursor, Audio, Video & Storage").size(10.5).color(Color32::from_rgb(148, 163, 184)));
                    });

                    ui.add_space(8.0);

                    // Action button
                    if card_action_button(ui, "Open Settings", false, false) {
                        self.switch_view(ShadowPlayView::Settings, ctx);
                    }
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("All Preferences").size(9.0).color(Color32::from_rgb(100, 116, 139)));
                    });
                });
        });
    }

    fn render_settings_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // Header with Back button
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new("< Back to Overlay").size(12.0).strong().color(Color32::from_rgb(118, 185, 0))).clicked() {
                self.switch_view(ShadowPlayView::MainHud, ctx);
                return;
            }

            ui.add_space(16.0);
            ui.label(egui::RichText::new("VREC SETTINGS").font(FontId::proportional(13.0)).strong().color(Color32::WHITE));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (close_rect, close_resp) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::click());
                let close_color = if close_resp.hovered() { Color32::from_rgb(239, 68, 68) } else { Color32::from_rgb(148, 163, 184) };
                draw_close_icon(ui.painter(), close_rect.center(), 12.0, close_color);
                if close_resp.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // CATEGORY 1: DISPLAY & MOUSE CURSOR
                ui.label(egui::RichText::new("DISPLAY & CAPTURE").size(11.5).strong().color(Color32::from_rgb(118, 185, 0)));
                ui.add_space(4.0);

                // MOUSE CURSOR TOGGLE
                let mut show_cur = self.show_cursor;
                if ui.checkbox(&mut show_cur, egui::RichText::new("Record Mouse Cursor").size(12.0).strong().color(Color32::WHITE)).changed() {
                    self.show_cursor = show_cur;
                    self.config.show_cursor = show_cur;
                    let _ = self.config.save();
                    let _ = ipc::send_command(Command::ToggleCursor);
                    self.status_msg = Some((format!("Cursor {}", if show_cur { "Visible" } else { "Hidden" }), Instant::now()));
                }
                ui.label(egui::RichText::new("Include the mouse pointer in recordings and instant replays.").size(10.5).color(Color32::from_rgb(148, 163, 184)));

                ui.add_space(10.0);

                // Framerate
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Target Framerate:").size(11.0).strong().color(Color32::WHITE));
                    for fps in [30, 60, 120, 144] {
                        if pill_button(ui, &format!("{} FPS", fps), self.target_fps == fps) {
                            self.target_fps = fps;
                            self.config.fps = fps;
                            let _ = self.config.save();
                            let _ = ipc::send_command(Command::ReloadConfig);
                        }
                    }
                });

                ui.add_space(8.0);

                // Bitrate Slider
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Video Bitrate:").size(11.0).strong().color(Color32::WHITE));
                    let mut br = self.bitrate_mbps;
                    if ui.add(egui::Slider::new(&mut br, 5..=100).text("Mbps").step_by(5.0)).changed() {
                        self.bitrate_mbps = br;
                        self.config.record_bitrate_kbps = br * 1000;
                        self.config.replay_bitrate_kbps = br * 1000;
                        let _ = self.config.save();
                        let _ = ipc::send_command(Command::ReloadConfig);
                    }
                });

                ui.add_space(8.0);

                // Video Codec
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Video Codec:").size(11.0).strong().color(Color32::WHITE));
                    for (codec_key, label) in [("h264", "H.264 (NVENC/VAAPI)"), ("hevc", "HEVC (H.265)"), ("av1", "AV1")] {
                        if pill_button(ui, label, self.video_codec == codec_key) {
                            self.video_codec = codec_key.to_string();
                            self.config.video_codec = codec_key.to_string();
                            let _ = self.config.save();
                            let _ = ipc::send_command(Command::ReloadConfig);
                        }
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);

                // CATEGORY 2: INSTANT REPLAY BUFFER
                ui.label(egui::RichText::new("INSTANT REPLAY BUFFER").size(11.5).strong().color(Color32::from_rgb(118, 185, 0)));
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Buffer Length:").size(11.0).strong().color(Color32::WHITE));
                    for sec in [15, 30, 60, 120, 300] {
                        let label = if sec >= 60 { format!("{}m", sec / 60) } else { format!("{}s", sec) };
                        if pill_button(ui, &label, self.replay_sec == sec) {
                            self.replay_sec = sec;
                            self.config.replay_duration_sec = sec;
                            let _ = self.config.save();
                            let _ = ipc::send_command(Command::ReloadConfig);
                        }
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);

                // CATEGORY 3: AUDIO & SOUND
                ui.label(egui::RichText::new("AUDIO & SOUND ROUTING").size(11.5).strong().color(Color32::from_rgb(118, 185, 0)));
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Audio Source:").size(11.0).strong().color(Color32::WHITE));
                    let modes = [("system", "System Audio"), ("mic", "Microphone"), ("both", "Both Combined"), ("muted", "Muted")];
                    for (idx, (m_id, m_label)) in modes.iter().enumerate() {
                        if pill_button(ui, m_label, self.audio_mode_idx == idx) {
                            self.audio_mode_idx = idx;
                            self.config.audio_mode = m_id.to_string();
                            let _ = self.config.save();
                            let _ = ipc::send_command(Command::ReloadConfig);
                        }
                    }
                });

                ui.add_space(8.0);

                // System Volume
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("System Volume:").size(11.0).strong().color(Color32::WHITE));
                    let mut sys_v = self.system_volume_pct;
                    if ui.add(egui::Slider::new(&mut sys_v, 0..=150).suffix("%")).changed() {
                        self.system_volume_pct = sys_v;
                        self.config.system_volume = sys_v as f32 / 100.0;
                        let _ = self.config.save();
                        let _ = ipc::send_command(Command::ReloadConfig);
                    }
                });

                ui.add_space(6.0);

                // Mic Volume
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Microphone Volume:").size(11.0).strong().color(Color32::WHITE));
                    let mut mic_v = self.mic_volume_pct;
                    if ui.add(egui::Slider::new(&mut mic_v, 0..=150).suffix("%")).changed() {
                        self.mic_volume_pct = mic_v;
                        self.config.mic_volume = mic_v as f32 / 100.0;
                        let _ = self.config.save();
                        let _ = ipc::send_command(Command::ReloadConfig);
                    }
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);

                // CATEGORY 4: STORAGE & OUTPUT
                ui.label(egui::RichText::new("STORAGE & RECORDINGS").size(11.5).strong().color(Color32::from_rgb(118, 185, 0)));
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Directory:").size(11.0).strong().color(Color32::WHITE));
                    let mut dir_str = self.output_dir.clone();
                    if ui.add(egui::TextEdit::singleline(&mut dir_str).desired_width(280.0)).changed() {
                        self.output_dir = dir_str.clone();
                        self.config.output_directory = dir_str;
                        let _ = self.config.save();
                    }

                    if ui.button(egui::RichText::new("Browse...").size(11.0)).clicked() {
                        pick_folder(&self.output_dir, self.folder_tx.clone());
                    }

                    if ui.button(egui::RichText::new("Open Folder").size(11.0)).clicked() {
                        open_folder(&VrecConfig::expand_tilde(&self.output_dir));
                    }
                });

                ui.label(egui::RichText::new(format!("{} video recordings found in destination folder.", self.clips.len())).size(10.5).color(Color32::from_rgb(148, 163, 184)));

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(6.0);

                // CATEGORY 5: KEYBOARD SHORTCUTS
                ui.label(egui::RichText::new("GLOBAL SHORTCUTS").size(11.5).strong().color(Color32::from_rgb(118, 185, 0)));
                ui.add_space(6.0);

                let shortcuts = [
                    ("Alt + Z", "Open / Close Overlay Dock"),
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

                ui.add_space(10.0);
            });
    }
}

impl eframe::App for VrecOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async_events();
        self.anim_time += 0.033;
        ctx.request_repaint_after(Duration::from_millis(50));

        // Position window a little at the top like an overlay!
        if !self.initial_pos_set
            && let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size)
            && monitor_size.x > 100.0
            && monitor_size.y > 100.0
        {
            let win_w = 780.0;
            let x = ((monitor_size.x - win_w) / 2.0).max(10.0);
            let y = 45.0; // a little at the top like an overlay!
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
            self.initial_pos_set = true;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.current_view != ShadowPlayView::MainHud {
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
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(12, 16, 24, 230))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 22)))
                    .corner_radius(CornerRadius::same(14_u8))
                    .inner_margin(Margin::same(14_i8)),
            )
            .show(ctx, |ui| match self.current_view {
                ShadowPlayView::MainHud => self.render_main_hud(ctx, ui),
                ShadowPlayView::Settings => self.render_settings_view(ctx, ui),
            });
    }
}

pub fn run_egui_overlay() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("vrec ShadowPlay")
            .with_app_id("vrec-overlay")
            .with_position([400.0, 45.0])
            .with_inner_size([780.0, 240.0])
            .with_min_inner_size([740.0, 220.0])
            .with_max_inner_size([1100.0, 800.0])
            .with_resizable(false)
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
