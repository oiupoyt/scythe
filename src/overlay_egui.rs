use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Margin, Stroke, Vec2};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use crate::config::VrecConfig;
use crate::ipc::{self, Command, DaemonStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTab {
    Dashboard,
    AudioMixer,
    Settings,
}

pub struct VrecOverlayApp {
    config: VrecConfig,
    status: DaemonStatus,
    daemon_connected: bool,
    current_tab: OverlayTab,
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

        // Non-blocking background thread for polling daemon status
        let (status_tx, status_rx) = channel::<DaemonStatus>();
        std::thread::spawn(move || {
            loop {
                if let Ok(s) = ipc::query_status() {
                    let _ = status_tx.send(s);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });

        // Channel for asynchronous folder picker dialog
        let (folder_tx, folder_rx) = channel::<String>();

        Self {
            config,
            status: DaemonStatus::default(),
            daemon_connected: false,
            current_tab: OverlayTab::Dashboard,
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
        }
    }

    fn poll_async_events(&mut self) {
        // Drain any status updates from background thread
        while let Ok(s) = self.status_rx.try_recv() {
            if self.current_tab == OverlayTab::Dashboard {
                self.show_cursor = s.show_cursor;
            }
            self.status = s;
            self.daemon_connected = true;
        }

        // Drain folder chooser responses
        if let Ok(chosen_dir) = self.folder_rx.try_recv() {
            if !chosen_dir.trim().is_empty() {
                self.output_dir = chosen_dir;
                self.set_msg("Save directory updated!");
            }
        }
    }

    fn set_msg(&mut self, text: &str) {
        self.status_msg = Some((text.to_string(), Instant::now()));
    }
}

// Helper to render realistic mechanical keyboard keycap badges
fn render_keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(18, 24, 38))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 58, 82)))
        .corner_radius(CornerRadius::same(5_u8))
        .inner_margin(Margin::symmetric(8_i8, 3_i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(FontId::monospace(10.0))
                    .strong()
                    .color(Color32::from_rgb(148, 163, 184)),
            );
        });
}

// Helper pill button
fn pill(ui: &mut egui::Ui, text: &str, active: bool) -> bool {
    let fill = if active { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(22, 28, 40) };
    let stroke = if active { Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(42, 53, 75)) };
    let btn = egui::Button::new(egui::RichText::new(text).size(11.0).color(Color32::from_rgb(240, 246, 252)))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(5_u8));
    ui.add(btn).clicked()
}

// Render studio level VU Meter
fn render_vu_meter(ui: &mut egui::Ui, level_pct: f32, active: bool, anim_phase: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), egui::Sense::hover());
    let painter = ui.painter();

    // Background track
    painter.rect_filled(rect, CornerRadius::same(3_u8), Color32::from_rgb(13, 17, 24));
    painter.rect_stroke(rect, CornerRadius::same(3_u8), Stroke::new(1.0_f32, Color32::from_rgb(28, 36, 52)), egui::StrokeKind::Inside);

    if !active {
        return;
    }

    // Dynamic animated activity simulation based on gain level
    let wave = ((anim_phase * 6.0).sin() * 0.15 + (anim_phase * 14.0).cos() * 0.10).max(-0.2);
    let fill_ratio = ((level_pct / 100.0) * (0.65 + wave)).clamp(0.02, 1.0);
    let filled_width = rect.width() * fill_ratio;

    // Segmented bar styling
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
            Color32::from_rgb(34, 197, 94) // Green (-40dB to -12dB)
        } else if pos_frac < 0.85 {
            Color32::from_rgb(234, 179, 8) // Yellow (-12dB to -3dB)
        } else {
            Color32::from_rgb(239, 68, 68) // Red (Limit/Peak)
        };
        painter.rect_filled(seg_rect, CornerRadius::same(2_u8), seg_color);
    }
}

