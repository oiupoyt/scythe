use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Margin, Stroke, Vec2};
use std::time::{Duration, Instant};
use crate::config::VrecConfig;
use crate::ipc::{self, Command, DaemonStatus};

pub struct VrecOverlayApp {
    config: VrecConfig,
    status: DaemonStatus,
    last_status_fetch: Instant,
    show_settings: bool,
    replay_sec: u32,
    bitrate_mbps: u32,
    target_fps: u32,
    audio_mode_idx: usize,
    status_msg: Option<(String, Instant)>,
}

impl VrecOverlayApp {
    pub fn new() -> Self {
        let config = VrecConfig::load();
        let status = ipc::query_status().unwrap_or_default();
        let replay_sec = config.replay_duration_sec;
        let bitrate_mbps = (config.record_bitrate_kbps / 1000).max(1);
        let target_fps = config.fps;
        let audio_mode_idx = match config.audio_mode.as_str() {
            "mic" => 1,
            "both" => 2,
            "muted" => 3,
            _ => 0,
        };

        Self {
            config,
            status,
            last_status_fetch: Instant::now(),
            show_settings: false,
            replay_sec,
            bitrate_mbps,
            target_fps,
            audio_mode_idx,
            status_msg: None,
        }
    }

    fn refresh_status(&mut self) {
        if self.last_status_fetch.elapsed() > Duration::from_millis(400) {
            if let Ok(s) = ipc::query_status() {
                self.status = s;
            }
            self.last_status_fetch = Instant::now();
        }
    }

    fn set_msg(&mut self, text: &str) {
        self.status_msg = Some((text.to_string(), Instant::now()));
    }
}

// Helper to render realistic mechanical keyboard keycap badges
fn render_keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(Color32::from_rgb(22, 27, 38))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(48, 56, 75)))
        .corner_radius(CornerRadius::same(4_u8))
        .inner_margin(Margin::symmetric(7_i8, 3_i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .font(FontId::monospace(10.0))
                    .color(Color32::from_rgb(148, 163, 184)),
            );
        });
}

