use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, Button, ComboBoxText, CssProvider, Entry, Label,
    Orientation, Stack, StackTransitionType, StyleContext, Switch,
};
#[cfg(target_os = "linux")]
use gtk_layer_shell::{Layer, LayerShell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use crate::config::VrecConfig;
use crate::ipc::{self, Command};

pub fn show_notification_overlay() {
    show_notification("Replay saved");
}

pub fn show_notification(message: &str) {
    #[cfg(unix)]
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        return;
    }
    if gtk::init().is_err() {
        return;
    }

    let app = Application::builder()
        .application_id("com.vrec.notification")
        .build();

    let msg_text = message.to_string();
    app.connect_activate(move |app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(340)
            .default_height(70)
            .build();

        #[cfg(target_os = "linux")]
        {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_namespace("vrec-notification");
            window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 35);
            window.set_anchor(gtk_layer_shell::Edge::Top, true);
        }

        #[cfg(not(target_os = "linux"))]
        {
            window.set_decorated(false);
            window.set_keep_above(true);
            window.set_skip_taskbar_hint(true);
            window.set_position(gtk::WindowPosition::Center);
        }

        let css_provider = CssProvider::new();
        let css = r#"
            window {
                background-color: rgba(18, 20, 26, 0.95);
                border-radius: 16px;
                border: 1px solid rgba(255, 255, 255, 0.18);
                box-shadow: 0px 8px 30px rgba(0, 0, 0, 0.7);
            }
            label {
                color: #ffffff;
                font-weight: 800;
                font-size: 16px;
                padding: 12px 28px;
            }
        "#;
        let _ = css_provider.load_from_data(css.as_bytes());
        if let Some(screen) = gdk::Screen::default() {
            StyleContext::add_provider_for_screen(
                &screen,
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let label = Label::new(Some(&msg_text));
        window.add(&label);
        window.show_all();

        let window_clone = window.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(2200), move || {
            window_clone.close();
            gtk::glib::ControlFlow::Break
        });
    });

    app.run_with_args(&[] as &[&str]);
}

