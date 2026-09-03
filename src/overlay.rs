use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Label, CssProvider, StyleContext};
use gtk_layer_shell::{LayerShell, Layer};
use std::time::Duration;
use crate::config::VrecConfig;
use crate::settings_ui;

pub fn show_notification_overlay() {
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        return;
    }
    if gtk::init().is_err() {
        return;
    }

    let app = Application::builder()
        .application_id("com.vrec.notification")
        .build();

    app.connect_activate(|app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(300)
            .default_height(80)
            .build();

        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace("vrec-overlay");
        window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 40);
        window.set_anchor(gtk_layer_shell::Edge::Top, true);
        
        let css_provider = CssProvider::new();
        let css = r#"
            window {
                background-color: rgba(30, 30, 30, 0.9);
                border-radius: 12px;
                color: #ffffff;
                font-weight: bold;
                font-size: 20px;
                padding: 15px;
                box-shadow: 0px 4px 15px rgba(0, 0, 0, 0.5);
            }
        "#;
        css_provider.load_from_data(css.as_bytes()).unwrap();
        StyleContext::add_provider_for_screen(
            &gdk::Screen::default().unwrap(),
            &css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let label = Label::new(Some("✅ Replay Saved!"));
        window.add(&label);

        window.show_all();

        let window_clone = window.clone();
        glib::timeout_add_local(Duration::from_secs(2), move || {
            window_clone.close();
            glib::ControlFlow::Break
        });
    });

    app.run_with_args(&[] as &[&str]);
}

pub fn show_menu_overlay() {
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("Error: No display server detected (WAYLAND_DISPLAY and DISPLAY are unset).");
        return;
    }
    if gtk::init().is_err() {
        eprintln!("Error: Failed to connect to display server.");
        return;
    }

    let app = Application::builder()
        .application_id("com.vrec.menu")
        .build();

    app.connect_activate(|app| {
        let config = VrecConfig::load();
        
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(700)
            .default_height(200)
            .build();

        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace("vrec-menu");
        window.set_keyboard_interactivity(true);
        window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 100);
        window.set_anchor(gtk_layer_shell::Edge::Top, true);

        let main_box = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        main_box.set_margin_start(20);
        main_box.set_margin_end(20);
        main_box.set_margin_top(20);
        main_box.set_margin_bottom(20);
        main_box.set_halign(gtk::Align::Center);
        
        let replay_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let replay_label = if config.replay_enabled { "Instant Replay\n🟢 ON" } else { "Instant Replay\n⚪ OFF" };
        let replay_btn = gtk::Button::with_label(replay_label);
        replay_btn.set_size_request(200, 100);
        let replay_settings_btn = gtk::Button::with_label("⚙️ Replay Settings");
        
        let cfg_clone1 = config.clone();
        replay_settings_btn.connect_clicked(move |_| {
            settings_ui::open_replay_settings(&cfg_clone1);
        });
        
        replay_box.pack_start(&replay_btn, true, true, 0);
        replay_box.pack_start(&replay_settings_btn, false, false, 0);

        let record_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let record_label = if config.record_enabled { "Recording\n🔴 ON" } else { "Recording\n⚪ OFF" };
        let record_btn = gtk::Button::with_label(record_label);
        record_btn.set_size_request(200, 100);
        let record_settings_btn = gtk::Button::with_label("⚙️ Record Settings");
        
        let cfg_clone2 = config.clone();
        record_settings_btn.connect_clicked(move |_| {
            settings_ui::open_record_settings(&cfg_clone2);
        });
        
        record_box.pack_start(&record_btn, true, true, 0);
        record_box.pack_start(&record_settings_btn, false, false, 0);

        let settings_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let gen_settings_btn = gtk::Button::with_label("General Settings");
        gen_settings_btn.set_size_request(200, 50);
        
        let cfg_clone3 = config.clone();
        gen_settings_btn.connect_clicked(move |_| {
            settings_ui::open_general_settings(&cfg_clone3);
        });

        let close_btn = gtk::Button::with_label("❌ Close Menu");
        close_btn.set_size_request(200, 50);
        
        let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        settings_box.pack_start(&gen_settings_btn, false, false, 0);
        settings_box.pack_start(&spacer, true, true, 0);
        settings_box.pack_start(&close_btn, false, false, 0);

        main_box.pack_start(&replay_box, true, true, 0);
        main_box.pack_start(&record_box, true, true, 0);
        main_box.pack_start(&settings_box, true, true, 0);

        window.add(&main_box);

        let window_clone = window.clone();
        close_btn.connect_clicked(move |_| {
            window_clone.close();
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

        let replay_btn_clone = replay_btn.clone();
        replay_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            cfg.replay_enabled = !cfg.replay_enabled;
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();
            if cfg.replay_enabled {
                replay_btn_clone.set_label("Instant Replay\n🟢 ON");
            } else {
                replay_btn_clone.set_label("Instant Replay\n⚪ OFF");
            }
        });

        let record_btn_clone = record_btn.clone();
        record_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            cfg.record_enabled = !cfg.record_enabled;
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();
            if cfg.record_enabled {
                record_btn_clone.set_label("Recording\n🔴 ON");
                let _ = crate::ipc::send_command(crate::ipc::Command::StartRecording);
            } else {
                record_btn_clone.set_label("Recording\n⚪ OFF");
                let _ = crate::ipc::send_command(crate::ipc::Command::StopRecording);
            }
        });


        let css_provider = CssProvider::new();
        let css = r#"
            window {
                background-color: rgba(25, 25, 25, 0.95);
                border-radius: 16px;
                color: #ffffff;
            }
            button {
                background-color: rgba(50, 50, 50, 0.8);
                color: white;
                border-radius: 12px;
                padding: 10px;
                font-weight: bold;
                font-size: 16px;
                border: 1px solid rgba(100, 100, 100, 0.5);
            }
            button:hover {
                background-color: rgba(80, 80, 80, 0.9);
            }
        "#;
        css_provider.load_from_data(css.as_bytes()).unwrap();
        StyleContext::add_provider_for_screen(
            &gdk::Screen::default().unwrap(),
            &css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        window.show_all();
    });

    app.run_with_args(&[] as &[&str]);
}