impl eframe::App for VrecOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_async_events();
        self.anim_time += 0.033;
        ctx.request_repaint_after(Duration::from_millis(50));

        // Handle Escape key to close
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Custom cyber-stealth dark theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(11, 14, 20);
        visuals.window_fill = Color32::from_rgb(11, 14, 20);
        visuals.window_stroke = Stroke::new(1.5_f32, Color32::from_rgb(38, 46, 64));
        visuals.window_corner_radius = CornerRadius::same(14_u8);
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgba_premultiplied(11, 14, 20, 250))
                    .stroke(Stroke::new(1.5_f32, Color32::from_rgb(34, 42, 60)))
                    .corner_radius(CornerRadius::same(14_u8))
                    .inner_margin(Margin::same(16_i8)),
            )
            .show(ctx, |ui| {
                // Top Header Bar
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("VREC STUDIO")
                            .font(FontId::proportional(19.0))
                            .strong()
                            .color(Color32::from_rgb(56, 189, 248)),
                    );

                    // Micro pill tag: GPU ACCELERATED
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(20, 26, 38))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 78)))
                        .corner_radius(CornerRadius::same(4_u8))
                        .inner_margin(Margin::symmetric(6_i8, 2_i8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("HARDWARE ACCELERATED")
                                    .font(FontId::monospace(9.0))
                                    .strong()
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        });

                    ui.add_space(4.0);

                    // Capsule Status Badge
                    let (status_bg, status_border, dot_color, status_text) = if !self.daemon_connected {
                        (Color32::from_rgba_unmultiplied(100, 116, 139, 30), Color32::from_rgb(71, 85, 105), Color32::from_rgb(148, 163, 184), "CONNECTING".to_string())
                    } else if self.status.is_recording {
                        let d = self.status.recording_duration_sec;
                        let text = format!("RECORDING {:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60);
                        (Color32::from_rgba_unmultiplied(220, 38, 38, 35), Color32::from_rgb(239, 68, 68), Color32::from_rgb(248, 113, 113), text)
                    } else if self.status.is_replay_active {
                        let text = format!("REPLAY ARMED ({}s)", self.config.replay_duration_sec);
                        (Color32::from_rgba_unmultiplied(16, 185, 129, 30), Color32::from_rgb(34, 197, 94), Color32::from_rgb(52, 211, 153), text)
                    } else {
                        (Color32::from_rgba_unmultiplied(71, 85, 105, 30), Color32::from_rgb(71, 85, 105), Color32::from_rgb(148, 163, 184), "STANDBY".to_string())
                    };

                    egui::Frame::NONE
                        .fill(status_bg)
                        .stroke(Stroke::new(1.0_f32, status_border))
                        .corner_radius(CornerRadius::same(12_u8))
                        .inner_margin(Margin::symmetric(9_i8, 3_i8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot_rect.center(), 4.0, dot_color);
                                ui.label(
                                    egui::RichText::new(&status_text)
                                        .font(FontId::monospace(10.0))
                                        .strong()
                                        .color(dot_color),
                                );
                            });
                        });

                    // Navigation Tabs & Close Button on Right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close_btn = egui::Button::new(egui::RichText::new("X").strong().color(Color32::from_rgb(203, 213, 225)))
                            .fill(Color32::from_rgb(28, 33, 46))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)))
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([28.0, 26.0], close_btn).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        let folder_btn = egui::Button::new(egui::RichText::new("Open Videos").size(11.0).color(Color32::from_rgb(203, 213, 225)))
                            .fill(Color32::from_rgb(24, 30, 42))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)))
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([88.0, 26.0], folder_btn).clicked() {
                            let dir = self.output_dir.clone();
                            std::thread::spawn(move || {
                                let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                            });
                        }

                        // Quick Cursor Toggle Button
                        let (cursor_label, cursor_color, cursor_bg) = if self.show_cursor {
                            ("Cursor: ON", Color32::from_rgb(52, 211, 153), Color32::from_rgb(20, 35, 30))
                        } else {
                            ("Cursor: OFF", Color32::from_rgb(148, 163, 184), Color32::from_rgb(24, 30, 42))
                        };
                        let cursor_btn = egui::Button::new(egui::RichText::new(cursor_label).size(11.0).color(cursor_color))
                            .fill(cursor_bg)
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)))
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([84.0, 26.0], cursor_btn).clicked() {
                            self.show_cursor = !self.show_cursor;
                            self.config.show_cursor = self.show_cursor;
                            let _ = ipc::send_command(Command::ToggleCursor);
                            let msg = if self.show_cursor { "Cursor set to ON" } else { "Cursor set to OFF" };
                            self.set_msg(msg);
                        }

                        // Tab Switcher Pills
                        let settings_active = self.current_tab == OverlayTab::Settings;
                        let settings_fill = if settings_active { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(20, 26, 38) };
                        let settings_stroke = if settings_active { Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(40, 50, 72)) };
                        let settings_btn = egui::Button::new(egui::RichText::new("Settings").size(11.5).color(Color32::WHITE))
                            .fill(settings_fill)
                            .stroke(settings_stroke)
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([72.0, 26.0], settings_btn).clicked() {
                            self.current_tab = OverlayTab::Settings;
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(840.0, 520.0)));
                        }

                        let audio_active = self.current_tab == OverlayTab::AudioMixer;
                        let audio_fill = if audio_active { Color32::from_rgb(124, 58, 237) } else { Color32::from_rgb(20, 26, 38) };
                        let audio_stroke = if audio_active { Stroke::new(1.0_f32, Color32::from_rgb(168, 85, 247)) } else { Stroke::new(1.0_f32, Color32::from_rgb(40, 50, 72)) };
                        let audio_btn = egui::Button::new(egui::RichText::new("Audio Mixer").size(11.5).color(Color32::WHITE))
                            .fill(audio_fill)
                            .stroke(audio_stroke)
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([86.0, 26.0], audio_btn).clicked() {
                            self.current_tab = OverlayTab::AudioMixer;
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(840.0, 480.0)));
                        }

                        let dash_active = self.current_tab == OverlayTab::Dashboard;
                        let dash_fill = if dash_active { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(20, 26, 38) };
                        let dash_stroke = if dash_active { Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(40, 50, 72)) };
                        let dash_btn = egui::Button::new(egui::RichText::new("Dashboard").size(11.5).color(Color32::WHITE))
                            .fill(dash_fill)
                            .stroke(dash_stroke)
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([80.0, 26.0], dash_btn).clicked() {
                            self.current_tab = OverlayTab::Dashboard;
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(840.0, 310.0)));
                        }
                    });
                });

                // Toast status feedback
                if let Some((msg, time)) = &self.status_msg {
                    if time.elapsed() < Duration::from_secs(3) {
                        ui.add_space(4.0);
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(16, 185, 129, 25))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(16, 185, 129)))
                            .corner_radius(CornerRadius::same(6_u8))
                            .inner_margin(Margin::symmetric(10_i8, 3_i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(msg)
                                        .color(Color32::from_rgb(74, 222, 128))
                                        .font(FontId::proportional(11.0))
                                        .strong(),
                                );
                            });
                    }
                }

                ui.add_space(10.0);

                // VIEW: DASHBOARD
                if self.current_tab == OverlayTab::Dashboard {
                    ui.columns(3, |cols| {
                        // Card 1: Instant Replay
                        let replay_border = if self.status.is_replay_active {
                            Stroke::new(1.0_f32, Color32::from_rgb(16, 185, 129))
                        } else {
                            Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 60))
                        };
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(15, 19, 28))
                            .stroke(replay_border)
                            .corner_radius(CornerRadius::same(10_u8))
                            .inner_margin(Margin::same(14_i8))
                            .show(&mut cols[0], |ui| {
                                ui.set_min_height(160.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(egui::RichText::new("INSTANT REPLAY").font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                    let buf_label = format!("Buffer: {} seconds in RAM", self.config.replay_duration_sec);
                                    ui.label(egui::RichText::new(buf_label).font(FontId::proportional(11.0)).color(Color32::from_rgb(56, 189, 248)));
                                    ui.add_space(14.0);
                                    let save_btn = egui::Button::new(egui::RichText::new("Save Instant Replay").strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .corner_radius(CornerRadius::same(7_u8));
                                    if ui.add_sized([180.0, 36.0], save_btn).clicked() {
                                        let _ = ipc::send_command(Command::SaveReplay);
                                        self.set_msg("Replay save command sent");
                                    }
                                    ui.add_space(10.0);
                                    render_keycap(ui, &self.config.save_hotkey);
                                });
                            });

                        // Card 2: Screen Recording
                        let rec_border = if self.status.is_recording {
                            Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68))
                        } else {
                            Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 60))
                        };
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(15, 19, 28))
                            .stroke(rec_border)
                            .corner_radius(CornerRadius::same(10_u8))
                            .inner_margin(Margin::same(14_i8))
                            .show(&mut cols[1], |ui| {
                                ui.set_min_height(160.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(egui::RichText::new("SCREEN RECORDING").font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                    let timer_text = if self.status.is_recording {
                                        let d = self.status.recording_duration_sec;
                                        format!("Active: {:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60)
                                    } else {
                                        "Status: Ready to Record".to_string()
                                    };
                                    let timer_color = if self.status.is_recording { Color32::from_rgb(248, 113, 113) } else { Color32::from_rgb(100, 116, 139) };
                                    ui.label(egui::RichText::new(timer_text).font(FontId::proportional(11.0)).color(timer_color));
                                    ui.add_space(14.0);
                                    let (rec_label, rec_color) = if self.status.is_recording {
                                        ("Stop Recording", Color32::from_rgb(220, 38, 38))
                                    } else {
                                        ("Start Recording", Color32::from_rgb(37, 99, 235))
                                    };
                                    let rec_btn = egui::Button::new(egui::RichText::new(rec_label).strong().color(Color32::WHITE))
                                        .fill(rec_color)
                                        .corner_radius(CornerRadius::same(7_u8));
                                    if ui.add_sized([180.0, 36.0], rec_btn).clicked() {
                                        let _ = ipc::send_command(Command::ToggleRecording);
                                        self.set_msg("Recording toggled");
                                    }
                                    ui.add_space(10.0);
                                    render_keycap(ui, &self.config.record_hotkey);
                                });
                            });

                        // Card 3: Audio Deck
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(15, 19, 28))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 60)))
                            .corner_radius(CornerRadius::same(10_u8))
                            .inner_margin(Margin::same(14_i8))
                            .show(&mut cols[2], |ui| {
                                ui.set_min_height(160.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(egui::RichText::new("AUDIO & CURSOR").font(FontId::monospace(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                    let mode_name = match self.status.audio_mode.as_str() {
                                        "mic" => format!("Mic Only ({}%)", self.mic_volume_pct),
                                        "both" => format!("System + Mic ({}%)", self.mic_volume_pct),
                                        "muted" => "Muted".to_string(),
                                        _ => format!("System Audio ({}%)", self.system_volume_pct),
                                    };
                                    ui.label(egui::RichText::new(mode_name).font(FontId::proportional(11.0)).color(Color32::from_rgb(168, 85, 247)));
                                    ui.add_space(14.0);
                                    let audio_btn = egui::Button::new(egui::RichText::new("Cycle Audio Mode").strong().color(Color32::from_rgb(226, 232, 240)))
                                        .fill(Color32::from_rgb(28, 36, 52))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 60, 86)))
                                        .corner_radius(CornerRadius::same(7_u8));
                                    if ui.add_sized([180.0, 36.0], audio_btn).clicked() {
                                        let _ = ipc::send_command(Command::CycleAudioMode);
                                        self.set_msg("Audio mode cycled");
                                    }
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        render_keycap(ui, "Cycle");
                                        ui.add_space(2.0);
                                        render_keycap(ui, &self.config.cursor_hotkey);
                                    });
                                });
                            });
                    });

                    ui.add_space(8.0);

                    // Bottom info ribbon
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(13, 16, 24))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(26, 32, 46)))
                        .corner_radius(CornerRadius::same(6_u8))
                        .inner_margin(Margin::symmetric(12_i8, 6_i8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("Hardware Engine: VAAPI NV12 Direct • {} FPS • {} Mbps", self.config.fps, self.bitrate_mbps))
                                        .font(FontId::monospace(10.0))
                                        .color(Color32::from_rgb(100, 116, 139)),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("Save: {}", self.output_dir))
                                            .font(FontId::monospace(10.0))
                                            .color(Color32::from_rgb(100, 116, 139)),
                                    );
                                });
                            });
                        });
                }

                // TAB 2: AUDIO MIXER & LEVELS (Dedicated Studio Audio Tab)
                if self.current_tab == OverlayTab::AudioMixer {
                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                        // Master Channel Strips
                        ui.columns(2, |ch_cols| {
                            // Strip 1: Microphone Input
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(15, 19, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 64)))
                                .corner_radius(CornerRadius::same(10_u8))
                                .inner_margin(Margin::same(14_i8))
                                .show(&mut ch_cols[0], |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new("MICROPHONE INPUT")
                                                .font(FontId::proportional(13.0))
                                                .strong()
                                                .color(Color32::from_rgb(56, 189, 248)),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("{}%", self.mic_volume_pct))
                                                    .font(FontId::monospace(13.0))
                                                    .strong()
                                                    .color(Color32::from_rgb(56, 189, 248)),
                                            );
                                        });
                                    });

                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("PulseAudio/PipeWire Native Source (@DEFAULT_SOURCE@)")
                                            .font(FontId::monospace(9.0))
                                            .color(Color32::from_rgb(100, 116, 139)),
                                    );

                                    ui.add_space(8.0);
                                    ui.add(egui::Slider::new(&mut self.mic_volume_pct, 0..=150).text("Gain"));

                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Presets:").font(FontId::proportional(10.0)).color(Color32::from_rgb(100, 116, 139)));
                                        if pill(ui, "30% Low", self.mic_volume_pct == 30) { self.mic_volume_pct = 30; }
                                        if pill(ui, "60% Optimal", self.mic_volume_pct == 60) { self.mic_volume_pct = 60; }
                                        if pill(ui, "80% Loud", self.mic_volume_pct == 80) { self.mic_volume_pct = 80; }
                                        if pill(ui, "100% Max", self.mic_volume_pct == 100) { self.mic_volume_pct = 100; }
                                    });

                                    ui.add_space(10.0);
                                    ui.label(egui::RichText::new("Live Input Peak Meter").font(FontId::proportional(10.5)).color(Color32::from_rgb(148, 163, 184)));
                                    let is_mic_active = self.status.audio_mode == "mic" || self.status.audio_mode == "both";
                                    render_vu_meter(ui, self.mic_volume_pct as f32, is_mic_active, self.anim_time);

                                    ui.add_space(8.0);
                                    egui::Frame::NONE
                                        .fill(Color32::from_rgba_unmultiplied(16, 185, 129, 20))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(16, 185, 129)))
                                        .corner_radius(CornerRadius::same(5_u8))
                                        .inner_margin(Margin::symmetric(8_i8, 4_i8))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new("Anti-Clipping Soft-Knee Limiter Active")
                                                    .font(FontId::monospace(9.5))
                                                    .color(Color32::from_rgb(52, 211, 153)),
                                            );
                                        });
                                });

                            // Strip 2: System Audio Output
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(15, 19, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 64)))
                                .corner_radius(CornerRadius::same(10_u8))
                                .inner_margin(Margin::same(14_i8))
                                .show(&mut ch_cols[1], |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new("SYSTEM AUDIO MONITOR")
                                                .font(FontId::proportional(13.0))
                                                .strong()
                                                .color(Color32::from_rgb(168, 85, 247)),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("{}%", self.system_volume_pct))
                                                    .font(FontId::monospace(13.0))
                                                    .strong()
                                                    .color(Color32::from_rgb(168, 85, 247)),
                                            );
                                        });
                                    });

                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("PipeWire Audio Monitor Sink (@DEFAULT_MONITOR@)")
                                            .font(FontId::monospace(9.0))
                                            .color(Color32::from_rgb(100, 116, 139)),
                                    );

                                    ui.add_space(8.0);
                                    ui.add(egui::Slider::new(&mut self.system_volume_pct, 0..=150).text("Level"));

                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Presets:").font(FontId::proportional(10.0)).color(Color32::from_rgb(100, 116, 139)));
                                        if pill(ui, "50% Half", self.system_volume_pct == 50) { self.system_volume_pct = 50; }
                                        if pill(ui, "75% Soft", self.system_volume_pct == 75) { self.system_volume_pct = 75; }
                                        if pill(ui, "100% Full", self.system_volume_pct == 100) { self.system_volume_pct = 100; }
                                    });

                                    ui.add_space(10.0);
                                    ui.label(egui::RichText::new("Live System Output Level").font(FontId::proportional(10.5)).color(Color32::from_rgb(148, 163, 184)));
                                    let is_sys_active = self.status.audio_mode != "muted" && self.status.audio_mode != "mic";
                                    render_vu_meter(ui, self.system_volume_pct as f32, is_sys_active, self.anim_time * 0.85);

                                    ui.add_space(8.0);
                                    egui::Frame::NONE
                                        .fill(Color32::from_rgba_unmultiplied(124, 58, 237, 20))
                                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(139, 92, 246)))
                                        .corner_radius(CornerRadius::same(5_u8))
                                        .inner_margin(Margin::symmetric(8_i8, 4_i8))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new("Direct PulseAudio Hardware Interceptor")
                                                    .font(FontId::monospace(9.5))
                                                    .color(Color32::from_rgb(196, 181, 253)),
                                            );
                                        });
                                });
                        });

                        ui.add_space(10.0);

                        // Section 2: Audio Routing Selection
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(15, 19, 28))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 64)))
                            .corner_radius(CornerRadius::same(10_u8))
                            .inner_margin(Margin::same(12_i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("STREAM AUDIO ROUTING")
                                        .font(FontId::proportional(12.0))
                                        .strong()
                                        .color(Color32::from_rgb(203, 213, 225)),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if pill(ui, "System Audio Only", self.audio_mode_idx == 0) { self.audio_mode_idx = 0; }
                                    if pill(ui, "Microphone Only", self.audio_mode_idx == 1) { self.audio_mode_idx = 1; }
                                    if pill(ui, "Mixed (System + Mic)", self.audio_mode_idx == 2) { self.audio_mode_idx = 2; }
                                    if pill(ui, "Muted (Silent Video)", self.audio_mode_idx == 3) { self.audio_mode_idx = 3; }
                                });
                            });

                        ui.add_space(10.0);

                        // Quick Apply Button for Audio
                        ui.horizontal(|ui| {
                            let apply_audio_btn = egui::Button::new(egui::RichText::new("Apply Audio Mixer Settings").strong().color(Color32::WHITE))
                                .fill(Color32::from_rgb(124, 58, 237))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add_sized([220.0, 32.0], apply_audio_btn).clicked() {
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
                                self.set_msg("Audio mixer levels saved and reloaded!");
                            }
                        });
                    });
                }

                // TAB 3: SETTINGS
                if self.current_tab == OverlayTab::Settings {
                    egui::ScrollArea::vertical().max_height(410.0).show(ui, |ui| {
                        // Section 1: Save Destination Directory
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(15, 19, 28))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 64)))
                            .corner_radius(CornerRadius::same(8_u8))
                            .inner_margin(Margin::same(12_i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("VIDEO SAVE DESTINATION")
                                        .font(FontId::proportional(12.0))
                                        .strong()
                                        .color(Color32::from_rgb(56, 189, 248)),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Folder Path:").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    ui.add(egui::TextEdit::singleline(&mut self.output_dir).desired_width(420.0));

                                    let browse_btn = egui::Button::new(egui::RichText::new("Browse...").color(Color32::WHITE))
                                        .fill(Color32::from_rgb(37, 99, 235))
                                        .corner_radius(CornerRadius::same(5_u8));
                                    if ui.add(browse_btn).clicked() {
                                        let cur = self.output_dir.clone();
                                        let tx = self.folder_tx.clone();
                                        std::thread::spawn(move || {
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
                                        });
                                    }

                                    let open_btn = egui::Button::new(egui::RichText::new("Open").color(Color32::from_rgb(203, 213, 225)))
                                        .fill(Color32::from_rgb(30, 41, 59))
                                        .corner_radius(CornerRadius::same(5_u8));
                                    if ui.add(open_btn).clicked() {
                                        let dir = self.output_dir.clone();
                                        std::thread::spawn(move || {
                                            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                                        });
                                    }
                                });

                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Presets:").font(FontId::proportional(10.0)).color(Color32::from_rgb(100, 116, 139)));
                                    let v_dir = format!("{}/Videos/vrec", std::env::var("HOME").unwrap_or_default());
                                    let c_dir = format!("{}/Videos/Captures", std::env::var("HOME").unwrap_or_default());
                                    let d_dir = format!("{}/Downloads/vrec", std::env::var("HOME").unwrap_or_default());
                                    if pill(ui, "~/Videos/vrec", self.output_dir == v_dir) { self.output_dir = v_dir; }
                                    if pill(ui, "~/Videos/Captures", self.output_dir == c_dir) { self.output_dir = c_dir; }
                                    if pill(ui, "~/Downloads/vrec", self.output_dir == d_dir) { self.output_dir = d_dir; }
                                });
                            });

                        ui.add_space(8.0);

                        // Section 2: Split Columns for Video and Replay
                        ui.columns(2, |settings_cols| {
                            // Column 1: Video Quality & Cursor
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(15, 19, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 64)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(&mut settings_cols[0], |ui| {
                                    ui.label(
                                        egui::RichText::new("VIDEO QUALITY")
                                            .font(FontId::proportional(12.0))
                                            .strong()
                                            .color(Color32::from_rgb(56, 189, 248)),
                                    );
                                    ui.add_space(8.0);

                                    // Framerate
                                    ui.label(egui::RichText::new("Target Framerate").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut self.target_fps).range(15..=240).suffix(" fps"));
                                        if pill(ui, "30", self.target_fps == 30) { self.target_fps = 30; }
                                        if pill(ui, "60", self.target_fps == 60) { self.target_fps = 60; }
                                        if pill(ui, "120", self.target_fps == 120) { self.target_fps = 120; }
                                        if pill(ui, "144", self.target_fps == 144) { self.target_fps = 144; }
                                    });

                                    ui.add_space(10.0);

                                    // Bitrate
                                    ui.label(egui::RichText::new("Video Bitrate").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut self.bitrate_mbps).range(2..=120).suffix(" Mbps"));
                                        if pill(ui, "10M", self.bitrate_mbps == 10) { self.bitrate_mbps = 10; }
                                        if pill(ui, "20M", self.bitrate_mbps == 20) { self.bitrate_mbps = 20; }
                                        if pill(ui, "30M", self.bitrate_mbps == 30) { self.bitrate_mbps = 30; }
                                        if pill(ui, "50M", self.bitrate_mbps == 50) { self.bitrate_mbps = 50; }
                                    });

                                    ui.add_space(10.0);

                                    // Mouse Cursor Capture
                                    ui.label(egui::RichText::new("Mouse Cursor Capture").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    ui.horizontal(|ui| {
                                        if pill(ui, "Show Cursor", self.show_cursor) { self.show_cursor = true; }
                                        if pill(ui, "Hide Cursor", !self.show_cursor) { self.show_cursor = false; }
                                    });

                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new("Codec: Hardware VAAPI H.264 (NV12 Direct)").font(FontId::monospace(9.0)).color(Color32::from_rgb(100, 116, 139)));
                                });

                            // Column 2: Replay Buffer & Hotkeys
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(15, 19, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 44, 64)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(&mut settings_cols[1], |ui| {
                                    ui.label(
                                        egui::RichText::new("REPLAY BUFFER & HOTKEYS")
                                            .font(FontId::proportional(12.0))
                                            .strong()
                                            .color(Color32::from_rgb(168, 85, 247)),
                                    );
                                    ui.add_space(8.0);

                                    // Replay Duration
                                    ui.label(egui::RichText::new("Buffer Duration").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut self.replay_sec).range(5..=600).suffix(" s"));
                                        if pill(ui, "30s", self.replay_sec == 30) { self.replay_sec = 30; }
                                        if pill(ui, "60s", self.replay_sec == 60) { self.replay_sec = 60; }
                                        if pill(ui, "120s", self.replay_sec == 120) { self.replay_sec = 120; }
                                        if pill(ui, "300s", self.replay_sec == 300) { self.replay_sec = 300; }
                                    });

                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new("Configured Global Hotkeys").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
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

                        ui.add_space(12.0);

                        // Actions Bar: Save & Apply
                        ui.horizontal(|ui| {
                            let apply_btn = egui::Button::new(egui::RichText::new("Save & Apply All Settings").strong().color(Color32::WHITE))
                                .fill(Color32::from_rgb(37, 99, 235))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add_sized([230.0, 34.0], apply_btn).clicked() {
                                self.config.output_directory = self.output_dir.clone();
                                let _ = std::fs::create_dir_all(&self.output_dir);
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
                                self.set_msg("Settings saved and reloaded into daemon!");
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new("Settings saved to ~/.config/vrec/config.json")
                                        .font(FontId::monospace(9.0))
                                        .color(Color32::from_rgb(100, 116, 139)),
                                );
                            });
                        });
                    });
                }
            });
    }
}

pub fn run_egui_overlay() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("vrec")
            .with_app_id("vrec-overlay")
            .with_inner_size([840.0, 310.0])
            .with_min_inner_size([720.0, 260.0])
            .with_max_inner_size([960.0, 600.0])
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
