use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, Button, CheckButton, ComboBoxText, CssProvider,
    DrawingArea, Entry, Label, Orientation, Revealer, RevealerTransitionType, Scale,
    ScrolledWindow, SpinButton, Stack, StackTransitionType, StyleContext, Switch,
};
#[cfg(target_os = "linux")]
use gtk_layer_shell::{Layer, LayerShell};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use crate::config::VrecConfig;
use crate::ipc::{self, Command};

pub fn show_notification_overlay() {
    show_notification("Replay saved");
}

pub fn show_notification(message: &str) {
    #[cfg(target_os = "windows")]
    {
        let msg = message.to_string();
        std::thread::spawn(move || {
            use std::os::windows::process::CommandExt;
            let script = format!(
                "[reflection.assembly]::loadwithpartialname('System.Windows.Forms') | Out-Null; \
                 $notify = new-object system.windows.forms.notifyicon; \
                 $notify.icon = [System.Drawing.SystemIcons]::Information; \
                 $notify.visible = $true; \
                 $notify.showballoontip(2000, 'vrec', '{}', [system.windows.forms.tooltipicon]::Info); \
                 Start-Sleep -Seconds 2; \
                 $notify.dispose()",
                msg.replace('\'', "''")
            );
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
                .creation_flags(0x08000000)
                .output();
        });
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
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
                .default_height(54)
                .build();

            #[cfg(target_os = "linux")]
            {
                window.init_layer_shell();
                window.set_layer(Layer::Overlay);
                window.set_namespace("vrec-notification");
                window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 45);
                window.set_anchor(gtk_layer_shell::Edge::Top, true);
            }

            if let Some(screen) = gdk::Screen::default()
                && let Some(visual) = screen.rgba_visual() {
                    window.set_visual(Some(&visual));
            }
            window.set_app_paintable(true);
            window.connect_draw(|_, cr| {
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
                cr.set_operator(gtk::cairo::Operator::Source);
                let _ = cr.paint();
                gtk::glib::Propagation::Proceed
            });

            let css_provider = CssProvider::new();
            let css = r#"
                window {
                    background-color: transparent !important;
                    background: transparent !important;
                }
                .notify-box {
                    background-color: rgba(14, 18, 26, 0.94);
                    border-radius: 10px;
                    border: 1px solid #76b900;
                    box-shadow: 0px 8px 32px rgba(0, 0, 0, 0.8);
                    padding: 8px 20px;
                }
                .notify-badge {
                    background-color: #76b900;
                    color: #0b1204;
                    font-size: 10px;
                    font-weight: 800;
                    border-radius: 4px;
                    padding: 2px 6px;
                }
                .notify-label {
                    color: #ffffff;
                    font-weight: 700;
                    font-size: 13px;
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

            let hbox = Box::new(Orientation::Horizontal, 10);
            hbox.style_context().add_class("notify-box");
            hbox.set_halign(gtk::Align::Center);

            let badge = Label::new(Some("VREC"));
            badge.style_context().add_class("notify-badge");
            let label = Label::new(Some(&msg_text));
            label.style_context().add_class("notify-label");

            hbox.pack_start(&badge, false, false, 0);
            hbox.pack_start(&label, false, false, 0);
            window.add(&hbox);
            window.show_all();

            let window_clone = window.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(2200), move || {
                window_clone.close();
                gtk::glib::ControlFlow::Break
            });
        });

        app.run_with_args(&[] as &[&str]);
    }
}

// Vector Icon Drawing Helpers (NVIDIA ShadowPlay & GPU Screen Recorder Style)
fn draw_replay_icon(cr: &gtk::cairo::Context, is_active: bool) {
    let cx = 28.0;
    let cy = 28.0;
    let r = 16.0;

    if is_active {
        cr.set_source_rgb(0.463, 0.725, 0.0); // #76b900 NVIDIA green
    } else {
        cr.set_source_rgb(0.58, 0.64, 0.72); // #94a3b8
    }
    cr.set_line_width(3.2);
    cr.arc(cx, cy, r, 0.25 * PI, 1.80 * PI);
    let _ = cr.stroke();

    // Arrowhead at top-left
    let a_x = cx + r * (0.25 * PI).cos();
    let a_y = cy + r * (0.25 * PI).sin();
    cr.move_to(a_x, a_y);
    cr.line_to(a_x - 7.0, a_y);
    cr.line_to(a_x, a_y - 7.0);
    cr.close_path();
    let _ = cr.fill();

    // Play triangle in center
    let tri_r = 6.0;
    cr.move_to(cx + tri_r, cy);
    cr.line_to(cx - tri_r * 0.6, cy - tri_r * 0.86);
    cr.line_to(cx - tri_r * 0.6, cy + tri_r * 0.86);
    cr.close_path();
    let _ = cr.fill();
}

fn draw_record_icon(cr: &gtk::cairo::Context, is_recording: bool) {
    let cx = 28.0;
    let cy = 28.0;
    if is_recording {
        // Glowing red recording indicator
        cr.set_source_rgba(0.937, 0.267, 0.267, 0.25);
        cr.arc(cx, cy, 22.0, 0.0, PI * 2.0);
        let _ = cr.fill();

        cr.set_source_rgb(0.937, 0.267, 0.267); // #ef4444
        cr.set_line_width(2.8);
        cr.arc(cx, cy, 18.0, 0.0, PI * 2.0);
        let _ = cr.stroke();

        cr.set_source_rgb(0.937, 0.267, 0.267);
        cr.arc(cx, cy, 9.0, 0.0, PI * 2.0);
        let _ = cr.fill();
    } else {
        cr.set_source_rgb(0.58, 0.64, 0.72); // #94a3b8
        cr.set_line_width(2.5);
        cr.arc(cx, cy, 17.0, 0.0, PI * 2.0);
        let _ = cr.stroke();

        cr.set_source_rgb(0.88, 0.91, 0.94);
        cr.arc(cx, cy, 8.0, 0.0, PI * 2.0);
        let _ = cr.fill();
    }
}

