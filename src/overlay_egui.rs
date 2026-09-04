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

impl eframe::App for VrecOverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_status();
        ctx.request_repaint_after(Duration::from_millis(200));

        // Handle Escape key to close
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Custom dark stealth theme matching GPU Screen Recorder
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(15, 17, 23);
        visuals.window_fill = Color32::from_rgb(15, 17, 23);
        visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 64));
        visuals.window_corner_radius = CornerRadius::same(14_u8);
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgba_premultiplied(13, 16, 23, 245))
                    .stroke(Stroke::new(1.5_f32, Color32::from_rgb(38, 44, 58)))
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
                            .color(Color32::from_rgb(88, 166, 255)),
                    );

                    let (dot_color, status_text) = if self.status.is_recording {
                        (Color32::from_rgb(239, 68, 68), "RECORDING")
                    } else if self.status.is_replay_active {
                        (Color32::from_rgb(34, 197, 94), "REPLAY READY")
                    } else {
                        (Color32::from_rgb(156, 163, 175), "STANDBY")
                    };

                    ui.add_space(8.0);
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 5.0, dot_color);
                    ui.label(
                        egui::RichText::new(status_text)
                            .font(FontId::proportional(12.0))
                            .color(dot_color)
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("X").strong())
                                    .fill(Color32::from_rgb(30, 34, 45))
                                    .corner_radius(CornerRadius::same(6_u8)),
                            )
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        let settings_text = if self.show_settings { "Close Settings" } else { "Settings" };
                        let settings_btn = egui::Button::new(egui::RichText::new(settings_text).color(Color32::from_rgb(200, 210, 225)))
                            .fill(if self.show_settings { Color32::from_rgb(38, 45, 60) } else { Color32::from_rgb(25, 28, 38) })
                            .corner_radius(CornerRadius::same(6_u8));
                        if ui.add(settings_btn).clicked() {
                            self.show_settings = !self.show_settings;
                        }
                    });
                });

                if let Some((msg, time)) = &self.status_msg {
                    if time.elapsed() < Duration::from_secs(3) {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(msg)
                                .color(Color32::from_rgb(74, 222, 128))
                                .font(FontId::proportional(12.0)),
                        );
                    }
                }

                ui.add_space(12.0);

                // Reusable custom card frame
                let card_frame = egui::Frame::NONE
                    .fill(Color32::from_rgb(18, 22, 31))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 45, 62)))
                    .corner_radius(CornerRadius::same(10_u8))
                    .inner_margin(Margin::same(12_i8));

                // Main HUD Cards Grid (4 Cards)
                ui.columns(4, |cols| {
                    // Card 1: Replay
                    card_frame.show(&mut cols[0], |ui| {
                        ui.set_min_height(115.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("REPLAY").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                            let buf_label = format!("{}s Buffer", self.config.replay_duration_sec);
                            ui.label(egui::RichText::new(buf_label).font(FontId::proportional(11.0)).color(Color32::from_rgb(56, 189, 248)));
                            ui.add_space(10.0);
                            let save_btn = egui::Button::new(egui::RichText::new("Save Replay").strong().color(Color32::WHITE))
                                .fill(Color32::from_rgb(16, 185, 129))
                                .corner_radius(CornerRadius::same(8_u8));
                            if ui.add_sized([120.0, 32.0], save_btn).clicked() {
                                let _ = ipc::send_command(Command::SaveReplay);
                                self.set_msg("Replay save command sent!");
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&self.config.save_hotkey).font(FontId::monospace(10.0)).color(Color32::from_rgb(71, 85, 105)));
                        });
                    });

                    // Card 2: Record
                    card_frame.show(&mut cols[1], |ui| {
                        ui.set_min_height(115.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("RECORD").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                            let timer_text = if self.status.is_recording {
                                let d = self.status.recording_duration_sec;
                                format!("{:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60)
                            } else {
                                "Idle".to_string()
                            };
                            let timer_color = if self.status.is_recording { Color32::from_rgb(248, 113, 113) } else { Color32::from_rgb(100, 116, 139) };
                            ui.label(egui::RichText::new(timer_text).font(FontId::proportional(11.0)).color(timer_color));
                            ui.add_space(10.0);
                            let (rec_label, rec_color) = if self.status.is_recording {
                                ("Stop Recording", Color32::from_rgb(220, 38, 38))
                            } else {
                                ("Start Record", Color32::from_rgb(37, 99, 235))
                            };
                            let rec_btn = egui::Button::new(egui::RichText::new(rec_label).strong().color(Color32::WHITE))
                                .fill(rec_color)
                                .corner_radius(CornerRadius::same(8_u8));
                            if ui.add_sized([120.0, 32.0], rec_btn).clicked() {
                                let _ = ipc::send_command(Command::ToggleRecording);
                                self.set_msg("Recording toggled!");
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&self.config.record_hotkey).font(FontId::monospace(10.0)).color(Color32::from_rgb(71, 85, 105)));
                        });
                    });

                    // Card 3: Audio
                    card_frame.show(&mut cols[2], |ui| {
                        ui.set_min_height(115.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("AUDIO").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                            let mode_name = match self.status.audio_mode.as_str() {
                                "mic" => "Microphone",
                                "both" => "System + Mic",
                                "muted" => "Muted",
                                _ => "System Audio",
                            };
                            ui.label(egui::RichText::new(mode_name).font(FontId::proportional(11.0)).color(Color32::from_rgb(168, 85, 247)));
                            ui.add_space(10.0);
                            let audio_btn = egui::Button::new(egui::RichText::new("Cycle Mode").strong().color(Color32::from_rgb(226, 232, 240)))
                                .fill(Color32::from_rgb(30, 41, 59))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                                .corner_radius(CornerRadius::same(8_u8));
                            if ui.add_sized([120.0, 32.0], audio_btn).clicked() {
                                let _ = ipc::send_command(Command::CycleAudioMode);
                                self.refresh_status();
                                self.set_msg("Audio mode cycled!");
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("Tap to cycle").font(FontId::proportional(10.0)).color(Color32::from_rgb(71, 85, 105)));
                        });
                    });

                    // Card 4: Preferences Toggle
                    card_frame.show(&mut cols[3], |ui| {
                        ui.set_min_height(115.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("SETTINGS").font(FontId::proportional(12.0)).strong().color(Color32::from_rgb(148, 163, 184)));
                            let codec_info = format!("{} / {}fps", self.config.video_codec.to_uppercase(), self.config.fps);
                            ui.label(egui::RichText::new(codec_info).font(FontId::proportional(11.0)).color(Color32::from_rgb(251, 146, 60)));
                            ui.add_space(10.0);
                            let gear_label = if self.show_settings { "Hide Panel" } else { "Open Panel" };
                            let gear_btn = egui::Button::new(egui::RichText::new(gear_label).strong().color(Color32::from_rgb(226, 232, 240)))
                                .fill(Color32::from_rgb(45, 55, 72))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(74, 85, 104)))
                                .corner_radius(CornerRadius::same(8_u8));
                            if ui.add_sized([120.0, 32.0], gear_btn).clicked() {
                                self.show_settings = !self.show_settings;
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("Configure").font(FontId::proportional(10.0)).color(Color32::from_rgb(71, 85, 105)));
                        });
                    });
                });

                // Detailed Settings Panel (Collapsible / Dynamic)
                if self.show_settings {
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(10.0);

                    let pill = |ui: &mut egui::Ui, text: &str, active: bool| -> bool {
                        let fill = if active { Color32::from_rgb(37, 99, 235) } else { Color32::from_rgb(26, 32, 44) };
                        let stroke = if active { Stroke::new(1.0_f32, Color32::from_rgb(96, 165, 250)) } else { Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 72)) };
                        let btn = egui::Button::new(egui::RichText::new(text).size(11.0).color(Color32::from_rgb(240, 246, 252)))
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(6_u8));
                        ui.add(btn).clicked()
                    };

                    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                        ui.label(egui::RichText::new("Precise Tuning & Hardware Configuration").strong().color(Color32::from_rgb(203, 213, 225)));
                        ui.add_space(8.0);

                        // 1. Replay Duration with Number Input & Pills
                        ui.horizontal(|ui| {
                            ui.set_min_width(180.0);
                            ui.label("Replay Duration (sec):");
                            ui.add(egui::DragValue::new(&mut self.replay_sec).range(5..=600).suffix(" s"));
                            ui.add_space(8.0);
                            if pill(ui, "30s", self.replay_sec == 30) { self.replay_sec = 30; }
                            if pill(ui, "60s", self.replay_sec == 60) { self.replay_sec = 60; }
                            if pill(ui, "120s", self.replay_sec == 120) { self.replay_sec = 120; }
                            if pill(ui, "300s", self.replay_sec == 300) { self.replay_sec = 300; }
                        });

                        ui.add_space(6.0);

                        // 2. Video Bitrate with Number Input & Pills
                        ui.horizontal(|ui| {
                            ui.set_min_width(180.0);
                            ui.label("Bitrate (Mbps):");
                            ui.add(egui::DragValue::new(&mut self.bitrate_mbps).range(2..=120).suffix(" Mbps"));
                            ui.add_space(8.0);
                            if pill(ui, "10M", self.bitrate_mbps == 10) { self.bitrate_mbps = 10; }
                            if pill(ui, "20M", self.bitrate_mbps == 20) { self.bitrate_mbps = 20; }
                            if pill(ui, "30M", self.bitrate_mbps == 30) { self.bitrate_mbps = 30; }
                            if pill(ui, "50M", self.bitrate_mbps == 50) { self.bitrate_mbps = 50; }
                        });

                        ui.add_space(6.0);

                        // 3. Target Framerate with Number Input & Pills
                        ui.horizontal(|ui| {
                            ui.set_min_width(180.0);
                            ui.label("Framerate (FPS):");
                            ui.add(egui::DragValue::new(&mut self.target_fps).range(15..=240).suffix(" fps"));
                            ui.add_space(8.0);
                            if pill(ui, "30", self.target_fps == 30) { self.target_fps = 30; }
                            if pill(ui, "60", self.target_fps == 60) { self.target_fps = 60; }
                            if pill(ui, "120", self.target_fps == 120) { self.target_fps = 120; }
                            if pill(ui, "144", self.target_fps == 144) { self.target_fps = 144; }
                        });

                        ui.add_space(6.0);

                        // 4. Audio Routing Mode
                        ui.horizontal(|ui| {
                            ui.set_min_width(180.0);
                            ui.label("Audio Routing Mode:");
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
                        });

                        ui.add_space(12.0);

                        // Save & Apply Button
                        ui.horizontal(|ui| {
                            if ui.add_sized([160.0, 32.0], egui::Button::new(egui::RichText::new("Save & Apply").strong()).fill(Color32::from_rgb(37, 99, 235)).corner_radius(CornerRadius::same(6_u8))).clicked() {
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
                                self.set_msg("Settings applied & saved!");
                            }

                            if ui.add_sized([180.0, 32.0], egui::Button::new("Open Recordings Folder").fill(Color32::from_rgb(30, 41, 59)).corner_radius(CornerRadius::same(6_u8))).clicked() {
                                let dir = self.config.output_directory.clone();
                                std::thread::spawn(move || {
                                    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                                });
                            }
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
            .with_inner_size([740.0, 220.0])
            .with_min_inner_size([640.0, 180.0])
            .with_max_inner_size([820.0, 560.0])
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