pub fn show_menu_overlay() {
    #[cfg(unix)]
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("Error: No display server detected (WAYLAND_DISPLAY and DISPLAY are unset).");
        return;
    }
    if gtk::init().is_err() {
        eprintln!("Error: Failed to connect to display server.");
        return;
    }

    let app = Application::builder()
        .application_id("com.vrec.hud")
        .build();

    app.connect_activate(|app| {
        let config = VrecConfig::load();

        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(860)
            .default_height(470)
            .build();

        #[cfg(target_os = "linux")]
        {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_namespace("vrec-hud");
            window.set_keyboard_interactivity(true);
            window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 70);
            window.set_anchor(gtk_layer_shell::Edge::Top, true);
        }

        #[cfg(not(target_os = "linux"))]
        {
            window.set_decorated(false);
            window.set_keep_above(true);
            window.set_skip_taskbar_hint(true);
            window.set_position(gtk::WindowPosition::Center);
        }

        // Main Stack (Allows toggling between HUD cards and Inline Settings with NO extra windows!)
        let stack = Stack::new();
        stack.set_transition_type(StackTransitionType::Crossfade);
        stack.set_transition_duration(200);

        // ==========================================
        // PAGE 1: HUD Quick Actions (Xbox / GPU Screen Recorder layout)
        // ==========================================
        let hud_page = Box::new(Orientation::Vertical, 16);
        hud_page.set_margin_start(24);
        hud_page.set_margin_end(24);
        hud_page.set_margin_top(20);
        hud_page.set_margin_bottom(20);

        // Header
        let header_box = Box::new(Orientation::Horizontal, 10);
        let title_label = Label::new(Some("vrec"));
        title_label.style_context().add_class("hud-title");
        let badge_label = Label::new(Some("GPU ZERO-COPY"));
        badge_label.style_context().add_class("badge-tag");
        let spacer = Box::new(Orientation::Horizontal, 0);
        let close_btn = Button::with_label("✕");
        close_btn.style_context().add_class("close-btn");

        header_box.pack_start(&title_label, false, false, 0);
        header_box.pack_start(&badge_label, false, false, 6);
        header_box.pack_start(&spacer, true, true, 0);
        header_box.pack_start(&close_btn, false, false, 0);

        // Cards Row (4 Primary Action Cards)
        let cards_box = Box::new(Orientation::Horizontal, 16);
        cards_box.set_halign(gtk::Align::Center);

        // Card 1: Instant Replay
        let replay_card = Box::new(Orientation::Vertical, 8);
        replay_card.style_context().add_class("card-box");
        let replay_toggle_btn = Button::with_label(if config.replay_enabled {
            "Instant Replay\nEnabled"
        } else {
            "Instant Replay\nDisabled"
        });
        replay_toggle_btn.set_size_request(170, 75);
        replay_toggle_btn.style_context().add_class("action-btn");

        let replay_save_btn = Button::with_label("Save Replay");
        replay_save_btn.style_context().add_class("sub-btn");
        replay_card.pack_start(&replay_toggle_btn, true, true, 0);
        replay_card.pack_start(&replay_save_btn, false, false, 0);

        // Card 2: Normal Recording
        let record_card = Box::new(Orientation::Vertical, 8);
        record_card.style_context().add_class("card-box");
        let record_toggle_btn = Button::with_label("Record\nStart");
        record_toggle_btn.set_size_request(170, 75);
        record_toggle_btn.style_context().add_class("action-btn");

        let record_status_sub = Label::new(Some("H.264 • 60 FPS"));
        record_status_sub.style_context().add_class("sub-label");
        record_card.pack_start(&record_toggle_btn, true, true, 0);
        record_card.pack_start(&record_status_sub, false, false, 0);

        // Card 3: Microphone & Audio
        let audio_card = Box::new(Orientation::Vertical, 8);
        audio_card.style_context().add_class("card-box");
        let mic_toggle_btn = Button::with_label("Microphone\nActive");
        mic_toggle_btn.set_size_request(170, 75);
        mic_toggle_btn.style_context().add_class("action-btn");

        let mic_status_sub = Label::new(Some("AAC • 48 kHz"));
        mic_status_sub.style_context().add_class("sub-label");
        audio_card.pack_start(&mic_toggle_btn, true, true, 0);
        audio_card.pack_start(&mic_status_sub, false, false, 0);

        // Card 4: Settings
        let settings_card = Box::new(Orientation::Vertical, 8);
        settings_card.style_context().add_class("card-box");
        let settings_open_btn = Button::with_label("Settings\nConfigure");
        settings_open_btn.set_size_request(170, 75);
        settings_open_btn.style_context().add_class("action-btn");

        let shortcut_hint = Label::new(Some("Esc to Close"));
        shortcut_hint.style_context().add_class("sub-label");
        settings_card.pack_start(&settings_open_btn, true, true, 0);
        settings_card.pack_start(&shortcut_hint, false, false, 0);

        cards_box.pack_start(&replay_card, true, true, 0);
        cards_box.pack_start(&record_card, true, true, 0);
        cards_box.pack_start(&audio_card, true, true, 0);
        cards_box.pack_start(&settings_card, true, true, 0);

        hud_page.pack_start(&header_box, false, false, 0);
        hud_page.pack_start(&cards_box, true, true, 0);

        // ==========================================
        // PAGE 2: Inline Settings Panel (Zero separate windows!)
        // ==========================================
        let settings_page = Box::new(Orientation::Vertical, 14);
        settings_page.set_margin_start(30);
        settings_page.set_margin_end(30);
        settings_page.set_margin_top(20);
        settings_page.set_margin_bottom(20);

        let settings_header = Box::new(Orientation::Horizontal, 12);
        let back_btn = Button::with_label("Back");
        back_btn.style_context().add_class("sub-btn");
        let settings_title = Label::new(Some("Recorder Settings"));
        settings_title.style_context().add_class("hud-title");
        settings_header.pack_start(&back_btn, false, false, 0);
        settings_header.pack_start(&settings_title, false, false, 0);

        let settings_grid = gtk::Grid::new();
        settings_grid.set_column_spacing(24);
        settings_grid.set_row_spacing(12);

        // Replay Duration
        let dur_lbl = Label::new(Some("Replay Duration:"));
        dur_lbl.set_halign(gtk::Align::Start);
        let dur_combo = ComboBoxText::new();
        dur_combo.append(Some("15"), "15 Seconds");
        dur_combo.append(Some("30"), "30 Seconds");
        dur_combo.append(Some("60"), "1 Minute");
        dur_combo.append(Some("120"), "2 Minutes");
        dur_combo.append(Some("300"), "5 Minutes");
        dur_combo.append(Some("600"), "10 Minutes");
        dur_combo.set_active_id(Some(&config.replay_duration_sec.to_string()));

        // Video Bitrate
        let bit_lbl = Label::new(Some("Recording Quality:"));
        bit_lbl.set_halign(gtk::Align::Start);
        let bit_combo = ComboBoxText::new();
        bit_combo.append(Some("10000"), "Low (10 Mbps)");
        bit_combo.append(Some("15000"), "Medium (15 Mbps)");
        bit_combo.append(Some("25000"), "High (25 Mbps)");
        bit_combo.append(Some("50000"), "Ultra (50 Mbps)");
        bit_combo.set_active_id(Some(&config.record_bitrate_kbps.to_string()));

        // Autostart
        let auto_lbl = Label::new(Some("Autostart on Boot:"));
        auto_lbl.set_halign(gtk::Align::Start);
        let auto_switch = Switch::new();
        auto_switch.set_active(config.autostart);

        // Hotkeys
        let save_hk_lbl = Label::new(Some("Save Replay Hotkey:"));
        save_hk_lbl.set_halign(gtk::Align::Start);
        let save_hk_entry = Entry::new();
        save_hk_entry.set_text(&config.save_hotkey);

        let menu_hk_lbl = Label::new(Some("Menu Overlay Hotkey:"));
        menu_hk_lbl.set_halign(gtk::Align::Start);
        let menu_hk_entry = Entry::new();
        menu_hk_entry.set_text(&config.menu_hotkey);

        // Framerate Selector (Crucial for low-end PCs: 30 FPS cuts GPU load by 50%)
        let fps_lbl = Label::new(Some("Framerate (FPS):"));
        fps_lbl.set_halign(gtk::Align::Start);
        let fps_combo = ComboBoxText::new();
        fps_combo.append(Some("30"), "30 FPS (Low-End PC)");
        fps_combo.append(Some("60"), "60 FPS (Smooth / Gaming)");
        fps_combo.append(Some("120"), "120 FPS (High-End)");
        fps_combo.set_active_id(Some(&config.fps.to_string()));

        // Video Codec Selector
        let codec_lbl = Label::new(Some("Video Codec:"));
        codec_lbl.set_halign(gtk::Align::Start);
        let codec_combo = ComboBoxText::new();
        codec_combo.append(Some("h264"), "H.264 / AVC (Most Compatible)");
        codec_combo.append(Some("hevc"), "HEVC / H.265 (High Efficiency)");
        codec_combo.append(Some("av1"), "AV1 (Next-Gen High Quality)");
        codec_combo.set_active_id(Some(&config.video_codec));

        // Audio Input Device Selector
        let audio_dev_lbl = Label::new(Some("Audio Device:"));
        audio_dev_lbl.set_halign(gtk::Align::Start);
        let audio_dev_combo = ComboBoxText::new();
        audio_dev_combo.append(Some("default"), "Default Device");
        for dev in crate::capture::audio::list_input_devices() {
            audio_dev_combo.append(Some(&dev), &dev);
        }
        audio_dev_combo.set_active_id(Some(&config.audio_device));

        // Save Folder
        let dir_lbl = Label::new(Some("Save Folder:"));
        dir_lbl.set_halign(gtk::Align::Start);
        let dir_entry = Entry::new();
        dir_entry.set_text(&config.output_directory);

        let record_hk_lbl = Label::new(Some("Toggle Record Hotkey:"));
        record_hk_lbl.set_halign(gtk::Align::Start);
        let record_hk_entry = Entry::new();
        record_hk_entry.set_text(&config.record_hotkey);

        settings_grid.attach(&dur_lbl, 0, 0, 1, 1);
        settings_grid.attach(&dur_combo, 1, 0, 1, 1);
        settings_grid.attach(&bit_lbl, 0, 1, 1, 1);
        settings_grid.attach(&bit_combo, 1, 1, 1, 1);
        settings_grid.attach(&fps_lbl, 0, 2, 1, 1);
        settings_grid.attach(&fps_combo, 1, 2, 1, 1);
        settings_grid.attach(&codec_lbl, 0, 3, 1, 1);
        settings_grid.attach(&codec_combo, 1, 3, 1, 1);
        settings_grid.attach(&audio_dev_lbl, 0, 4, 1, 1);
        settings_grid.attach(&audio_dev_combo, 1, 4, 1, 1);
        settings_grid.attach(&dir_lbl, 0, 5, 1, 1);
        settings_grid.attach(&dir_entry, 1, 5, 1, 1);
        settings_grid.attach(&auto_lbl, 0, 6, 1, 1);
        settings_grid.attach(&auto_switch, 1, 6, 1, 1);
        settings_grid.attach(&save_hk_lbl, 0, 7, 1, 1);
        settings_grid.attach(&save_hk_entry, 1, 7, 1, 1);
        settings_grid.attach(&record_hk_lbl, 0, 8, 1, 1);
        settings_grid.attach(&record_hk_entry, 1, 8, 1, 1);
        settings_grid.attach(&menu_hk_lbl, 0, 9, 1, 1);
        settings_grid.attach(&menu_hk_entry, 1, 9, 1, 1);

        let save_settings_btn = Button::with_label("Apply & Save");
        save_settings_btn.style_context().add_class("apply-btn");

        settings_page.pack_start(&settings_header, false, false, 0);
        settings_page.pack_start(&settings_grid, true, true, 4);
        settings_page.pack_start(&save_settings_btn, false, false, 4);

        stack.add_named(&hud_page, "hud");
        stack.add_named(&settings_page, "settings");
        window.add(&stack);

        // CSS Styling (Sleek Frosted Glass HUD)
        let css_provider = CssProvider::new();
        let css = r#"
            window {
                background-color: rgba(18, 20, 26, 0.95);
                border-radius: 20px;
                border: 1px solid rgba(255, 255, 255, 0.12);
                box-shadow: 0px 12px 40px rgba(0, 0, 0, 0.75);
                color: #ffffff;
            }
            .hud-title {
                font-size: 20px;
                font-weight: 900;
                color: #ffffff;
                letter-spacing: 1px;
            }
            .badge-tag {
                background-color: rgba(56, 189, 248, 0.2);
                color: #38bdf8;
                font-size: 10px;
                font-weight: bold;
                border-radius: 6px;
                padding: 2px 8px;
            }
            .card-box {
                background-color: rgba(255, 255, 255, 0.04);
                border: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 14px;
                padding: 10px;
            }
            .card-box:hover {
                background-color: rgba(255, 255, 255, 0.07);
                border-color: rgba(255, 255, 255, 0.18);
            }
            .action-btn {
                background-color: transparent;
                border: none;
                color: #f1f5f9;
                font-size: 15px;
                font-weight: bold;
            }
            .action-btn:hover {
                color: #38bdf8;
            }
            .recording-active {
                color: #ef4444 !important;
                animation: pulse 1s infinite alternate;
            }
            .sub-btn {
                background-color: rgba(255, 255, 255, 0.08);
                color: #cbd5e1;
                border: 1px solid rgba(255, 255, 255, 0.1);
                border-radius: 8px;
                font-size: 12px;
                font-weight: 600;
                padding: 6px 12px;
            }
            .sub-btn:hover {
                background-color: rgba(255, 255, 255, 0.16);
                color: #ffffff;
            }
            .apply-btn {
                background-color: #2563eb;
                color: #ffffff;
                border: none;
                border-radius: 10px;
                font-size: 14px;
                font-weight: bold;
                padding: 10px 20px;
            }
            .apply-btn:hover {
                background-color: #1d4ed8;
            }
            .close-btn {
                background-color: transparent;
                color: #94a3b8;
                border: none;
                font-size: 18px;
                font-weight: bold;
                padding: 4px 8px;
            }
            .close-btn:hover {
                color: #f43f5e;
            }
            .sub-label {
                color: #64748b;
                font-size: 11px;
                font-weight: 500;
            }
            entry, combobox button {
                background-color: rgba(30, 32, 40, 0.9);
                color: #ffffff;
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 8px;
                padding: 6px 10px;
            }
        "#;
        let _ = css_provider.load_from_data(css.as_bytes());
        if let Some(screen) = gdk::Screen::default() {
            StyleContext::add_provider_for_screen(
                &screen,
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        // Window Close Triggers
        let window_close = window.clone();
        close_btn.connect_clicked(move |_| {
            window_close.close();
        });

        let window_esc = window.clone();
        window.connect_key_press_event(move |_, key| {
            if key.keyval() == gdk::keys::constants::Escape {
                window_esc.close();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });

        // Stack Navigation
        let stack_to_settings = stack.clone();
        settings_open_btn.connect_clicked(move |_| {
            stack_to_settings.set_visible_child_name("settings");
        });

        let stack_to_hud = stack.clone();
        back_btn.connect_clicked(move |_| {
            stack_to_hud.set_visible_child_name("hud");
        });

        // Save Settings Action
        let stack_after_save = stack.clone();
        save_settings_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            if let Some(dur_str) = dur_combo.active_id()
                && let Ok(dur) = dur_str.parse() {
                    cfg.replay_duration_sec = dur;
                }
            if let Some(bit_str) = bit_combo.active_id()
                && let Ok(bit) = bit_str.parse() {
                    cfg.record_bitrate_kbps = bit;
                    cfg.replay_bitrate_kbps = bit;
                }
            if let Some(fps_str) = fps_combo.active_id()
                && let Ok(fps) = fps_str.parse() {
                    cfg.fps = fps;
                }
            if let Some(c) = codec_combo.active_id() {
                cfg.video_codec = c.to_string();
            }
            let dir_val = dir_entry.text().to_string();
            if !dir_val.trim().is_empty() {
                cfg.output_directory = dir_val;
            }
            if let Some(dev) = audio_dev_combo.active_id() {
                cfg.audio_device = dev.to_string();
            }
            cfg.autostart = auto_switch.is_active();
            cfg.save_hotkey = save_hk_entry.text().to_string();
            cfg.menu_hotkey = menu_hk_entry.text().to_string();
            cfg.record_hotkey = record_hk_entry.text().to_string();
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();
            stack_after_save.set_visible_child_name("hud");
        });

        // Action Handlers
        let replay_lbl_btn = replay_toggle_btn.clone();
        replay_toggle_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            cfg.replay_enabled = !cfg.replay_enabled;
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();
            if cfg.replay_enabled {
                replay_lbl_btn.set_label("Instant Replay\nEnabled");
            } else {
                replay_lbl_btn.set_label("Instant Replay\nDisabled");
            }
        });

        replay_save_btn.connect_clicked(|_| {
            let _ = ipc::send_command(Command::SaveReplay);
            show_notification_overlay();
        });

        record_toggle_btn.connect_clicked(|_| {
            let _ = ipc::send_command(Command::ToggleRecording);
        });

        mic_toggle_btn.connect_clicked(|_| {
            let _ = ipc::send_command(Command::ToggleAudio);
        });

        // Periodic Live Status Query from Daemon (Syncs live recording state & timer!)
        let rec_btn_sync = record_toggle_btn.clone();
        let mic_btn_sync = mic_toggle_btn.clone();
        let replay_btn_sync = replay_toggle_btn.clone();
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = Arc::clone(&is_running);

        gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
            if !is_running_clone.load(Ordering::Relaxed) {
                return gtk::glib::ControlFlow::Break;
            }

            if let Ok(status) = ipc::query_status() {
                if status.is_recording {
                    let mins = status.recording_duration_sec / 60;
                    let secs = status.recording_duration_sec % 60;
                    rec_btn_sync.set_label(&format!("Recording\n{:02}:{:02}", mins, secs));
                    rec_btn_sync.style_context().add_class("recording-active");
                } else {
                    rec_btn_sync.set_label("Record\nStart");
                    rec_btn_sync.style_context().remove_class("recording-active");
                }

                if status.audio_muted {
                    mic_btn_sync.set_label("Microphone\nMuted");
                } else {
                    mic_btn_sync.set_label("Microphone\nActive");
                }

                if status.is_replay_active {
                    replay_btn_sync.set_label("Instant Replay\nEnabled");
                } else {
                    replay_btn_sync.set_label("Instant Replay\nDisabled");
                }
            }
            gtk::glib::ControlFlow::Continue
        });

        window.connect_destroy(move |_| {
            is_running.store(false, Ordering::Relaxed);
        });

        window.show_all();
    });

    app.run_with_args(&[] as &[&str]);
}