fn draw_gear_icon(cr: &gtk::cairo::Context) {
    let cx = 28.0;
    let cy = 28.0;
    let r = 16.0;

    cr.set_source_rgb(0.58, 0.64, 0.72); // #94a3b8
    cr.set_line_width(2.2);
    cr.arc(cx, cy, r * 0.75, 0.0, PI * 2.0);
    let _ = cr.stroke();

    cr.arc(cx, cy, r * 0.30, 0.0, PI * 2.0);
    let _ = cr.stroke();

    for i in 0..8 {
        let angle = i as f64 * (PI / 4.0);
        let p_in_x = cx + angle.cos() * (r * 0.68);
        let p_in_y = cy + angle.sin() * (r * 0.68);
        let p_out_x = cx + angle.cos() * (r * 1.05);
        let p_out_y = cy + angle.sin() * (r * 1.05);
        cr.set_line_width(3.0);
        cr.move_to(p_in_x, p_in_y);
        cr.line_to(p_out_x, p_out_y);
        let _ = cr.stroke();
    }
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
            .default_width(740)
            .default_height(280)
            .build();

        #[cfg(target_os = "linux")]
        {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_namespace("vrec-overlay");
            window.set_keyboard_interactivity(true);
            // Position at upper ~20-25% of the screen, just like GPU Screen Recorder UI
            window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 110);
            window.set_anchor(gtk_layer_shell::Edge::Top, true);
        }

        #[cfg(not(target_os = "linux"))]
        {
            window.set_decorated(false);
            window.set_keep_above(true);
            window.set_skip_taskbar_hint(true);
            window.set_position(gtk::WindowPosition::Center);
        }

        // RGBA transparent visual setup
        if let Some(screen) = gdk::Screen::default()
            && let Some(visual) = screen.rgba_visual() {
                window.set_visual(Some(&visual));
        }
        window.set_app_paintable(true);
        window.connect_draw(|_, cr| {
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            cr.set_operator(gtk::cairo::Operator::Source);
            let _ = cr.paint();
            gtk::glib::Propagation::Proceed
        });

        let stack = Stack::new();
        stack.set_transition_type(StackTransitionType::Crossfade);
        stack.set_transition_duration(160);
        stack.set_homogeneous(false);

        // =========================================================================
        // PAGE 1: 3 SQUARE CARDS WITH DROPDOWN MENUS (NVIDIA ShadowPlay Exact Match)
        // =========================================================================
        let hud_page = Box::new(Orientation::Vertical, 8);
        hud_page.style_context().add_class("hud-wrapper");
        hud_page.set_size_request(720, -1);

        // Top Bar (Floating frosted strip)
        let header_box = Box::new(Orientation::Horizontal, 10);
        header_box.style_context().add_class("header-bar");

        let brand_badge = Label::new(Some("VREC"));
        brand_badge.style_context().add_class("vrec-badge");

        let title_label = Label::new(Some("SHADOWPLAY OVERLAY"));
        title_label.style_context().add_class("overlay-title");

        let status_banner = Label::new(Some(""));
        status_banner.style_context().add_class("status-banner");

        let spacer = Box::new(Orientation::Horizontal, 0);

        let close_btn = Button::with_label("✕");
        close_btn.style_context().add_class("close-btn");

        header_box.pack_start(&brand_badge, false, false, 0);
        header_box.pack_start(&title_label, false, false, 4);
        header_box.pack_start(&status_banner, false, false, 12);
        header_box.pack_start(&spacer, true, true, 0);
        header_box.pack_start(&close_btn, false, false, 0);

        // Row of 3 Cards
        let cards_box = Box::new(Orientation::Horizontal, 14);
        cards_box.style_context().add_class("cards-container");
        cards_box.set_halign(gtk::Align::Center);

        // State Trackers for Dynamic Drawing
        let replay_active_state = Arc::new(AtomicBool::new(config.replay_enabled));
        let record_active_state = Arc::new(AtomicBool::new(false));

        // -------------------------------------------------------------------------
        // CARD 1: INSTANT REPLAY (With Attached Dropdown Menu)
        // -------------------------------------------------------------------------
        let replay_col = Box::new(Orientation::Vertical, 0);
        replay_col.set_size_request(215, -1);

        let replay_card_btn = Button::new();
        replay_card_btn.style_context().add_class("card-btn");
        replay_card_btn.set_size_request(215, 180);

        let replay_card_inner = Box::new(Orientation::Vertical, 6);
        let replay_title_lbl = Label::new(Some("INSTANT REPLAY"));
        replay_title_lbl.style_context().add_class("card-title");

        let replay_icon_area = DrawingArea::new();
        replay_icon_area.set_size_request(56, 56);
        let r_state_clone = Arc::clone(&replay_active_state);
        replay_icon_area.connect_draw(move |_, cr| {
            draw_replay_icon(cr, r_state_clone.load(Ordering::Relaxed));
            gtk::glib::Propagation::Proceed
        });

        let replay_status_lbl = Label::new(Some(if config.replay_enabled { "On" } else { "Off" }));
        replay_status_lbl.style_context().add_class("card-status");
        if config.replay_enabled {
            replay_status_lbl.style_context().add_class("status-green");
        }

        let replay_sub_lbl = Label::new(Some(&format!("{}s Buffer", config.replay_duration_sec)));
        replay_sub_lbl.style_context().add_class("card-sub");

        replay_card_inner.pack_start(&replay_title_lbl, false, false, 0);
        replay_card_inner.pack_start(&replay_icon_area, true, true, 4);
        replay_card_inner.pack_start(&replay_status_lbl, false, false, 0);
        replay_card_inner.pack_start(&replay_sub_lbl, false, false, 2);
        replay_card_btn.add(&replay_card_inner);

        // Instant Replay Attached Dropdown Menu
        let replay_revealer = Revealer::new();
        replay_revealer.set_transition_type(RevealerTransitionType::SlideDown);
        replay_revealer.set_transition_duration(150);

        let replay_menu_box = Box::new(Orientation::Vertical, 2);
        replay_menu_box.style_context().add_class("dropdown-menu");

        let replay_toggle_item = Button::with_label(if config.replay_enabled { "Turn off" } else { "Turn on" });
        replay_toggle_item.style_context().add_class("dropdown-item");

        let replay_save_item = Button::with_label("Save (Ctrl+Shift+R)");
        replay_save_item.style_context().add_class("dropdown-item");

        let replay_settings_item = Button::with_label("Settings");
        replay_settings_item.style_context().add_class("dropdown-item");

        replay_menu_box.pack_start(&replay_toggle_item, false, false, 0);
        replay_menu_box.pack_start(&replay_save_item, false, false, 0);
        replay_menu_box.pack_start(&replay_settings_item, false, false, 0);
        replay_revealer.add(&replay_menu_box);

        replay_col.pack_start(&replay_card_btn, false, false, 0);
        replay_col.pack_start(&replay_revealer, false, false, 0);

        // -------------------------------------------------------------------------
        // CARD 2: RECORD (With Attached Dropdown Menu)
        // -------------------------------------------------------------------------
        let record_col = Box::new(Orientation::Vertical, 0);
        record_col.set_size_request(215, -1);

        let record_card_btn = Button::new();
        record_card_btn.style_context().add_class("card-btn");
        record_card_btn.set_size_request(215, 180);

        let record_card_inner = Box::new(Orientation::Vertical, 6);
        let record_title_lbl = Label::new(Some("RECORD"));
        record_title_lbl.style_context().add_class("card-title");

        let record_icon_area = DrawingArea::new();
        record_icon_area.set_size_request(56, 56);
        let rec_state_clone = Arc::clone(&record_active_state);
        record_icon_area.connect_draw(move |_, cr| {
            draw_record_icon(cr, rec_state_clone.load(Ordering::Relaxed));
            gtk::glib::Propagation::Proceed
        });

        let record_status_lbl = Label::new(Some("Not recording"));
        record_status_lbl.style_context().add_class("card-status");

        let record_sub_lbl = Label::new(Some(&format!("{} FPS • {} Mbps", config.fps, config.record_bitrate_kbps / 1000)));
        record_sub_lbl.style_context().add_class("card-sub");

        record_card_inner.pack_start(&record_title_lbl, false, false, 0);
        record_card_inner.pack_start(&record_icon_area, true, true, 4);
        record_card_inner.pack_start(&record_status_lbl, false, false, 0);
        record_card_inner.pack_start(&record_sub_lbl, false, false, 2);
        record_card_btn.add(&record_card_inner);

        // Record Attached Dropdown Menu
        let record_revealer = Revealer::new();
        record_revealer.set_transition_type(RevealerTransitionType::SlideDown);
        record_revealer.set_transition_duration(150);

        let record_menu_box = Box::new(Orientation::Vertical, 2);
        record_menu_box.style_context().add_class("dropdown-menu");

        let record_toggle_item = Button::with_label("Start (Ctrl+Shift+F9)");
        record_toggle_item.style_context().add_class("dropdown-item");

        let record_settings_item = Button::with_label("Settings");
        record_settings_item.style_context().add_class("dropdown-item");

        record_menu_box.pack_start(&record_toggle_item, false, false, 0);
        record_menu_box.pack_start(&record_settings_item, false, false, 0);
        record_revealer.add(&record_menu_box);

        record_col.pack_start(&record_card_btn, false, false, 0);
        record_col.pack_start(&record_revealer, false, false, 0);

        // -------------------------------------------------------------------------
        // CARD 3: SETTINGS (Direct Click to Settings Page)
        // -------------------------------------------------------------------------
        let settings_col = Box::new(Orientation::Vertical, 0);
        settings_col.set_size_request(215, -1);

        let settings_card_btn = Button::new();
        settings_card_btn.style_context().add_class("card-btn");
        settings_card_btn.set_size_request(215, 180);

        let settings_card_inner = Box::new(Orientation::Vertical, 6);
        let settings_title_lbl = Label::new(Some("SETTINGS"));
        settings_title_lbl.style_context().add_class("card-title");

        let settings_icon_area = DrawingArea::new();
        settings_icon_area.set_size_request(56, 56);
        settings_icon_area.connect_draw(|_, cr| {
            draw_gear_icon(cr);
            gtk::glib::Propagation::Proceed
        });

        let settings_status_lbl = Label::new(Some("Preferences"));
        settings_status_lbl.style_context().add_class("card-status");

        let settings_sub_lbl = Label::new(Some("Audio, Quality & Hotkeys"));
        settings_sub_lbl.style_context().add_class("card-sub");

        settings_card_inner.pack_start(&settings_title_lbl, false, false, 0);
        settings_card_inner.pack_start(&settings_icon_area, true, true, 4);
        settings_card_inner.pack_start(&settings_status_lbl, false, false, 0);
        settings_card_inner.pack_start(&settings_sub_lbl, false, false, 2);
        settings_card_btn.add(&settings_card_inner);

        settings_col.pack_start(&settings_card_btn, false, false, 0);

        // Pack the 3 columns
        cards_box.pack_start(&replay_col, false, false, 0);
        cards_box.pack_start(&record_col, false, false, 0);
        cards_box.pack_start(&settings_col, false, false, 0);

        hud_page.pack_start(&header_box, false, false, 0);
        hud_page.pack_start(&cards_box, true, true, 0);

        // =========================================================================
        // PAGE 2: UNIFIED SETTINGS PANEL (Containing Mouse Cursor, Audio, Video, Storage)
        // =========================================================================
        let settings_page = Box::new(Orientation::Vertical, 10);
        settings_page.style_context().add_class("settings-panel");
        settings_page.set_size_request(740, -1);

        // Settings Header Bar with Back Button
        let settings_header = Box::new(Orientation::Horizontal, 12);
        let back_btn = Button::with_label("< Back to Overlay");
        back_btn.style_context().add_class("back-btn");

        let settings_page_title = Label::new(Some("RECORDER SETTINGS & HARDWARE TUNING"));
        settings_page_title.style_context().add_class("settings-title");

        let settings_spacer = Box::new(Orientation::Horizontal, 0);

        let settings_close_btn = Button::with_label("✕");
        settings_close_btn.style_context().add_class("close-btn");

        settings_header.pack_start(&back_btn, false, false, 0);
        settings_header.pack_start(&settings_page_title, false, false, 8);
        settings_header.pack_start(&settings_spacer, true, true, 0);
        settings_header.pack_start(&settings_close_btn, false, false, 0);

        // Scrolled Settings Body
        let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(420);
        scroll.set_max_content_height(460);

        let settings_body = Box::new(Orientation::Vertical, 12);
        settings_body.set_margin_start(6);
        settings_body.set_margin_end(6);

        // -------------------------------------------------------------------------
        // SECTION 1: DISPLAY & CAPTURE (Mouse Cursor toggle prominently inside)
        // -------------------------------------------------------------------------
        let sec1_lbl = Label::new(Some("DISPLAY & CAPTURE"));
        sec1_lbl.style_context().add_class("section-header");
        sec1_lbl.set_halign(gtk::Align::Start);
        settings_body.pack_start(&sec1_lbl, false, false, 0);

        let sec1_grid = gtk::Grid::new();
        sec1_grid.set_column_spacing(18);
        sec1_grid.set_row_spacing(8);

        // 1.1 Mouse Cursor Checkbox
        let cursor_lbl = Label::new(Some("Mouse Cursor:"));
        cursor_lbl.set_halign(gtk::Align::Start);
        let cursor_box = Box::new(Orientation::Horizontal, 10);
        let cursor_check = CheckButton::with_label("Record Mouse Cursor");
        cursor_check.set_active(config.show_cursor);
        let cursor_desc = Label::new(Some("(Include mouse pointer in video & replays)"));
        cursor_desc.style_context().add_class("sub-info-label");
        cursor_box.pack_start(&cursor_check, false, false, 0);
        cursor_box.pack_start(&cursor_desc, false, false, 0);

        // 1.2 Target Framerate (FPS)
        let fps_lbl = Label::new(Some("Target Framerate:"));
        fps_lbl.set_halign(gtk::Align::Start);
        let fps_box = Box::new(Orientation::Horizontal, 6);
        let fps_spin = SpinButton::with_range(15.0, 240.0, 1.0);
        fps_spin.set_value(config.fps as f64);
        fps_box.pack_start(&fps_spin, false, false, 0);
        for &fps_val in &[30, 60, 120, 144] {
            let p_btn = Button::with_label(&format!("{} FPS", fps_val));
            p_btn.style_context().add_class("preset-btn");
            let s_clone = fps_spin.clone();
            p_btn.connect_clicked(move |_| s_clone.set_value(fps_val as f64));
            fps_box.pack_start(&p_btn, false, false, 0);
        }

        // 1.3 Video Bitrate (Mbps)
        let bit_lbl = Label::new(Some("Video Bitrate:"));
        bit_lbl.set_halign(gtk::Align::Start);
        let bit_box = Box::new(Orientation::Horizontal, 6);
        let bit_spin = SpinButton::with_range(2.0, 100.0, 1.0);
        bit_spin.set_value((config.record_bitrate_kbps / 1000) as f64);
        bit_box.pack_start(&bit_spin, false, false, 0);
        for &bit_val in &[10, 20, 30, 50] {
            let p_btn = Button::with_label(&format!("{} Mbps", bit_val));
            p_btn.style_context().add_class("preset-btn");
            let s_clone = bit_spin.clone();
            p_btn.connect_clicked(move |_| s_clone.set_value(bit_val as f64));
            bit_box.pack_start(&p_btn, false, false, 0);
        }

        // 1.4 Video Codec
        let codec_lbl = Label::new(Some("Video Codec:"));
        codec_lbl.set_halign(gtk::Align::Start);
        let codec_combo = ComboBoxText::new();
        codec_combo.append(Some("h264"), "H.264 / AVC (NVENC / VAAPI - Universal)");
        codec_combo.append(Some("hevc"), "HEVC / H.265 (High Efficiency)");
        codec_combo.append(Some("av1"), "AV1 (Next-Generation High Fidelity)");
        codec_combo.set_active_id(Some(&config.video_codec));

        sec1_grid.attach(&cursor_lbl, 0, 0, 1, 1);
        sec1_grid.attach(&cursor_box, 1, 0, 1, 1);
        sec1_grid.attach(&fps_lbl, 0, 1, 1, 1);
        sec1_grid.attach(&fps_box, 1, 1, 1, 1);
        sec1_grid.attach(&bit_lbl, 0, 2, 1, 1);
        sec1_grid.attach(&bit_box, 1, 2, 1, 1);
        sec1_grid.attach(&codec_lbl, 0, 3, 1, 1);
        sec1_grid.attach(&codec_combo, 1, 3, 1, 1);
        settings_body.pack_start(&sec1_grid, false, false, 0);

        // -------------------------------------------------------------------------
        // SECTION 2: INSTANT REPLAY BUFFER
        // -------------------------------------------------------------------------
        let sec2_lbl = Label::new(Some("INSTANT REPLAY BUFFER"));
        sec2_lbl.style_context().add_class("section-header");
        sec2_lbl.set_halign(gtk::Align::Start);
        settings_body.pack_start(&sec2_lbl, false, false, 4);

        let sec2_grid = gtk::Grid::new();
        sec2_grid.set_column_spacing(18);
        sec2_grid.set_row_spacing(8);

        let dur_lbl = Label::new(Some("Buffer Length:"));
        dur_lbl.set_halign(gtk::Align::Start);
        let dur_box = Box::new(Orientation::Horizontal, 6);
        let dur_spin = SpinButton::with_range(5.0, 1800.0, 5.0);
        dur_spin.set_value(config.replay_duration_sec as f64);
        dur_box.pack_start(&dur_spin, false, false, 0);
        for &(dur_val, dur_txt) in &[(15, "15s"), (30, "30s"), (60, "60s"), (120, "2m"), (300, "5m")] {
            let p_btn = Button::with_label(dur_txt);
            p_btn.style_context().add_class("preset-btn");
            let s_clone = dur_spin.clone();
            p_btn.connect_clicked(move |_| s_clone.set_value(dur_val as f64));
            dur_box.pack_start(&p_btn, false, false, 0);
        }

        sec2_grid.attach(&dur_lbl, 0, 0, 1, 1);
        sec2_grid.attach(&dur_box, 1, 0, 1, 1);
        settings_body.pack_start(&sec2_grid, false, false, 0);

        // -------------------------------------------------------------------------
        // SECTION 3: AUDIO & SOUND ROUTING
        // -------------------------------------------------------------------------
        let sec3_lbl = Label::new(Some("AUDIO & SOUND ROUTING"));
        sec3_lbl.style_context().add_class("section-header");
        sec3_lbl.set_halign(gtk::Align::Start);
        settings_body.pack_start(&sec3_lbl, false, false, 4);

        let sec3_grid = gtk::Grid::new();
        sec3_grid.set_column_spacing(18);
        sec3_grid.set_row_spacing(8);

        let audio_mode_lbl = Label::new(Some("Audio Mode:"));
        audio_mode_lbl.set_halign(gtk::Align::Start);
        let audio_mode_combo = ComboBoxText::new();
        audio_mode_combo.append(Some("system"), "System Sounds Only (Game / Desktop)");
        audio_mode_combo.append(Some("mic"), "Microphone Only");
        audio_mode_combo.append(Some("both"), "Both Combined (System Sounds + Microphone)");
        audio_mode_combo.append(Some("muted"), "Muted (No Audio Recording)");
        audio_mode_combo.set_active_id(Some(&config.audio_mode));

        let audio_dev_lbl = Label::new(Some("Input Device:"));
        audio_dev_lbl.set_halign(gtk::Align::Start);
        let audio_dev_combo = ComboBoxText::new();
        audio_dev_combo.append(Some("default"), "Default Recording Device");
        for dev in crate::capture::audio::list_input_devices() {
            audio_dev_combo.append(Some(&dev), &dev);
        }
        audio_dev_combo.set_active_id(Some(&config.audio_device));

        let sys_vol_lbl = Label::new(Some("System Volume:"));
        sys_vol_lbl.set_halign(gtk::Align::Start);
        let sys_vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 150.0, 5.0);
        sys_vol_scale.set_value((config.system_volume * 100.0).round() as f64);
        sys_vol_scale.set_size_request(240, -1);

        let mic_vol_lbl = Label::new(Some("Mic Volume:"));
        mic_vol_lbl.set_halign(gtk::Align::Start);
        let mic_vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 150.0, 5.0);
        mic_vol_scale.set_value((config.mic_volume * 100.0).round() as f64);
        mic_vol_scale.set_size_request(240, -1);

        sec3_grid.attach(&audio_mode_lbl, 0, 0, 1, 1);
        sec3_grid.attach(&audio_mode_combo, 1, 0, 1, 1);
        sec3_grid.attach(&audio_dev_lbl, 0, 1, 1, 1);
        sec3_grid.attach(&audio_dev_combo, 1, 1, 1, 1);
        sec3_grid.attach(&sys_vol_lbl, 0, 2, 1, 1);
        sec3_grid.attach(&sys_vol_scale, 1, 2, 1, 1);
        sec3_grid.attach(&mic_vol_lbl, 0, 3, 1, 1);
        sec3_grid.attach(&mic_vol_scale, 1, 3, 1, 1);
        settings_body.pack_start(&sec3_grid, false, false, 0);

        // -------------------------------------------------------------------------
        // SECTION 4: STORAGE & SHORTCUTS
        // -------------------------------------------------------------------------
        let sec4_lbl = Label::new(Some("STORAGE & SHORTCUTS"));
        sec4_lbl.style_context().add_class("section-header");
        sec4_lbl.set_halign(gtk::Align::Start);
        settings_body.pack_start(&sec4_lbl, false, false, 4);

        let sec4_grid = gtk::Grid::new();
        sec4_grid.set_column_spacing(18);
        sec4_grid.set_row_spacing(8);

        let dir_lbl = Label::new(Some("Save Folder:"));
        dir_lbl.set_halign(gtk::Align::Start);
        let dir_box = Box::new(Orientation::Horizontal, 8);
        let dir_entry = Entry::new();
        dir_entry.set_text(&config.output_directory);
        dir_entry.set_size_request(280, -1);
        let dir_browse_btn = Button::with_label("Browse...");
        dir_browse_btn.style_context().add_class("preset-btn");

        let dir_entry_clone = dir_entry.clone();
        dir_browse_btn.connect_clicked(move |_| {
            let cur_path = dir_entry_clone.text().to_string();
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            let de = dir_entry_clone.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                if let Ok(sel) = rx.try_recv() {
                    de.set_text(&sel);
                    gtk::glib::ControlFlow::Break
                } else {
                    gtk::glib::ControlFlow::Continue
                }
            });
            std::thread::spawn(move || {
                if let Ok(out) = std::process::Command::new("kdialog")
                    .args(["--getexistingdirectory", &cur_path])
                    .output()
                {
                    let sel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !sel.is_empty() {
                        let _ = tx.send(sel);
                    }
                }
            });
        });

        dir_box.pack_start(&dir_entry, true, true, 0);
        dir_box.pack_start(&dir_browse_btn, false, false, 0);

        let save_hk_lbl = Label::new(Some("Save Replay Hotkey:"));
        save_hk_lbl.set_halign(gtk::Align::Start);
        let save_hk_entry = Entry::new();
        save_hk_entry.set_text(&config.save_hotkey);

        let rec_hk_lbl = Label::new(Some("Toggle Record Hotkey:"));
        rec_hk_lbl.set_halign(gtk::Align::Start);
        let rec_hk_entry = Entry::new();
        rec_hk_entry.set_text(&config.record_hotkey);

        let menu_hk_lbl = Label::new(Some("Overlay Menu Hotkey:"));
        menu_hk_lbl.set_halign(gtk::Align::Start);
        let menu_hk_entry = Entry::new();
        menu_hk_entry.set_text(&config.menu_hotkey);

        let cur_hk_lbl = Label::new(Some("Toggle Cursor Hotkey:"));
        cur_hk_lbl.set_halign(gtk::Align::Start);
        let cur_hk_entry = Entry::new();
        cur_hk_entry.set_text(&config.cursor_hotkey);

        let auto_lbl = Label::new(Some("Autostart on Login:"));
        auto_lbl.set_halign(gtk::Align::Start);
        let auto_switch = Switch::new();
        auto_switch.set_active(config.autostart);

        sec4_grid.attach(&dir_lbl, 0, 0, 1, 1);
        sec4_grid.attach(&dir_box, 1, 0, 1, 1);
        sec4_grid.attach(&save_hk_lbl, 0, 1, 1, 1);
        sec4_grid.attach(&save_hk_entry, 1, 1, 1, 1);
        sec4_grid.attach(&rec_hk_lbl, 0, 2, 1, 1);
        sec4_grid.attach(&rec_hk_entry, 1, 2, 1, 1);
        sec4_grid.attach(&menu_hk_lbl, 0, 3, 1, 1);
        sec4_grid.attach(&menu_hk_entry, 1, 3, 1, 1);
        sec4_grid.attach(&cur_hk_lbl, 0, 4, 1, 1);
        sec4_grid.attach(&cur_hk_entry, 1, 4, 1, 1);
        sec4_grid.attach(&auto_lbl, 0, 5, 1, 1);
        sec4_grid.attach(&auto_switch, 1, 5, 1, 1);
        settings_body.pack_start(&sec4_grid, false, false, 0);

        scroll.add(&settings_body);

        // Apply & Save Settings Button
        let apply_btn = Button::with_label("Apply & Save Settings");
        apply_btn.style_context().add_class("apply-save-btn");
        apply_btn.set_size_request(-1, 42);

        settings_page.pack_start(&settings_header, false, false, 0);
        settings_page.pack_start(&scroll, true, true, 4);
        settings_page.pack_start(&apply_btn, false, false, 4);

        // Add Pages to Stack
        stack.add_named(&hud_page, "hud");
        stack.add_named(&settings_page, "settings");
        window.add(&stack);

        // =========================================================================
        // CSS STYLING (Translucent NVIDIA Frosted Glass Theme)
        // =========================================================================
        let css_provider = CssProvider::new();
        let css = r#"
            window {
                background-color: transparent !important;
                background: transparent !important;
            }
            .hud-wrapper {
                background-color: transparent;
                padding: 0px;
            }
            .header-bar {
                background-color: rgba(10, 14, 20, 0.88);
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 10px;
                padding: 6px 14px;
                box-shadow: 0px 8px 24px rgba(0, 0, 0, 0.6);
            }
            .vrec-badge {
                background-color: #76b900;
                color: #0b1204;
                font-size: 11px;
                font-weight: 900;
                border-radius: 4px;
                padding: 2px 7px;
                letter-spacing: 0.5px;
            }
            .overlay-title {
                color: #f1f5f9;
                font-size: 12px;
                font-weight: 800;
                letter-spacing: 1px;
            }
            .status-banner {
                color: #76b900;
                font-size: 11.5px;
                font-weight: 700;
            }
            .close-btn {
                background-color: transparent;
                color: #94a3b8;
                border: none;
                font-size: 14px;
                font-weight: 700;
                padding: 2px 8px;
            }
            .close-btn:hover {
                color: #ef4444;
            }
            /* Square Action Cards */
            .card-btn {
                background-color: rgba(14, 18, 26, 0.85);
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 14px;
                padding: 14px;
                box-shadow: 0px 12px 36px rgba(0, 0, 0, 0.7);
            }
            .card-btn:hover {
                background-color: rgba(10, 14, 20, 0.98);
                border-color: #76b900;
                box-shadow: 0 0 16px rgba(118, 185, 0, 0.4);
            }
            .card-btn-active {
                background-color: rgba(10, 14, 20, 0.98);
                border-color: #76b900;
                border-bottom-left-radius: 0px;
                border-bottom-right-radius: 0px;
                box-shadow: 0 0 16px rgba(118, 185, 0, 0.4);
            }
            .card-title {
                color: #ffffff;
                font-size: 11.5px;
                font-weight: 800;
                letter-spacing: 0.8px;
            }
            .card-status {
                color: #94a3b8;
                font-size: 11px;
                font-weight: 700;
            }
            .status-green {
                color: #76b900 !important;
            }
            .status-red {
                color: #ef4444 !important;
            }
            .card-sub {
                color: #64748b;
                font-size: 10px;
                font-weight: 500;
            }
            /* Attached Dropdown Menus */
            .dropdown-menu {
                background-color: rgba(10, 14, 20, 0.98);
                border: 1px solid rgba(255, 255, 255, 0.14);
                border-top: none;
                border-bottom-left-radius: 12px;
                border-bottom-right-radius: 12px;
                padding: 6px;
                box-shadow: 0px 16px 36px rgba(0, 0, 0, 0.85);
            }
            .dropdown-item {
                background-color: transparent;
                color: #e2e8f0;
                font-size: 12px;
                font-weight: 700;
                border-radius: 6px;
                border: 1px solid transparent;
                padding: 8px 12px;
            }
            .dropdown-item:hover {
                background-color: rgba(255, 255, 255, 0.08);
                border-color: #76b900;
                color: #76b900;
            }
            .settings-panel {
                background-color: rgba(14, 18, 26, 0.96);
                border: 1px solid rgba(255, 255, 255, 0.14);
                border-radius: 14px;
                padding: 16px 22px;
                box-shadow: 0px 16px 48px rgba(0, 0, 0, 0.85);
            }
            .settings-title {
                color: #ffffff;
                font-size: 13px;
                font-weight: 800;
                letter-spacing: 1px;
            }
            .back-btn {
                background-color: transparent;
                color: #76b900;
                font-size: 12px;
                font-weight: 700;
                border: 1px solid rgba(118, 185, 0, 0.4);
                border-radius: 6px;
                padding: 4px 10px;
            }
            .back-btn:hover {
                background-color: rgba(118, 185, 0, 0.15);
                color: #8ce000;
            }
            .section-header {
                color: #76b900;
                font-size: 11px;
                font-weight: 800;
                letter-spacing: 1px;
            }
            .preset-btn {
                background-color: rgba(255, 255, 255, 0.06);
                border: 1px solid rgba(255, 255, 255, 0.1);
                border-radius: 6px;
                color: #cbd5e1;
                font-size: 11px;
                font-weight: 600;
                padding: 3px 8px;
            }
            .preset-btn:hover {
                background-color: rgba(118, 185, 0, 0.2);
                border-color: #76b900;
                color: #ffffff;
            }
            .apply-save-btn {
                background-color: #76b900;
                color: #0b1204;
                font-size: 13px;
                font-weight: 800;
                border-radius: 8px;
                border: none;
                padding: 8px 14px;
            }
            .apply-save-btn:hover {
                background-color: #8ce000;
            }
            checkbutton check {
                background-color: #1a2230;
                border: 1px solid rgba(255, 255, 255, 0.2);
                border-radius: 4px;
            }
            checkbutton check:checked {
                background-color: #76b900;
                border-color: #8ce000;
            }
            checkbutton label {
                color: #ffffff;
                font-size: 12px;
                font-weight: 700;
            }
            entry, spinbutton, combobox button {
                background-color: #161c28;
                color: #f1f5f9;
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 6px;
                padding: 5px 8px;
                font-size: 12px;
                font-weight: 600;
            }
            entry:focus, spinbutton:focus, combobox button:focus {
                border-color: #76b900;
            }
            scale highlight {
                background-color: #76b900;
                border-radius: 3px;
            }
            scale trough {
                background-color: #1c2432;
                border-radius: 3px;
            }
            scale slider {
                background-color: #ffffff;
                border-radius: 50%;
                min-width: 14px;
                min-height: 14px;
            }
            .sub-info-label {
                color: #64748b;
                font-size: 10.5px;
                font-weight: 500;
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

        // =========================================================================
        // EVENT HANDLERS & NAVIGATION
        // =========================================================================
        let w_close1 = window.clone();
        close_btn.connect_clicked(move |_| w_close1.close());
        let w_close2 = window.clone();
        settings_close_btn.connect_clicked(move |_| w_close2.close());

        let w_esc = window.clone();
        window.connect_key_press_event(move |_, key| {
            if key.keyval() == gdk::keys::constants::Escape {
                w_esc.close();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });

        // -------------------------------------------------------------------------
        // CARD INTERACTION: TOGGLE ATTACHED DROPDOWNS
        // -------------------------------------------------------------------------
        // 1. Replay Card Click -> Toggle Replay Dropdown
        let rev_r_click = replay_revealer.clone();
        let rev_rec_close = record_revealer.clone();
        let card_r_btn = replay_card_btn.clone();
        let card_rec_btn = record_card_btn.clone();
        replay_card_btn.connect_clicked(move |_| {
            let next_rev = !rev_r_click.reveals_child();
            rev_r_click.set_reveal_child(next_rev);
            rev_rec_close.set_reveal_child(false);
            card_rec_btn.style_context().remove_class("card-btn-active");
            if next_rev {
                card_r_btn.style_context().add_class("card-btn-active");
            } else {
                card_r_btn.style_context().remove_class("card-btn-active");
            }
        });

        // 2. Record Card Click -> Toggle Record Dropdown
        let rev_rec_click = record_revealer.clone();
        let rev_r_close = replay_revealer.clone();
        let card_rec_btn2 = record_card_btn.clone();
        let card_r_btn2 = replay_card_btn.clone();
        record_card_btn.connect_clicked(move |_| {
            let next_rev = !rev_rec_click.reveals_child();
            rev_rec_click.set_reveal_child(next_rev);
            rev_r_close.set_reveal_child(false);
            card_r_btn2.style_context().remove_class("card-btn-active");
            if next_rev {
                card_rec_btn2.style_context().add_class("card-btn-active");
            } else {
                card_rec_btn2.style_context().remove_class("card-btn-active");
            }
        });

        // 3. Settings Card Click -> Navigate directly to Settings Page
        let stack_to_settings = stack.clone();
        let w_to_settings = window.clone();
        settings_card_btn.connect_clicked(move |_| {
            stack_to_settings.set_visible_child_name("settings");
            w_to_settings.resize(740, 560);
        });

        // Back to HUD from Settings
        let stack_to_hud = stack.clone();
        let w_to_hud = window.clone();
        back_btn.connect_clicked(move |_| {
            stack_to_hud.set_visible_child_name("hud");
            w_to_hud.resize(740, 280);
        });

        // Replay Menu Items:
        // Item 1: Turn on / Turn off Replay
        let r_toggle_lbl = replay_toggle_item.clone();
        let r_status_lbl_sync = replay_status_lbl.clone();
        let r_state_sync = Arc::clone(&replay_active_state);
        let r_icon_sync = replay_icon_area.clone();
        let rev_r_hide = replay_revealer.clone();
        let card_r_deactivate = replay_card_btn.clone();
        replay_toggle_item.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            cfg.replay_enabled = !cfg.replay_enabled;
            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();

            r_state_sync.store(cfg.replay_enabled, Ordering::Relaxed);
            r_icon_sync.queue_draw();

            if cfg.replay_enabled {
                r_toggle_lbl.set_label("Turn off");
                r_status_lbl_sync.set_label("On");
                r_status_lbl_sync.style_context().add_class("status-green");
            } else {
                r_toggle_lbl.set_label("Turn on");
                r_status_lbl_sync.set_label("Off");
                r_status_lbl_sync.style_context().remove_class("status-green");
            }
            rev_r_hide.set_reveal_child(false);
            card_r_deactivate.style_context().remove_class("card-btn-active");
        });

        // Item 2: Save Replay
        let banner_save = status_banner.clone();
        let rev_r_hide2 = replay_revealer.clone();
        let card_r_deactivate2 = replay_card_btn.clone();
        replay_save_item.connect_clicked(move |_| {
            let _ = ipc::send_command(Command::SaveReplay);
            banner_save.set_text("Replay saved successfully!");
            let b_clear = banner_save.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(3000), move || {
                b_clear.set_text("");
                gtk::glib::ControlFlow::Break
            });
            rev_r_hide2.set_reveal_child(false);
            card_r_deactivate2.style_context().remove_class("card-btn-active");
        });

        // Item 3: Replay Settings
        let stack_r_settings = stack.clone();
        let w_r_settings = window.clone();
        let rev_r_hide3 = replay_revealer.clone();
        let card_r_deactivate3 = replay_card_btn.clone();
        replay_settings_item.connect_clicked(move |_| {
            rev_r_hide3.set_reveal_child(false);
            card_r_deactivate3.style_context().remove_class("card-btn-active");
            stack_r_settings.set_visible_child_name("settings");
            w_r_settings.resize(740, 560);
        });

        // Record Menu Items:
        // Item 1: Start / Stop Record
        let rev_rec_hide = record_revealer.clone();
        let card_rec_deactivate = record_card_btn.clone();
        record_toggle_item.connect_clicked(move |_| {
            let _ = ipc::send_command(Command::ToggleRecording);
            rev_rec_hide.set_reveal_child(false);
            card_rec_deactivate.style_context().remove_class("card-btn-active");
        });

        // Item 2: Record Settings
        let stack_rec_settings = stack.clone();
        let w_rec_settings = window.clone();
        let rev_rec_hide2 = record_revealer.clone();
        let card_rec_deactivate2 = record_card_btn.clone();
        record_settings_item.connect_clicked(move |_| {
            rev_rec_hide2.set_reveal_child(false);
            card_rec_deactivate2.style_context().remove_class("card-btn-active");
            stack_rec_settings.set_visible_child_name("settings");
            w_rec_settings.resize(740, 560);
        });

        // Direct Mouse Cursor Checkbox Toggle
        let cursor_check_sync = cursor_check.clone();
        let banner_cur = status_banner.clone();
        cursor_check.connect_toggled(move |c| {
            let active = c.is_active();
            let mut cfg = VrecConfig::load();
            if cfg.show_cursor != active {
                cfg.show_cursor = active;
                let _ = cfg.save();
                let _ = ipc::send_command(Command::ToggleCursor);
                banner_cur.set_text(if active { "Cursor: Visible in recording" } else { "Cursor: Hidden from recording" });
                let b_clear = banner_cur.clone();
                gtk::glib::timeout_add_local(Duration::from_millis(3000), move || {
                    b_clear.set_text("");
                    gtk::glib::ControlFlow::Break
                });
            }
        });

        // Apply & Save Settings
        let stack_after_save = stack.clone();
        let w_after_save = window.clone();
        let banner_after_save = status_banner.clone();
        let r_sub_lbl_sync = replay_sub_lbl.clone();
        let rec_sub_lbl_sync = record_sub_lbl.clone();

        apply_btn.connect_clicked(move |_| {
            let mut cfg = VrecConfig::load();
            cfg.show_cursor = cursor_check_sync.is_active();
            cfg.fps = fps_spin.value() as u32;
            let bit_mbps = bit_spin.value() as u32;
            cfg.record_bitrate_kbps = bit_mbps * 1000;
            cfg.replay_bitrate_kbps = bit_mbps * 1000;
            if let Some(c) = codec_combo.active_id() {
                cfg.video_codec = c.to_string();
            }
            cfg.replay_duration_sec = dur_spin.value() as u32;
            if let Some(m) = audio_mode_combo.active_id() {
                cfg.audio_mode = m.to_string();
            }
            if let Some(dev) = audio_dev_combo.active_id() {
                cfg.audio_device = dev.to_string();
            }
            cfg.system_volume = (sys_vol_scale.value() / 100.0) as f32;
            cfg.mic_volume = (mic_vol_scale.value() / 100.0) as f32;

            let dir_val = dir_entry.text().to_string();
            if !dir_val.trim().is_empty() {
                cfg.output_directory = dir_val;
            }
            cfg.save_hotkey = save_hk_entry.text().to_string();
            cfg.record_hotkey = rec_hk_entry.text().to_string();
            cfg.menu_hotkey = menu_hk_entry.text().to_string();
            cfg.cursor_hotkey = cur_hk_entry.text().to_string();
            cfg.autostart = auto_switch.is_active();

            let _ = cfg.save();
            VrecConfig::notify_daemon_reload();

            r_sub_lbl_sync.set_text(&format!("{}s Buffer", cfg.replay_duration_sec));
            rec_sub_lbl_sync.set_text(&format!("{} FPS • {} Mbps", cfg.fps, cfg.record_bitrate_kbps / 1000));

            banner_after_save.set_text("Settings saved!");
            let b_clear = banner_after_save.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(3000), move || {
                b_clear.set_text("");
                gtk::glib::ControlFlow::Break
            });

            stack_after_save.set_visible_child_name("hud");
            w_after_save.resize(740, 280);
        });

        // =========================================================================
        // LIVE STATUS POLLER (Syncs Daemon States, Vectors & Timers Every 300ms)
        // =========================================================================
        let rec_status_sync = record_status_lbl.clone();
        let rec_toggle_item_sync = record_toggle_item.clone();
        let rec_state_sync2 = Arc::clone(&record_active_state);
        let rec_icon_sync = record_icon_area.clone();

        let r_status_sync2 = replay_status_lbl.clone();
        let r_toggle_item_sync2 = replay_toggle_item.clone();
        let r_state_sync2 = Arc::clone(&replay_active_state);
        let r_icon_sync2 = replay_icon_area.clone();

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = Arc::clone(&is_running);

        gtk::glib::timeout_add_local(Duration::from_millis(300), move || {
            if !is_running_clone.load(Ordering::Relaxed) {
                return gtk::glib::ControlFlow::Break;
            }

            if let Ok(st) = ipc::query_status() {
                // 1. Recording status sync
                let prev_rec = rec_state_sync2.swap(st.is_recording, Ordering::Relaxed);
                if st.is_recording {
                    let mins = st.recording_duration_sec / 60;
                    let secs = st.recording_duration_sec % 60;
                    rec_status_sync.set_label(&format!("Recording {:02}:{:02}", mins, secs));
                    rec_status_sync.style_context().add_class("status-red");
                    rec_toggle_item_sync.set_label("Stop (Ctrl+Shift+F9)");
                } else {
                    rec_status_sync.set_label("Not recording");
                    rec_status_sync.style_context().remove_class("status-red");
                    rec_toggle_item_sync.set_label("Start (Ctrl+Shift+F9)");
                }
                if prev_rec != st.is_recording {
                    rec_icon_sync.queue_draw();
                }

                // 2. Replay status sync
                let prev_replay = r_state_sync2.swap(st.is_replay_active, Ordering::Relaxed);
                if st.is_replay_active {
                    r_status_sync2.set_label("On");
                    r_status_sync2.style_context().add_class("status-green");
                    r_toggle_item_sync2.set_label("Turn off");
                } else {
                    r_status_sync2.set_label("Off");
                    r_status_sync2.style_context().remove_class("status-green");
                    r_toggle_item_sync2.set_label("Turn on");
                }
                if prev_replay != st.is_replay_active {
                    r_icon_sync2.queue_draw();
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