impl eframe::App for VrecOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_status();
        ctx.request_repaint_after(Duration::from_millis(200));

        // Handle Escape key to close
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Custom cyber-stealth dark theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(11, 14, 20);
        visuals.window_fill = Color32::from_rgb(11, 14, 20);
        visuals.window_stroke = Stroke::new(1.5_f32, Color32::from_rgb(38, 46, 62));
        visuals.window_corner_radius = CornerRadius::same(14_u8);
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgba_premultiplied(11, 14, 20, 248))
                    .stroke(Stroke::new(1.5_f32, Color32::from_rgb(34, 42, 58)))
                    .corner_radius(CornerRadius::same(14_u8))
                    .inner_margin(Margin::same(16_i8)),
            )
            .show(ctx, |ui| {
                // Top Header Bar
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("VREC")
                            .font(FontId::proportional(20.0))
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
                                egui::RichText::new("GPU ACCELERATED")
                                    .font(FontId::monospace(9.0))
                                    .strong()
                                    .color(Color32::from_rgb(148, 163, 184)),
                            );
                        });

                    ui.add_space(6.0);

                    // Capsule Status Badge
                    let (status_bg, status_border, dot_color, status_text) = if self.status.is_recording {
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
                        .inner_margin(Margin::symmetric(10_i8, 4_i8))
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

                    // Right side controls: Settings toggle & Close button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close_btn = egui::Button::new(egui::RichText::new("X").strong().color(Color32::from_rgb(203, 213, 225)))
                            .fill(Color32::from_rgb(28, 33, 46))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)))
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([28.0, 26.0], close_btn).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        let settings_text = if self.show_settings { "Close Settings" } else { "Settings" };
                        let settings_fill = if self.show_settings { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(24, 29, 41) };
                        let settings_stroke = if self.show_settings { Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)) };
                        let settings_btn = egui::Button::new(egui::RichText::new(settings_text).size(12.0).color(Color32::WHITE))
                            .fill(settings_fill)
                            .stroke(settings_stroke)
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add_sized([108.0, 26.0], settings_btn).clicked() {
                            self.show_settings = !self.show_settings;
                            let target_height = if self.show_settings { 510.0 } else { 190.0 };
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(760.0, target_height)));
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

                // Main HUD Cards Grid (4 Cards)
                ui.columns(4, |cols| {
                    // Card 1: Replay
                    let replay_border = if self.status.is_replay_active {
                        Stroke::new(1.0_f32, Color32::from_rgb(16, 185, 129))
                    } else {
                        Stroke::new(1.0_f32, Color32::from_rgb(34, 42, 58))
                    };
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(16, 20, 29))
                        .stroke(replay_border)
                        .corner_radius(CornerRadius::same(10_u8))
                        .inner_margin(Margin::same(12_i8))
                        .show(&mut cols[0], |ui| {
                            ui.set_min_height(108.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("INSTANT REPLAY").font(FontId::proportional(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                let buf_label = format!("{}s Rolling Buffer", self.config.replay_duration_sec);
                                ui.label(egui::RichText::new(buf_label).font(FontId::proportional(11.0)).color(Color32::from_rgb(56, 189, 248)));
                                ui.add_space(8.0);
                                let save_btn = egui::Button::new(egui::RichText::new("Save Replay").strong().color(Color32::WHITE))
                                    .fill(Color32::from_rgb(16, 185, 129))
                                    .corner_radius(CornerRadius::same(7_u8));
                                if ui.add_sized([128.0, 30.0], save_btn).clicked() {
                                    let _ = ipc::send_command(Command::SaveReplay);
                                    self.set_msg("Replay save command sent");
                                }
                                ui.add_space(6.0);
                                render_keycap(ui, &self.config.save_hotkey);
                            });
                        });

                    // Card 2: Record
                    let rec_border = if self.status.is_recording {
                        Stroke::new(1.0_f32, Color32::from_rgb(239, 68, 68))
                    } else {
                        Stroke::new(1.0_f32, Color32::from_rgb(34, 42, 58))
                    };
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(16, 20, 29))
                        .stroke(rec_border)
                        .corner_radius(CornerRadius::same(10_u8))
                        .inner_margin(Margin::same(12_i8))
                        .show(&mut cols[1], |ui| {
                            ui.set_min_height(108.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("RECORD").font(FontId::proportional(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                let timer_text = if self.status.is_recording {
                                    let d = self.status.recording_duration_sec;
                                    format!("{:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60)
                                } else {
                                    "Ready to Record".to_string()
                                };
                                let timer_color = if self.status.is_recording { Color32::from_rgb(248, 113, 113) } else { Color32::from_rgb(100, 116, 139) };
                                ui.label(egui::RichText::new(timer_text).font(FontId::proportional(11.0)).color(timer_color));
                                ui.add_space(8.0);
                                let (rec_label, rec_color) = if self.status.is_recording {
                                    ("Stop Recording", Color32::from_rgb(220, 38, 38))
                                } else {
                                    ("Start Record", Color32::from_rgb(37, 99, 235))
                                };
                                let rec_btn = egui::Button::new(egui::RichText::new(rec_label).strong().color(Color32::WHITE))
                                    .fill(rec_color)
                                    .corner_radius(CornerRadius::same(7_u8));
                                if ui.add_sized([128.0, 30.0], rec_btn).clicked() {
                                    let _ = ipc::send_command(Command::ToggleRecording);
                                    self.set_msg("Recording toggled");
                                }
                                ui.add_space(6.0);
                                render_keycap(ui, &self.config.record_hotkey);
                            });
                        });

                    // Card 3: Audio
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(16, 20, 29))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 42, 58)))
                        .corner_radius(CornerRadius::same(10_u8))
                        .inner_margin(Margin::same(12_i8))
                        .show(&mut cols[2], |ui| {
                            ui.set_min_height(108.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("AUDIO").font(FontId::proportional(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                let mode_name = match self.status.audio_mode.as_str() {
                                    "mic" => "Microphone",
                                    "both" => "System + Mic",
                                    "muted" => "Muted",
                                    _ => "System Audio",
                                };
                                ui.label(egui::RichText::new(mode_name).font(FontId::proportional(11.0)).color(Color32::from_rgb(168, 85, 247)));
                                ui.add_space(8.0);
                                let audio_btn = egui::Button::new(egui::RichText::new("Cycle Audio").strong().color(Color32::from_rgb(226, 232, 240)))
                                    .fill(Color32::from_rgb(30, 41, 59))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                    .corner_radius(CornerRadius::same(7_u8));
                                if ui.add_sized([128.0, 30.0], audio_btn).clicked() {
                                    let _ = ipc::send_command(Command::CycleAudioMode);
                                    self.refresh_status();
                                    self.set_msg("Audio mode cycled");
                                }
                                ui.add_space(6.0);
                                render_keycap(ui, "Cycle");
                            });
                        });

                    // Card 4: Hardware / Encoder
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(16, 20, 29))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 42, 58)))
                        .corner_radius(CornerRadius::same(10_u8))
                        .inner_margin(Margin::same(12_i8))
                        .show(&mut cols[3], |ui| {
                            ui.set_min_height(108.0);
                            ui.vertical_centered(|ui| {
                                ui.label(egui::RichText::new("ENCODER").font(FontId::proportional(11.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                                let codec_info = format!("{} / {}fps", self.config.video_codec.to_uppercase(), self.config.fps);
                                ui.label(egui::RichText::new(codec_info).font(FontId::proportional(11.0)).color(Color32::from_rgb(251, 146, 60)));
                                ui.add_space(8.0);
                                let panel_btn_text = if self.show_settings { "Hide Panel" } else { "Configure" };
                                let panel_btn = egui::Button::new(egui::RichText::new(panel_btn_text).strong().color(Color32::from_rgb(226, 232, 240)))
                                    .fill(Color32::from_rgb(45, 55, 72))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(74, 85, 104)))
                                    .corner_radius(CornerRadius::same(7_u8));
                                if ui.add_sized([128.0, 30.0], panel_btn).clicked() {
                                    self.show_settings = !self.show_settings;
                                    let target_height = if self.show_settings { 510.0 } else { 190.0 };
                                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(760.0, target_height)));
                                }
                                ui.add_space(6.0);
                                render_keycap(ui, &self.config.menu_hotkey);
                            });
                        });
                });

                // Detailed Settings Panel (Expanded View)
                if self.show_settings {
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(10.0);

                    let pill = |ui: &mut egui::Ui, text: &str, active: bool| -> bool {
                        let fill = if active { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(24, 30, 42) };
                        let stroke = if active { Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)) };
                        let btn = egui::Button::new(egui::RichText::new(text).size(11.0).color(Color32::from_rgb(240, 246, 252)))
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(5_u8));
                        ui.add(btn).clicked()
                    };

                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.columns(2, |settings_cols| {
                            // Column 1: Video & Encoding
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 29))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 42, 58)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(&mut settings_cols[0], |ui| {
                                    ui.label(
                                        egui::RichText::new("VIDEO ENCODING")
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

                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new("Encoder: VAAPI / NVENC Zero-Copy").font(FontId::monospace(9.0)).color(Color32::from_rgb(100, 116, 139)));
                                });

                            // Column 2: Replay & Audio Routing
                            egui::Frame::NONE
                                .fill(Color32::from_rgb(16, 20, 29))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 42, 58)))
                                .corner_radius(CornerRadius::same(8_u8))
                                .inner_margin(Margin::same(12_i8))
                                .show(&mut settings_cols[1], |ui| {
                                    ui.label(
                                        egui::RichText::new("CAPTURE & AUDIO")
                                            .font(FontId::proportional(12.0))
                                            .strong()
                                            .color(Color32::from_rgb(168, 85, 247)),
                                    );
                                    ui.add_space(8.0);

                                    // Replay Duration
                                    ui.label(egui::RichText::new("Replay Buffer Duration").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut self.replay_sec).range(5..=600).suffix(" s"));
                                        if pill(ui, "30s", self.replay_sec == 30) { self.replay_sec = 30; }
                                        if pill(ui, "60s", self.replay_sec == 60) { self.replay_sec = 60; }
                                        if pill(ui, "120s", self.replay_sec == 120) { self.replay_sec = 120; }
                                        if pill(ui, "300s", self.replay_sec == 300) { self.replay_sec = 300; }
                                    });

                                    ui.add_space(10.0);

                                    // Audio Mode
                                    ui.label(egui::RichText::new("Audio Source Routing").font(FontId::proportional(11.0)).color(Color32::from_rgb(203, 213, 225)));
                                    egui::ComboBox::from_id_salt("audio_routing_cb")
                                        .selected_text(match self.audio_mode_idx {
                                            1 => "Microphone Only",
                                            2 => "Both (System + Mic)",
                                            3 => "Muted",
                                            _ => "System Audio Only",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.audio_mode_idx, 0, "System Audio Only");
                                            ui.selectable_value(&mut self.audio_mode_idx, 1, "Microphone Only");
                                            ui.selectable_value(&mut self.audio_mode_idx, 2, "Both (System + Mic)");
                                            ui.selectable_value(&mut self.audio_mode_idx, 3, "Muted");
                                        });

                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new("Captures default monitor audio").font(FontId::monospace(9.0)).color(Color32::from_rgb(100, 116, 139)));
                                });
                        });

                        ui.add_space(12.0);

                        // Actions Bar: Save & Apply + Folder
                        ui.horizontal(|ui| {
                            let apply_btn = egui::Button::new(egui::RichText::new("Save & Apply Settings").strong().color(Color32::WHITE))
                                .fill(Color32::from_rgb(37, 99, 235))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add_sized([180.0, 32.0], apply_btn).clicked() {
                                self.config.replay_duration_sec = self.replay_sec;
                                self.config.record_bitrate_kbps = self.bitrate_mbps * 1000;
                                self.config.replay_bitrate_kbps = self.bitrate_mbps * 1000;
                                self.config.fps = self.target_fps;
                                self.config.audio_mode = match self.audio_mode_idx {
                                    1 => "mic",
                                    2 => "both",
                                    3 => "muted",
                                    _ => "system",
                                }.to_string();
                                let _ = self.config.save();
                                let _ = ipc::send_command(Command::ReloadConfig);
                                self.set_msg("Settings saved and reloaded into daemon");
                            }

                            ui.add_space(6.0);

                            let folder_btn = egui::Button::new(egui::RichText::new("Open Recordings Folder").color(Color32::from_rgb(226, 232, 240)))
                                .fill(Color32::from_rgb(30, 41, 59))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                .corner_radius(CornerRadius::same(6_u8));
                            if ui.add_sized([180.0, 32.0], folder_btn).clicked() {
                                let dir = self.config.output_directory.clone();
                                std::thread::spawn(move || {
                                    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                                });
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(&self.config.output_directory)
                                        .font(FontId::monospace(10.0))
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
            .with_inner_size([760.0, 190.0])
            .with_min_inner_size([640.0, 170.0])
            .with_max_inner_size([840.0, 560.0])
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
