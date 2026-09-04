use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, Button, ComboBoxText, CssProvider, Entry, Label,
    Orientation, SpinButton, Stack, StackTransitionType, StyleContext, Switch,
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
            .default_width(320)
            .default_height(60)
            .build();

        #[cfg(target_os = "linux")]
        {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_namespace("vrec-notification");
            window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 30);
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
                background-color: #12141a;
                border-radius: 12px;
                border: 1px solid rgba(255, 255, 255, 0.15);
                box-shadow: 0px 8px 32px rgba(0, 0, 0, 0.8);
            }
            label {
                color: #ffffff;
                font-weight: 700;
                font-size: 14px;
                padding: 10px 24px;
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
            .default_width(820)
            .default_height(480)
            .build();

        #[cfg(target_os = "linux")]
        {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_namespace("vrec-hud");
            window.set_keyboard_interactivity(true);
            window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 50);
            window.set_anchor(gtk_layer_shell::Edge::Top, true);
        }

        #[cfg(not(target_os = "linux"))]
        {
            window.set_decorated(false);
            window.set_keep_above(true);
            window.set_skip_taskbar_hint(true);
            window.set_position(gtk::WindowPosition::Center);
        }

        let stack = Stack::new();
        stack.set_transition_type(StackTransitionType::Crossfade);
        stack.set_transition_duration(180);

        // ==========================================
        // PAGE 1: HUD Quick Actions (GPU Screen Recorder Style)
        // ==========================================
        let hud_page = Box::new(Orientation::Vertical, 16);
        hud_page.set_margin_start(24);
        hud_page.set_margin_end(24);
        hud_page.set_margin_top(18);
        hud_page.set_margin_bottom(18);

        // Header Bar
        let header_box = Box::new(Orientation::Horizontal, 10);
        let title_label = Label::new(Some("VREC"));
        title_label.style_context().add_class("hud-title");
        let badge_label = Label::new(Some("GPU ACCELERATED"));
        badge_label.style_context().add_class("badge-tag");
        let spacer = Box::new(Orientation::Horizontal, 0);
        let close_btn = Button::with_label("X");
        close_btn.style_context().add_class("close-btn");

        header_box.pack_start(&title_label, false, false, 0);
        header_box.pack_start(&badge_label, false, false, 6);
        header_box.pack_start(&spacer, true, true, 0);
        header_box.pack_start(&close_btn, false, false, 0);

        // 4 Action Cards Row
        let cards_box = Box::new(Orientation::Horizontal, 14);
        cards_box.set_halign(gtk::Align::Center);

        // Card 1: Replay
        let replay_card = Box::new(Orientation::Vertical, 8);
        replay_card.style_context().add_class("card-box");
        let replay_hdr = Box::new(Orientation::Horizontal, 6);
        let replay_title = Label::new(Some("REPLAY"));
        replay_title.style_context().add_class("card-header-label");
        let replay_pill = Label::new(Some(if config.replay_enabled { "ACTIVE" } else { "OFF" }));
        replay_pill.style_context().add_class("status-pill");
        replay_pill.style_context().add_class(if config.replay_enabled { "pill-active" } else { "pill-idle" });
        let replay_sp = Box::new(Orientation::Horizontal, 0);
        replay_hdr.pack_start(&replay_title, false, false, 0);
        replay_hdr.pack_start(&replay_sp, true, true, 0);
        replay_hdr.pack_start(&replay_pill, false, false, 0);

        let replay_toggle_btn = Button::with_label(if config.replay_enabled { "Disable Replay" } else { "Enable Replay" });
        replay_toggle_btn.set_size_request(165, 45);
        replay_toggle_btn.style_context().add_class("action-btn");

        let replay_save_btn = Button::with_label("Save Replay");
        replay_save_btn.style_context().add_class("sub-btn");
        replay_card.pack_start(&replay_hdr, false, false, 0);
        replay_card.pack_start(&replay_toggle_btn, true, true, 4);
        replay_card.pack_start(&replay_save_btn, false, false, 0);

        // Card 2: Record
        let record_card = Box::new(Orientation::Vertical, 8);
        record_card.style_context().add_class("card-box");
        let record_hdr = Box::new(Orientation::Horizontal, 6);
        let record_title = Label::new(Some("RECORD"));
        record_title.style_context().add_class("card-header-label");
        let record_pill = Label::new(Some("IDLE"));
        record_pill.style_context().add_class("status-pill");
        record_pill.style_context().add_class("pill-idle");
        let record_sp = Box::new(Orientation::Horizontal, 0);
        record_hdr.pack_start(&record_title, false, false, 0);
        record_hdr.pack_start(&record_sp, true, true, 0);
        record_hdr.pack_start(&record_pill, false, false, 0);

        let record_toggle_btn = Button::with_label("Start Record");
        record_toggle_btn.set_size_request(165, 45);
        record_toggle_btn.style_context().add_class("action-btn");

        let record_format_sub = Label::new(Some(&format!("{} • {} FPS", config.video_codec.to_uppercase(), config.fps)));
        record_format_sub.style_context().add_class("sub-label");
        record_card.pack_start(&record_hdr, false, false, 0);
        record_card.pack_start(&record_toggle_btn, true, true, 4);
        record_card.pack_start(&record_format_sub, false, false, 0);

        // Card 3: Audio Mode
        let audio_card = Box::new(Orientation::Vertical, 8);
        audio_card.style_context().add_class("card-box");
        let audio_hdr = Box::new(Orientation::Horizontal, 6);
        let audio_title = Label::new(Some("AUDIO"));
        audio_title.style_context().add_class("card-header-label");
        let audio_pill = Label::new(Some(match config.audio_mode.as_str() {
            "mic" => "MIC ONLY",
            "both" => "SYSTEM + MIC",
            "muted" => "MUTED",
            _ => "SYSTEM ONLY",
        }));
        audio_pill.style_context().add_class("status-pill");
        audio_pill.style_context().add_class(match config.audio_mode.as_str() {
            "mic" => "pill-cyan",
            "both" => "pill-purple",
            "muted" => "pill-recording",
            _ => "pill-active",
        });
        let audio_sp = Box::new(Orientation::Horizontal, 0);
        audio_hdr.pack_start(&audio_title, false, false, 0);
        audio_hdr.pack_start(&audio_sp, true, true, 0);
        audio_hdr.pack_start(&audio_pill, false, false, 0);

        let audio_cycle_btn = Button::with_label(match config.audio_mode.as_str() {
            "mic" => "Microphone Only",
            "both" => "System + Mic",
            "muted" => "Muted",
            _ => "System Audio Only",
        });
        audio_cycle_btn.set_size_request(165, 45);
        audio_cycle_btn.style_context().add_class("action-btn");

        let audio_hint_sub = Label::new(Some("Click to Cycle Mode"));
        audio_hint_sub.style_context().add_class("sub-label");
        audio_card.pack_start(&audio_hdr, false, false, 0);
        audio_card.pack_start(&audio_cycle_btn, true, true, 4);
        audio_card.pack_start(&audio_hint_sub, false, false, 0);

        // Card 4: Settings
        let settings_card = Box::new(Orientation::Vertical, 8);
        settings_card.style_context().add_class("card-box");
        let settings_hdr = Box::new(Orientation::Horizontal, 6);
        let settings_title_hdr = Label::new(Some("PREFERENCES"));
        settings_title_hdr.style_context().add_class("card-header-label");
        let settings_pill = Label::new(Some("TUNING"));
        settings_pill.style_context().add_class("status-pill");
        settings_pill.style_context().add_class("pill-idle");
        let settings_sp = Box::new(Orientation::Horizontal, 0);
        settings_hdr.pack_start(&settings_title_hdr, false, false, 0);
        settings_hdr.pack_start(&settings_sp, true, true, 0);
        settings_hdr.pack_start(&settings_pill, false, false, 0);

        let settings_open_btn = Button::with_label("Open Settings");
        settings_open_btn.set_size_request(165, 45);
        settings_open_btn.style_context().add_class("action-btn");

        let settings_hint_sub = Label::new(Some("Esc to Close"));
        settings_hint_sub.style_context().add_class("sub-label");
        settings_card.pack_start(&settings_hdr, false, false, 0);
        settings_card.pack_start(&settings_open_btn, true, true, 4);
        settings_card.pack_start(&settings_hint_sub, false, false, 0);

        cards_box.pack_start(&replay_card, true, true, 0);
        cards_box.pack_start(&record_card, true, true, 0);
        cards_box.pack_start(&audio_card, true, true, 0);
        cards_box.pack_start(&settings_card, true, true, 0);

        hud_page.pack_start(&header_box, false, false, 0);
        hud_page.pack_start(&cards_box, true, true, 0);

        // ==========================================
        // PAGE 2: Fine-Tuned Settings Panel (Numeric inputs for Time, Quality, FPS)
        // ==========================================
        let settings_page = Box::new(Orientation::Vertical, 12);
        settings_page.set_margin_start(24);
        settings_page.set_margin_end(24);
        settings_page.set_margin_top(16);
        settings_page.set_margin_bottom(16);

        let settings_header = Box::new(Orientation::Horizontal, 12);
        let back_btn = Button::with_label("Back to HUD");
        back_btn.style_context().add_class("sub-btn");
        let settings_title = Label::new(Some("Recorder Settings & Quality Tuning"));
        settings_title.style_context().add_class("hud-title");
        settings_header.pack_start(&back_btn, false, false, 0);
        settings_header.pack_start(&settings_title, false, false, 0);

        let settings_grid = gtk::Grid::new();
        settings_grid.set_column_spacing(20);
        settings_grid.set_row_spacing(10);

        // 1. Replay Duration (Seconds) - Direct numeric spin button + preset pills!
        let dur_lbl = Label::new(Some("Replay Buffer (Seconds):"));
        dur_lbl.set_halign(gtk::Align::Start);
        let dur_box = Box::new(Orientation::Horizontal, 6);
        let dur_spin = SpinButton::with_range(5.0, 1800.0, 5.0);
        dur_spin.set_value(config.replay_duration_sec as f64);
        dur_box.pack_start(&dur_spin, false, false, 0);
        
        let p_30s = Button::with_label("30s"); p_30s.style_context().add_class("preset-btn");
        let p_60s = Button::with_label("60s"); p_60s.style_context().add_class("preset-btn");
        let p_2m  = Button::with_label("2m");  p_2m.style_context().add_class("preset-btn");
        let p_5m  = Button::with_label("5m");  p_5m.style_context().add_class("preset-btn");
        let s_clone = dur_spin.clone(); p_30s.connect_clicked(move |_| s_clone.set_value(30.0));
        let s_clone = dur_spin.clone(); p_60s.connect_clicked(move |_| s_clone.set_value(60.0));
        let s_clone = dur_spin.clone(); p_2m.connect_clicked(move |_| s_clone.set_value(120.0));
        let s_clone = dur_spin.clone(); p_5m.connect_clicked(move |_| s_clone.set_value(300.0));
        dur_box.pack_start(&p_30s, false, false, 0);
        dur_box.pack_start(&p_60s, false, false, 0);
        dur_box.pack_start(&p_2m, false, false, 0);
        dur_box.pack_start(&p_5m, false, false, 0);

        // 2. Video Quality / Bitrate (Mbps) - Direct numeric spin button + preset pills!
        let bit_lbl = Label::new(Some("Video Bitrate (Mbps):"));
        bit_lbl.set_halign(gtk::Align::Start);
        let bit_box = Box::new(Orientation::Horizontal, 6);
        let bit_spin = SpinButton::with_range(2.0, 100.0, 1.0);
        bit_spin.set_value((config.record_bitrate_kbps / 1000) as f64);
        bit_box.pack_start(&bit_spin, false, false, 0);

        let b_10m = Button::with_label("10M"); b_10m.style_context().add_class("preset-btn");
        let b_20m = Button::with_label("20M"); b_20m.style_context().add_class("preset-btn");
        let b_30m = Button::with_label("30M"); b_30m.style_context().add_class("preset-btn");
        let b_50m = Button::with_label("50M"); b_50m.style_context().add_class("preset-btn");
        let b_clone = bit_spin.clone(); b_10m.connect_clicked(move |_| b_clone.set_value(10.0));
        let b_clone = bit_spin.clone(); b_20m.connect_clicked(move |_| b_clone.set_value(20.0));
        let b_clone = bit_spin.clone(); b_30m.connect_clicked(move |_| b_clone.set_value(30.0));
        let b_clone = bit_spin.clone(); b_50m.connect_clicked(move |_| b_clone.set_value(50.0));
        bit_box.pack_start(&b_10m, false, false, 0);
        bit_box.pack_start(&b_20m, false, false, 0);
        bit_box.pack_start(&b_30m, false, false, 0);
        bit_box.pack_start(&b_50m, false, false, 0);

        // 3. Framerate (FPS) - Direct numeric spin button + preset pills!
        let fps_lbl = Label::new(Some("Target Framerate (FPS):"));
        fps_lbl.set_halign(gtk::Align::Start);
        let fps_box = Box::new(Orientation::Horizontal, 6);
        let fps_spin = SpinButton::with_range(15.0, 240.0, 1.0);
        fps_spin.set_value(config.fps as f64);
        fps_box.pack_start(&fps_spin, false, false, 0);

        let f_30  = Button::with_label("30");  f_30.style_context().add_class("preset-btn");
        let f_60  = Button::with_label("60");  f_60.style_context().add_class("preset-btn");
        let f_120 = Button::with_label("120"); f_120.style_context().add_class("preset-btn");
        let f_144 = Button::with_label("144"); f_144.style_context().add_class("preset-btn");
        let f_clone = fps_spin.clone(); f_30.connect_clicked(move |_| f_clone.set_value(30.0));
        let f_clone = fps_spin.clone(); f_60.connect_clicked(move |_| f_clone.set_value(60.0));
        let f_clone = fps_spin.clone(); f_120.connect_clicked(move |_| f_clone.set_value(120.0));
        let f_clone = fps_spin.clone(); f_144.connect_clicked(move |_| f_clone.set_value(144.0));
        fps_box.pack_start(&f_30, false, false, 0);
        fps_box.pack_start(&f_60, false, false, 0);
        fps_box.pack_start(&f_120, false, false, 0);
        fps_box.pack_start(&f_144, false, false, 0);

        // 4. Video Codec
        let codec_lbl = Label::new(Some("Video Codec:"));
        codec_lbl.set_halign(gtk::Align::Start);
        let codec_combo = ComboBoxText::new();
        codec_combo.append(Some("h264"), "H.264 / AVC (Most Compatible)");
        codec_combo.append(Some("hevc"), "HEVC / H.265 (High Efficiency)");
        codec_combo.append(Some("av1"), "AV1 (Next-Gen High Quality)");
        codec_combo.set_active_id(Some(&config.video_codec));

        // 5. Audio Source Mode (System Only vs Mic Only vs Both!)
        let audio_mode_lbl = Label::new(Some("Audio Track Mode:"));
        audio_mode_lbl.set_halign(gtk::Align::Start);
        let audio_mode_combo = ComboBoxText::new();
        audio_mode_combo.append(Some("system"), "System Sounds Only (Game / Desktop)");
        audio_mode_combo.append(Some("mic"), "Microphone Only");
        audio_mode_combo.append(Some("both"), "Both (System Sounds + Microphone Merged)");
        audio_mode_combo.append(Some("muted"), "Muted (No Audio)");
        audio_mode_combo.set_active_id(Some(&config.audio_mode));

        // 6. Audio Device
        let audio_dev_lbl = Label::new(Some("Audio Hardware Device:"));
        audio_dev_lbl.set_halign(gtk::Align::Start);
        let audio_dev_combo = ComboBoxText::new();
        audio_dev_combo.append(Some("default"), "Default Device");
        for dev in crate::capture::audio::list_input_devices() {
            audio_dev_combo.append(Some(&dev), &dev);
        }
        audio_dev_combo.set_active_id(Some(&config.audio_device));

        // 7. Save Folder
        let dir_lbl = Label::new(Some("Save Folder:"));
        dir_lbl.set_halign(gtk::Align::Start);
        let dir_entry = Entry::new();
        dir_entry.set_text(&config.output_directory);

        // 8. Hotkeys
        let save_hk_lbl = Label::new(Some("Save Replay Hotkey:"));
        save_hk_lbl.set_halign(gtk::Align::Start);
        let save_hk_entry = Entry::new();
        save_hk_entry.set_text(&config.save_hotkey);

        let record_hk_lbl = Label::new(Some("Toggle Record Hotkey:"));
        record_hk_lbl.set_halign(gtk::Align::Start);
        let record_hk_entry = Entry::new();
        record_hk_entry.set_text(&config.record_hotkey);

        let menu_hk_lbl = Label::new(Some("Menu Overlay Hotkey:"));
        menu_hk_lbl.set_halign(gtk::Align::Start);
        let menu_hk_entry = Entry::new();
        menu_hk_entry.set_text(&config.menu_hotkey);

        // 9. Autostart
        let auto_lbl = Label::new(Some("Autostart on Login:"));
        auto_lbl.set_halign(gtk::Align::Start);
        let auto_switch = Switch::new();
        auto_switch.set_active(config.autostart);

        settings_grid.attach(&dur_lbl, 0, 0, 1, 1);
        settings_grid.attach(&dur_box, 1, 0, 1, 1);
        settings_grid.attach(&bit_lbl, 0, 1, 1, 1);
        settings_grid.attach(&bit_box, 1, 1, 1, 1);
        settings_grid.attach(&fps_lbl, 0, 2, 1, 1);
        settings_grid.attach(&fps_box, 1, 2, 1, 1);
        settings_grid.attach(&codec_lbl, 0, 3, 1, 1);
        settings_grid.attach(&codec_combo, 1, 3, 1, 1);
        settings_grid.attach(&audio_mode_lbl, 0, 4, 1, 1);
        settings_grid.attach(&audio_mode_combo, 1, 4, 1, 1);
        settings_grid.attach(&audio_dev_lbl, 0, 5, 1, 1);
        settings_grid.attach(&audio_dev_combo, 1, 5, 1, 1);
        settings_grid.attach(&dir_lbl, 0, 6, 1, 1);
        settings_grid.attach(&dir_entry, 1, 6, 1, 1);
        settings_grid.attach(&save_hk_lbl, 0, 7, 1, 1);
        settings_grid.attach(&save_hk_entry, 1, 7, 1, 1);
        settings_grid.attach(&record_hk_lbl, 0, 8, 1, 1);
        settings_grid.attach(&record_hk_entry, 1, 8, 1, 1);
        settings_grid.attach(&menu_hk_lbl, 0, 9, 1, 1);
        settings_grid.attach(&menu_hk_entry, 1, 9, 1, 1);
        settings_grid.attach(&auto_lbl, 0, 10, 1, 1);
        settings_grid.attach(&auto_switch, 1, 10, 1, 1);

        let save_settings_btn = Button::with_label("Apply & Save Settings");
        save_settings_btn.style_context().add_class("apply-btn");

        settings_page.pack_start(&settings_header, false, false, 0);
        settings_page.pack_start(&settings_grid, true, true, 4);
        settings_page.pack_start(&save_settings_btn, false, false, 4);

        stack.add_named(&hud_page, "hud");
        stack.add_named(&settings_page, "settings");
        window.add(&stack);

        // CSS Styling (Brushed Stealth HUD Theme)
        let css_provider = CssProvider::new();
        let css = r#"
            window {
                background-color: #12141a;
                border-radius: 16px;
                border: 1px solid rgba(255, 255, 255, 0.1);
                box-shadow: 0 16px 48px rgba(0, 0, 0, 0.85);
                color: #f1f5f9;
            }
            .hud-title {
                font-size: 18px;
                font-weight: 800;
                color: #ffffff;
                letter-spacing: 1.5px;
            }
            .badge-tag {
                background-color: rgba(56, 189, 248, 0.12);
                color: #38bdf8;
                border: 1px solid rgba(56, 189, 248, 0.3);
                font-size: 10px;
                font-weight: 700;
                border-radius: 6px;
                padding: 2px 8px;
            }
            .card-box {
                background-color: #181b23;
                border: 1px solid rgba(255, 255, 255, 0.07);
                border-radius: 12px;
                padding: 12px;
                min-width: 170px;
            }
            .card-box:hover {
                background-color: #1e222d;
                border-color: rgba(255, 255, 255, 0.15);
            }
            .card-header-label {
                font-size: 11px;
                font-weight: 700;
                color: #94a3b8;
                letter-spacing: 0.8px;
            }
            .status-pill {
                font-size: 10px;
                font-weight: 800;
                padding: 2px 8px;
                border-radius: 10px;
            }
            .pill-active {
                background-color: rgba(16, 185, 129, 0.15);
                color: #10b981;
                border: 1px solid rgba(16, 185, 129, 0.3);
            }
            .pill-idle {
                background-color: rgba(148, 163, 184, 0.12);
                color: #94a3b8;
                border: 1px solid rgba(148, 163, 184, 0.2);
            }
            .pill-recording {
                background-color: rgba(239, 68, 68, 0.2);
                color: #ef4444;
                border: 1px solid rgba(239, 68, 68, 0.4);
            }
            .pill-cyan {
                background-color: rgba(6, 182, 212, 0.15);
                color: #06b6d4;
                border: 1px solid rgba(6, 182, 212, 0.3);
            }
            .pill-purple {
                background-color: rgba(168, 85, 247, 0.15);
                color: #a855f7;
                border: 1px solid rgba(168, 85, 247, 0.3);
            }
            .action-btn {
                background-color: rgba(255, 255, 255, 0.06);
                border: 1px solid rgba(255, 255, 255, 0.1);
                border-radius: 8px;
                color: #ffffff;
                font-size: 13px;
                font-weight: 700;
                padding: 8px 12px;
            }
            .action-btn:hover {
                background-color: #2563eb;
                border-color: #3b82f6;
                color: #ffffff;
            }
            .recording-active {
                background-color: rgba(239, 68, 68, 0.25) !important;
                border-color: #ef4444 !important;
                color: #ef4444 !important;
            }
            .sub-btn {
                background-color: transparent;
                border: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 6px;
                color: #94a3b8;
                font-size: 11px;
                font-weight: 600;
                padding: 5px 10px;
            }
            .sub-btn:hover {
                background-color: rgba(255, 255, 255, 0.08);
                color: #ffffff;
            }
            .preset-btn {
                background-color: #1e222d;
                border: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 6px;
                color: #94a3b8;
                font-size: 11px;
                font-weight: 600;
                padding: 4px 8px;
            }
            .preset-btn:hover {
                background-color: rgba(37, 99, 235, 0.2);
                border-color: #3b82f6;
                color: #ffffff;
            }
            .apply-btn {
                background-color: #2563eb;
                color: #ffffff;
                border: none;
                border-radius: 8px;
                font-size: 13px;
                font-weight: 700;
                padding: 10px 24px;
            }
            .apply-btn:hover {
                background-color: #1d4ed8;
            }
            .close-btn {
                background-color: transparent;
                color: #94a3b8;
                border: none;
                font-size: 16px;
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
            entry, spinbutton, combobox button {
                background-color: #171a22;
                color: #f1f5f9;
                border: 1px solid rgba(255, 255, 255, 0.1);
                border-radius: 8px;
                padding: 6px 10px;
                font-size: 13px;
                font-weight: 600;
            }
            entry:focus, spinbutton:focus, combobox button:focus {
                border-color: #3b82f6;
                background-color: #1c202a;
            }
            spinbutton button {
                background-color: transparent;
                color: #94a3b8;
                border: none;
            }
            spinbutton button:hover {
                color: #ffffff;
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
            cfg.replay_duration_sec = dur_spin.value() as u32;
            let bit_mbps = bit_spin.value() as u32;
            cfg.record_bitrate_kbps = bit_mbps * 1000;
            cfg.replay_bitrate_kbps = bit_mbps * 1000;
            cfg.fps = fps_spin.value() as u32;
            
            if let Some(c) = codec_combo.active_id() {
                cfg.video_codec = c.to_string();
            }
            if let Some(m) = audio_mode_combo.active_id() {
                cfg.audio_mode = m.to_string();
            }
            if let Some(dev) = audio_dev_combo.active_id() {
                cfg.audio_device = dev.to_string();
            }
            let dir_val = dir_entry.text().to_string();
            if !dir_val.trim().is_empty() {
                cfg.output_directory = dir_val;
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
        let replay_pill_clone = replay_pill.clone();
        replay_toggle_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            cfg.replay_enabled = !cfg.replay_enabled;
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();
            if cfg.replay_enabled {
                replay_lbl_btn.set_label("Disable Replay");
                replay_pill_clone.set_label("ACTIVE");
                replay_pill_clone.style_context().remove_class("pill-idle");
                replay_pill_clone.style_context().add_class("pill-active");
            } else {
                replay_lbl_btn.set_label("Enable Replay");
                replay_pill_clone.set_label("OFF");
                replay_pill_clone.style_context().remove_class("pill-active");
                replay_pill_clone.style_context().add_class("pill-idle");
            }
        });

        replay_save_btn.connect_clicked(|_| {
            let _ = ipc::send_command(Command::SaveReplay);
            show_notification_overlay();
        });

        record_toggle_btn.connect_clicked(|_| {
            let _ = ipc::send_command(Command::ToggleRecording);
        });

        // Audio Source Mode Quick-Cycle
        let audio_pill_clone = audio_pill.clone();
        let audio_btn_clone = audio_cycle_btn.clone();
        audio_cycle_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            let next_mode = match cfg.audio_mode.as_str() {
                "system" => "mic",
                "mic" => "both",
                "both" => "muted",
                _ => "system",
            };
            cfg.audio_mode = next_mode.to_string();
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();

            audio_pill_clone.style_context().remove_class("pill-active");
            audio_pill_clone.style_context().remove_class("pill-cyan");
            audio_pill_clone.style_context().remove_class("pill-purple");
            audio_pill_clone.style_context().remove_class("pill-recording");

            match next_mode {
                "mic" => {
                    audio_pill_clone.set_label("MIC ONLY");
                    audio_pill_clone.style_context().add_class("pill-cyan");
                    audio_btn_clone.set_label("Microphone Only");
                }
                "both" => {
                    audio_pill_clone.set_label("SYSTEM + MIC");
                    audio_pill_clone.style_context().add_class("pill-purple");
                    audio_btn_clone.set_label("System + Mic");
                }
                "muted" => {
                    audio_pill_clone.set_label("MUTED");
                    audio_pill_clone.style_context().add_class("pill-recording");
                    audio_btn_clone.set_label("Muted");
                }
                _ => {
                    audio_pill_clone.set_label("SYSTEM ONLY");
                    audio_pill_clone.style_context().add_class("pill-active");
                    audio_btn_clone.set_label("System Audio Only");
                }
            }
        });

        // Periodic Status Sync (Timer & Live States)
        let rec_btn_sync = record_toggle_btn.clone();
        let rec_pill_sync = record_pill.clone();
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
                    rec_btn_sync.set_label("Stop Recording");
                    rec_btn_sync.style_context().add_class("recording-active");
                    rec_pill_sync.set_label(&format!("REC {:02}:{:02}", mins, secs));
                    rec_pill_sync.style_context().remove_class("pill-idle");
                    rec_pill_sync.style_context().add_class("pill-recording");
                } else {
                    rec_btn_sync.set_label("Start Record");
                    rec_btn_sync.style_context().remove_class("recording-active");
                    rec_pill_sync.set_label("IDLE");
                    rec_pill_sync.style_context().remove_class("pill-recording");
                    rec_pill_sync.style_context().add_class("pill-idle");
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
