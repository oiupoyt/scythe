use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Label, CssProvider, StyleContext};
use gtk_layer_shell::{LayerShell, Layer};
use std::time::Duration;

pub fn show_notification_overlay() {
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
        
        // Make it transparent and borderless
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

        // Close after 2 seconds
        let window_clone = window.clone();
        glib::timeout_add_local(Duration::from_secs(2), move || {
            window_clone.close();
            glib::ControlFlow::Break
        });
    });

    app.run_with_args(&[] as &[&str]);
}

pub fn show_menu_overlay() {
    let app = Application::builder()
        .application_id("com.vrec.menu")
        .build();

    app.connect_activate(|app| {
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

        // Main layout
        let main_box = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        main_box.set_margin_start(20);
        main_box.set_margin_end(20);
        main_box.set_margin_top(20);
        main_box.set_margin_bottom(20);
        main_box.set_halign(gtk::Align::Center);
        
        // 1. Replay Box
        let replay_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let replay_btn = gtk::Button::with_label("Instant Replay\n🟢 ON");
        replay_btn.set_size_request(200, 100);
        let replay_settings_btn = gtk::Button::with_label("⚙️ Replay Settings");
        replay_box.pack_start(&replay_btn, true, true, 0);
        replay_box.pack_start(&replay_settings_btn, false, false, 0);

        // 2. Record Box
        let record_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let record_btn = gtk::Button::with_label("Recording\n⚪ OFF");
        record_btn.set_size_request(200, 100);
        let record_settings_btn = gtk::Button::with_label("⚙️ Record Settings");
        record_box.pack_start(&record_btn, true, true, 0);
        record_box.pack_start(&record_settings_btn, false, false, 0);

        // 3. Settings Box
        let settings_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let gen_settings_btn = gtk::Button::with_label("General Settings");
        gen_settings_btn.set_size_request(200, 50);
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

        // Add CSS styling
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
