use gtk::prelude::*;
use gtk::{Window, WindowType, Box, Orientation, Label, Entry, Button, Switch, ComboBoxText};
use crate::config::VrecConfig;

pub fn open_replay_settings(config_in: &VrecConfig) {
    let window = Window::new(WindowType::Toplevel);
    window.set_title("Replay Settings");
    window.set_default_size(300, 200);

    let vbox = Box::new(Orientation::Vertical, 10);
    vbox.set_margin(15);

    let duration_label = Label::new(Some("Replay Duration (seconds):"));
    let duration_entry = Entry::new();
    duration_entry.set_text(&config_in.replay_duration_sec.to_string());

    let bitrate_label = Label::new(Some("Bitrate (kbps):"));
    let bitrate_entry = Entry::new();
    bitrate_entry.set_text(&config_in.replay_bitrate_kbps.to_string());

    let save_btn = Button::with_label("Save");
    
    vbox.pack_start(&duration_label, false, false, 0);
    vbox.pack_start(&duration_entry, false, false, 0);
    vbox.pack_start(&bitrate_label, false, false, 0);
    vbox.pack_start(&bitrate_entry, false, false, 0);
    vbox.pack_start(&save_btn, false, false, 0);

    window.add(&vbox);

    let window_clone = window.clone();
    save_btn.connect_clicked(move |_| {
        let mut cfg = VrecConfig::load();
        if let Ok(val) = duration_entry.text().parse() {
            cfg.replay_duration_sec = val;
        }
        if let Ok(val) = bitrate_entry.text().parse() {
            cfg.replay_bitrate_kbps = val;
        }
        let _ = cfg.save(); VrecConfig::notify_daemon_reload();
        window_clone.close();
    });

    window.show_all();
}

pub fn open_record_settings(config_in: &VrecConfig) {
    let window = Window::new(WindowType::Toplevel);
    window.set_title("Record Settings");
    window.set_default_size(300, 150);

    let vbox = Box::new(Orientation::Vertical, 10);
    vbox.set_margin(15);

    let bitrate_label = Label::new(Some("Recording Bitrate (kbps):"));
    let bitrate_entry = Entry::new();
    bitrate_entry.set_text(&config_in.record_bitrate_kbps.to_string());

    let save_btn = Button::with_label("Save");
    
    vbox.pack_start(&bitrate_label, false, false, 0);
    vbox.pack_start(&bitrate_entry, false, false, 0);
    vbox.pack_start(&save_btn, false, false, 0);

    window.add(&vbox);

    let window_clone = window.clone();
    save_btn.connect_clicked(move |_| {
        let mut cfg = VrecConfig::load();
        if let Ok(val) = bitrate_entry.text().parse() {
            cfg.record_bitrate_kbps = val;
        }
        let _ = cfg.save(); VrecConfig::notify_daemon_reload();
        window_clone.close();
    });

    window.show_all();
}

pub fn open_general_settings(config_in: &VrecConfig) {
    let window = Window::new(WindowType::Toplevel);
    window.set_title("General Settings");
    window.set_default_size(350, 300);

    let vbox = Box::new(Orientation::Vertical, 10);
    vbox.set_margin(15);

    // Autostart
    let hbox_auto = Box::new(Orientation::Horizontal, 10);
    let auto_label = Label::new(Some("Autostart with system:"));
    let auto_switch = Switch::new();
    auto_switch.set_active(config_in.autostart);
    hbox_auto.pack_start(&auto_label, false, false, 0);
    hbox_auto.pack_start(&auto_switch, false, false, 0);

    // Theme
    let theme_label = Label::new(Some("UI Theme:"));
    let theme_combo = ComboBoxText::new();
    theme_combo.append_text("dark");
    theme_combo.append_text("light");
    theme_combo.set_active(Some(if config_in.ui_color_theme == "dark" { 0 } else { 1 }));

    // Language
    let lang_label = Label::new(Some("Language Code (e.g. en, fr):"));
    let lang_entry = Entry::new();
    lang_entry.set_text(&config_in.language);

    // Hotkeys
    let menu_hk_label = Label::new(Some("Menu Hotkey:"));
    let menu_hk_entry = Entry::new();
    menu_hk_entry.set_text(&config_in.menu_hotkey);
    
    let save_hk_label = Label::new(Some("Save Replay Hotkey:"));
    let save_hk_entry = Entry::new();
    save_hk_entry.set_text(&config_in.save_hotkey);

    let save_btn = Button::with_label("Save General Settings");
    
    vbox.pack_start(&hbox_auto, false, false, 0);
    vbox.pack_start(&theme_label, false, false, 0);
    vbox.pack_start(&theme_combo, false, false, 0);
    vbox.pack_start(&lang_label, false, false, 0);
    vbox.pack_start(&lang_entry, false, false, 0);
    vbox.pack_start(&menu_hk_label, false, false, 0);
    vbox.pack_start(&menu_hk_entry, false, false, 0);
    vbox.pack_start(&save_hk_label, false, false, 0);
    vbox.pack_start(&save_hk_entry, false, false, 0);
    vbox.pack_start(&save_btn, false, false, 0);

    window.add(&vbox);

    let window_clone = window.clone();
    save_btn.connect_clicked(move |_| {
        let mut cfg = VrecConfig::load();
        cfg.autostart = auto_switch.is_active();
        if let Some(txt) = theme_combo.active_text() {
            cfg.ui_color_theme = txt.to_string();
        }
        cfg.language = lang_entry.text().to_string();
        cfg.menu_hotkey = menu_hk_entry.text().to_string();
        cfg.save_hotkey = save_hk_entry.text().to_string();
        let _ = cfg.save(); VrecConfig::notify_daemon_reload();
        window_clone.close();
    });

    window.show_all();
}
