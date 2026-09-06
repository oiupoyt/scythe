use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, Box, Button, ComboBoxText, CssProvider,
    DrawingArea, Entry, Fixed, Label, LevelBar, Orientation, Revealer, RevealerTransitionType, Scale,
    ScrolledWindow, SpinButton, Stack, StackTransitionType, StyleContext, Switch,
};
#[cfg(target_os = "linux")]
use gtk_layer_shell::{Layer, LayerShell};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::config::ScytheConfig;
use crate::ipc::{self, Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastIcon {
    Replay,
    Record,
    Save,
    Cursor,
    Info,
    Error,
}

impl ToastIcon {
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "replay" | "save_replay" => ToastIcon::Replay,
            "record" | "recording" | "start" => ToastIcon::Record,
            "save" | "saved" | "stop" => ToastIcon::Save,
            "cursor" => ToastIcon::Cursor,
            "error" => ToastIcon::Error,
            _ => ToastIcon::Info,
        }
    }
}

pub fn spawn_toast(title: &str, subtitle: &str, icon: ToastIcon) {
    let icon_str = match icon {
        ToastIcon::Replay => "replay",
        ToastIcon::Record => "record",
        ToastIcon::Save => "save",
        ToastIcon::Cursor => "cursor",
        ToastIcon::Error => "error",
        ToastIcon::Info => "info",
    };

    let title_owned = title.to_string();
    let subtitle_owned = subtitle.to_string();
    let icon_owned = icon_str.to_string();

    std::thread::spawn(move || {
        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("scythe-ui"));
        let _ = std::process::Command::new(exe)
            .args(["--toast", &title_owned, &subtitle_owned, &icon_owned])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });
}

pub fn show_notification_overlay() {
    show_shadowplay_toast("INSTANT REPLAY", "Saved to Videos", ToastIcon::Replay);
}

pub fn show_notification(message: &str) {
    let lower = message.to_lowercase();
    let (title, subtitle, icon) = if lower.contains("replay saved") || lower.contains("replay") {
        ("INSTANT REPLAY", "Saved to Videos", ToastIcon::Replay)
    } else if lower.contains("recording started") {
        ("RECORDING", "Recording started", ToastIcon::Record)
    } else if lower.contains("recording saved") || lower.contains("stopped") {
        ("RECORDING", "Recording saved", ToastIcon::Save)
    } else if lower.contains("cursor") {
        ("MOUSE CURSOR", message, ToastIcon::Cursor)
    } else if lower.contains("error") {
        ("SCYTHE", message, ToastIcon::Error)
    } else {
        ("SCYTHE", message, ToastIcon::Info)
    };
    show_shadowplay_toast(title, subtitle, icon);
}

pub fn ensure_wayland_env() {
    #[cfg(target_os = "linux")]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        unsafe {
            if std::env::var("WAYLAND_DISPLAY").is_err()
                && let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("wayland-") && !name.ends_with(".lock") {
                            std::env::set_var("WAYLAND_DISPLAY", &name);
                            break;
                        }
                    }
            }
            if std::env::var("DISPLAY").is_err() && std::path::Path::new("/tmp/.X11-unix/X0").exists() {
                std::env::set_var("DISPLAY", ":0");
            }
            if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
                let bus_path = format!("{}/bus", runtime_dir);
                if std::path::Path::new(&bus_path).exists() {
                    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", bus_path));
                }
            }
            if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
                let hypr_dir = std::path::Path::new(&runtime_dir).join("hypr");
                if let Ok(entries) = std::fs::read_dir(&hypr_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            let sig = entry.file_name().to_string_lossy().to_string();
                            if !sig.is_empty() {
                                std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", &sig);
                                break;
                            }
                        }
                    }
                }
            }
            if std::env::var("XDG_CURRENT_DESKTOP").is_err() && std::env::var("WAYLAND_DISPLAY").is_ok() {
                std::env::set_var("XDG_CURRENT_DESKTOP", "Hyprland");
            }
            if std::env::var("XDG_SESSION_TYPE").map(|s| s == "tty" || s.is_empty()).unwrap_or(true) {
                if std::env::var("WAYLAND_DISPLAY").is_ok() {
                    std::env::set_var("XDG_SESSION_TYPE", "wayland");
                } else if std::env::var("DISPLAY").is_ok() {
                    std::env::set_var("XDG_SESSION_TYPE", "x11");
                }
            }
        }
    }
}

pub fn show_shadowplay_toast(title: &str, subtitle: &str, icon: ToastIcon) {
    #[cfg(target_os = "windows")]
    {
        spawn_toast(title, subtitle, icon);
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
        ensure_wayland_env();
        if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
            crate::overlay_egui::run_egui_toast(title, subtitle, icon);
            return;
        }
        if gtk::init().is_err() {
            crate::overlay_egui::run_egui_toast(title, subtitle, icon);
            return;
        }

        let app_id = format!("com.scythe.toast.p{}", std::process::id());
        let app = Application::builder()
            .application_id(&app_id)
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();

        let title_text = title.to_string();
        let sub_text = subtitle.to_string();

        let cfg = ScytheConfig::load();
        let accent_name = cfg.accent_color.to_lowercase();
        let (accent_hex, accent_rgb): (&str, (f64, f64, f64)) = match accent_name.as_str() {
            "green" | "emerald" => ("#22c55e", (0.133, 0.773, 0.369)),
            "cyan" | "ice" => ("#06b6d4", (0.024, 0.714, 0.831)),
            "purple" | "violet" => ("#a855f7", (0.659, 0.333, 0.969)),
            "amber" | "orange" => ("#f59e0b", (0.961, 0.620, 0.043)),
            "red" | "crimson" => ("#ef4444", (0.937, 0.267, 0.267)),
            "blue" | "sapphire" | _ => ("#38bdf8", (0.220, 0.741, 0.973)),
        };

        let (active_accent_hex, active_accent) = if icon == ToastIcon::Record {
            ("#ef4444", (0.937, 0.267, 0.267))
        } else {
            (accent_hex, accent_rgb)
        };

        app.connect_activate(move |app| {
            let window = ApplicationWindow::builder()
                .application(app)
                .default_width(340)
                .default_height(64)
                .build();

            #[cfg(target_os = "linux")]
            let layer_shell_ok = gtk_layer_shell::is_supported();
            #[cfg(not(target_os = "linux"))]
            let layer_shell_ok = false;

            if layer_shell_ok {
                window.init_layer_shell();
                window.set_layer(Layer::Overlay);
                window.set_namespace("scythe-notification");
                window.set_anchor(gtk_layer_shell::Edge::Top, true);
                window.set_anchor(gtk_layer_shell::Edge::Right, true);
                window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 24);
                window.set_layer_shell_margin(gtk_layer_shell::Edge::Right, 24);
                window.set_keyboard_interactivity(false);
            } else {
                window.set_decorated(false);
                window.set_keep_above(true);
                window.set_skip_taskbar_hint(true);
                window.set_accept_focus(false);
                if let Some(display) = gdk::Display::default()
                    && let Some(mon) = display.primary_monitor().or_else(|| display.monitor(0)) {
                        let geom = mon.geometry();
                        let x = geom.x() + geom.width() - 340 - 24;
                        let y = geom.y() + 24;
                        window.move_(x, y);
                }
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
                cr.set_operator(gtk::cairo::Operator::Over);
                gtk::glib::Propagation::Proceed
            });

            let css_provider = CssProvider::new();
            let css = format!(
                r#"
                window, window.background {{
                    background-color: transparent;
                    background: transparent;
                    border: none;
                    box-shadow: none;
                }}
                .toast-card {{
                    background-color: rgba(14, 16, 21, 0.96);
                    border: 1px solid rgba(255, 255, 255, 0.14);
                    border-left: 4px solid {accent};
                    border-radius: 0px;
                    box-shadow: 0px 8px 24px rgba(0, 0, 0, 0.75);
                }}
                "#,
                accent = active_accent_hex
            );
            let _ = css_provider.load_from_data(css.as_bytes());
            if let Some(screen) = gdk::Screen::default() {
                StyleContext::add_provider_for_screen(
                    &screen,
                    &css_provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            window.style_context().add_provider(&css_provider, gtk::STYLE_PROVIDER_PRIORITY_USER);

            let hbox = Box::new(Orientation::Horizontal, 10);
            hbox.style_context().add_class("toast-card");
            hbox.set_size_request(320, 56);
            hbox.set_app_paintable(true);

            hbox.connect_draw(move |widget, cr| {
                let w = widget.allocated_width() as f64;
                let h = widget.allocated_height() as f64;
                let card_h = (h - 4.0).max(10.0);

                cr.set_operator(gtk::cairo::Operator::Over);

                // 1. Drop shadow (soft dark shadow offset down by 4px)
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.60);
                cr.rectangle(0.0, 4.0, w, card_h);
                let _ = cr.fill();

                // 2. Solid obsidian dark slate background (matching egui rgba(14, 16, 21, 0.96))
                cr.set_source_rgba(14.0 / 255.0, 16.0 / 255.0, 21.0 / 255.0, 0.96);
                cr.rectangle(0.0, 0.0, w, card_h);
                let _ = cr.fill();

                // 3. Subtle white border (1px inside)
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
                cr.set_line_width(1.0);
                cr.rectangle(0.5, 0.5, w - 1.0, card_h - 1.0);
                let _ = cr.stroke();

                // 4. Left accent bar (4.0px width)
                cr.set_source_rgb(active_accent.0, active_accent.1, active_accent.2);
                cr.rectangle(0.0, 0.0, 4.0, card_h);
                let _ = cr.fill();

                gtk::glib::Propagation::Proceed
            });

            // Left DrawingArea for vector icon
            let icon_area = DrawingArea::new();
            icon_area.set_size_request(32, 32);
            icon_area.set_margin_start(14);
            icon_area.set_valign(gtk::Align::Center);
            let icon_type = icon;
            icon_area.connect_draw(move |_, cr| {
                let cx = 16.0;
                let cy = 16.0;
                match icon_type {
                    ToastIcon::Replay => {
                        let r = 10.0;
                        cr.set_source_rgb(accent_rgb.0, accent_rgb.1, accent_rgb.2);
                        cr.set_line_width(2.2);
                        cr.arc(cx, cy, r, 0.25 * PI, 1.80 * PI);
                        let _ = cr.stroke();

                        let a_x = cx + r * (0.25 * PI).cos();
                        let a_y = cy + r * (0.25 * PI).sin();
                        cr.move_to(a_x, a_y);
                        cr.line_to(a_x - 4.5, a_y + 0.5);
                        cr.line_to(a_x - 0.5, a_y - 4.5);
                        cr.close_path();
                        let _ = cr.fill();

                        let tri_r = 4.0;
                        cr.move_to(cx + tri_r + 0.5, cy);
                        cr.line_to(cx - tri_r * 0.6 + 0.5, cy - tri_r * 0.86);
                        cr.line_to(cx - tri_r * 0.6 + 0.5, cy + tri_r * 0.86);
                        cr.close_path();
                        let _ = cr.fill();
                    }
                    ToastIcon::Record => {
                        cr.set_source_rgba(0.937, 0.267, 0.267, 0.25);
                        cr.arc(cx, cy, 13.0, 0.0, PI * 2.0);
                        let _ = cr.fill();

                        cr.set_source_rgb(0.937, 0.267, 0.267);
                        cr.set_line_width(1.8);
                        cr.arc(cx, cy, 10.0, 0.0, PI * 2.0);
                        let _ = cr.stroke();

                        cr.arc(cx, cy, 5.0, 0.0, PI * 2.0);
                        let _ = cr.fill();
                    }
                    ToastIcon::Save => {
                        cr.set_source_rgb(accent_rgb.0, accent_rgb.1, accent_rgb.2);
                        cr.set_line_width(2.5);
                        cr.set_line_cap(gtk::cairo::LineCap::Round);
                        cr.set_line_join(gtk::cairo::LineJoin::Round);
                        cr.move_to(cx - 7.0, cy);
                        cr.line_to(cx - 2.0, cy + 5.0);
                        cr.line_to(cx + 7.0, cy - 5.0);
                        let _ = cr.stroke();
                    }
                    ToastIcon::Cursor => {
                        cr.set_source_rgb(accent_rgb.0, accent_rgb.1, accent_rgb.2);
                        cr.set_line_width(1.8);
                        cr.move_to(cx - 6.0, cy - 8.0);
                        cr.line_to(cx + 6.0, cy - 1.0);
                        cr.line_to(cx, cy + 1.0);
                        cr.line_to(cx + 2.0, cy + 7.0);
                        cr.line_to(cx - 1.0, cy + 8.0);
                        cr.line_to(cx - 3.0, cy + 2.0);
                        cr.line_to(cx - 6.0, cy + 4.0);
                        cr.close_path();
                        let _ = cr.fill();
                    }
                    ToastIcon::Error => {
                        cr.set_source_rgb(0.937, 0.267, 0.267);
                        cr.set_line_width(2.2);
                        cr.move_to(cx - 6.0, cy - 6.0);
                        cr.line_to(cx + 6.0, cy + 6.0);
                        cr.move_to(cx + 6.0, cy - 6.0);
                        cr.line_to(cx - 6.0, cy + 6.0);
                        let _ = cr.stroke();
                    }
                    ToastIcon::Info => {
                        cr.set_source_rgb(accent_rgb.0, accent_rgb.1, accent_rgb.2);
                        cr.arc(cx, cy, 6.0, 0.0, PI * 2.0);
                        let _ = cr.fill();
                    }
                }
                gtk::glib::Propagation::Proceed
            });

            // Right text column
            let vbox = Box::new(Orientation::Vertical, 2);
            vbox.set_valign(gtk::Align::Center);
            vbox.set_margin_end(16);

            let title_lbl = Label::new(None);
            title_lbl.set_markup(&format!(
                "<span font_desc='monospace bold 10.5' color='#ffffff'>{}</span>",
                gtk::glib::markup_escape_text(&title_text)
            ));
            title_lbl.set_halign(gtk::Align::Start);

            let sub_lbl = Label::new(None);
            sub_lbl.set_markup(&format!(
                "<span font_desc='sans 9' color='#a1a1aa'>{}</span>",
                gtk::glib::markup_escape_text(&sub_text)
            ));
            sub_lbl.set_halign(gtk::Align::Start);

            vbox.pack_start(&title_lbl, false, false, 0);
            vbox.pack_start(&sub_lbl, false, false, 0);

            hbox.pack_start(&icon_area, false, false, 0);
            hbox.pack_start(&vbox, true, true, 0);

            let fixed = Fixed::new();
            fixed.set_size_request(340, 64);
            fixed.put(&hbox, 340, 4);
            window.add(&fixed);
            window.show_all();

            let window_clone = window.clone();
            let hbox_clone = hbox.clone();
            let fixed_clone = fixed.clone();
            let start_time = Instant::now();

            gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
                let elapsed = start_time.elapsed().as_secs_f32();
                if elapsed >= 2.80 {
                    window_clone.close();
                    return gtk::glib::ControlFlow::Break;
                }

                let slide_x = if elapsed < 0.35 {
                    let t = (elapsed / 0.35).min(1.0);
                    let ease = 1.0 - (1.0 - t).powi(3);
                    (1.0 - ease) * 340.0
                } else if elapsed < 2.40 {
                    0.0
                } else {
                    let t = ((elapsed - 2.40) / 0.40).min(1.0);
                    let ease = t.powi(3);
                    ease * 340.0
                };

                let target_x = (slide_x + 10.0).round() as i32;
                fixed_clone.move_(&hbox_clone, target_x, 4);
                window_clone.queue_draw();
                gtk::glib::ControlFlow::Continue
            });
        });

        app.run_with_args(&[] as &[&str]);
    }
}

// Vector Icon Drawing Helpers (Centered & Antialiased)
fn draw_replay_icon(cr: &gtk::cairo::Context, width: f64, height: f64, is_active: bool) {
    let cx = width / 2.0;
    let cy = height / 2.0;
    let r = 24.0;

    if is_active {
        cr.set_source_rgb(0.463, 0.725, 0.0); // #76b900 NVIDIA green
    } else {
        cr.set_source_rgb(0.60, 0.68, 0.78); // #94a3b8
    }
    cr.set_line_width(3.8);
    cr.arc(cx, cy, r, 0.25 * PI, 1.80 * PI);
    let _ = cr.stroke();

    // Arrowhead at start of arc
    let a_x = cx + r * (0.25 * PI).cos();
    let a_y = cy + r * (0.25 * PI).sin();
    cr.move_to(a_x, a_y);
    cr.line_to(a_x - 9.0, a_y + 1.0);
    cr.line_to(a_x - 1.0, a_y - 9.0);
    cr.close_path();
    let _ = cr.fill();

    // Centered play triangle
    let tri_r = 8.5;
    let tri_cx = cx + 1.0;
    let tri_cy = cy;
    cr.move_to(tri_cx + tri_r, tri_cy);
    cr.line_to(tri_cx - tri_r * 0.6, tri_cy - tri_r * 0.86);
    cr.line_to(tri_cx - tri_r * 0.6, tri_cy + tri_r * 0.86);
    cr.close_path();
    let _ = cr.fill();
}

fn draw_record_icon(cr: &gtk::cairo::Context, width: f64, height: f64, is_recording: bool) {
    let cx = width / 2.0;
    let cy = height / 2.0;
    if is_recording {
        // Glowing red recording indicator with halo
        cr.set_source_rgba(0.937, 0.267, 0.267, 0.25);
        cr.arc(cx, cy, 32.0, 0.0, PI * 2.0);
        let _ = cr.fill();

        cr.set_source_rgb(0.937, 0.267, 0.267); // #ef4444
        cr.set_line_width(3.5);
        cr.arc(cx, cy, 26.0, 0.0, PI * 2.0);
        let _ = cr.stroke();

        cr.set_source_rgb(0.937, 0.267, 0.267);
        cr.arc(cx, cy, 13.0, 0.0, PI * 2.0);
        let _ = cr.fill();
    } else {
        cr.set_source_rgb(0.60, 0.68, 0.78); // #94a3b8
        cr.set_line_width(3.2);
        cr.arc(cx, cy, 25.0, 0.0, PI * 2.0);
        let _ = cr.stroke();

        cr.set_source_rgb(0.88, 0.91, 0.94);
        cr.arc(cx, cy, 12.0, 0.0, PI * 2.0);
        let _ = cr.fill();
    }
}

fn draw_gear_icon(cr: &gtk::cairo::Context, width: f64, height: f64) {
    let cx = width / 2.0;
    let cy = height / 2.0;
    let r = 24.0;

    cr.set_source_rgb(0.60, 0.68, 0.78); // #94a3b8
    cr.set_line_width(3.0);
    cr.arc(cx, cy, r * 0.75, 0.0, PI * 2.0);
    let _ = cr.stroke();

    cr.arc(cx, cy, r * 0.32, 0.0, PI * 2.0);
    let _ = cr.stroke();

    for i in 0..8 {
        let angle = i as f64 * (PI / 4.0);
        let p_in_x = cx + angle.cos() * (r * 0.68);
        let p_in_y = cy + angle.sin() * (r * 0.68);
        let p_out_x = cx + angle.cos() * (r * 1.08);
        let p_out_y = cy + angle.sin() * (r * 1.08);
        cr.set_line_width(4.2);
        cr.move_to(p_in_x, p_in_y);
        cr.line_to(p_out_x, p_out_y);
        let _ = cr.stroke();
    }
}

pub fn show_menu_overlay() {
    #[cfg(target_os = "windows")]
    {
        crate::overlay_egui::run_egui_overlay();
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
    if std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_err() {
        eprintln!("Warning: No display found for GTK menu overlay. Falling back to cross-platform egui overlay...");
        crate::overlay_egui::run_egui_overlay();
        return;
    }
    if gtk::init().is_err() {
        eprintln!("Note: GTK display init failed. Falling back to cross-platform egui overlay...");
        crate::overlay_egui::run_egui_overlay();
        return;
    }

    let app = Application::builder()
        .application_id("com.scythe.hud")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(|app| {
        let config = ScytheConfig::load();

        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(740)
            .default_height(280)
            .build();

        #[cfg(target_os = "linux")]
        let layer_shell_ok = gtk_layer_shell::is_supported();
        #[cfg(not(target_os = "linux"))]
        let layer_shell_ok = false;

        if layer_shell_ok {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_namespace("scythe-overlay");
            window.set_keyboard_interactivity(true);
            // Shifted higher towards the top (50px margin) per user feedback
            window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, 50);
            window.set_anchor(gtk_layer_shell::Edge::Top, true);
        } else {
            window.set_decorated(false);
            window.set_keep_above(true);
            window.set_skip_taskbar_hint(true);
            window.set_position(gtk::WindowPosition::Center);
            if let Some(display) = gdk::Display::default()
                && let Some(mon) = display.primary_monitor().or_else(|| display.monitor(0)) {
                    let geom = mon.geometry();
                    let x = geom.x() + (geom.width() - 760) / 2;
                    let y = geom.y() + 50;
                    window.move_(x, y);
            }
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
        // PAGE 1: 3 FROSTED GLASS CARDS WITH CONTEXTUAL DROPDOWNS
        // =========================================================================
        let hud_page = Box::new(Orientation::Vertical, 10);
        hud_page.style_context().add_class("hud-wrapper");
        hud_page.set_size_request(740, -1);

        // Top Bar (Frosted glass header strip, no "X" button)
        let header_box = Box::new(Orientation::Horizontal, 10);
        header_box.style_context().add_class("header-bar");

        let brand_badge = Label::new(Some("SCYTHE"));
        brand_badge.style_context().add_class("scythe-badge");

        let title_label = Label::new(Some("SHADOWPLAY OVERLAY"));
        title_label.style_context().add_class("overlay-title");

        let status_banner = Label::new(Some(""));
        status_banner.style_context().add_class("status-banner");

        let spacer = Box::new(Orientation::Horizontal, 0);

        let esc_hint = Label::new(Some("Esc to Close"));
        esc_hint.style_context().add_class("esc-pill");

        header_box.pack_start(&brand_badge, false, false, 0);
        header_box.pack_start(&title_label, false, false, 4);
        header_box.pack_start(&status_banner, false, false, 12);
        header_box.pack_start(&spacer, true, true, 0);
        header_box.pack_start(&esc_hint, false, false, 0);

        // Row of 3 Cards
        let cards_box = Box::new(Orientation::Horizontal, 16);
        cards_box.style_context().add_class("cards-container");
        cards_box.set_halign(gtk::Align::Center);

        // State Trackers for Vector Redraws
        let replay_active_state = Arc::new(AtomicBool::new(config.replay_enabled));
        let record_active_state = Arc::new(AtomicBool::new(false));

        // -------------------------------------------------------------------------
        // CARD 1: INSTANT REPLAY
        // -------------------------------------------------------------------------
        let replay_col = Box::new(Orientation::Vertical, 0);
        replay_col.set_size_request(225, -1);

        let replay_card_btn = Button::new();
        replay_card_btn.style_context().add_class("card-btn");
        replay_card_btn.set_size_request(225, 195);

        let replay_card_inner = Box::new(Orientation::Vertical, 6);
        replay_card_inner.set_valign(gtk::Align::Center);
        replay_card_inner.set_halign(gtk::Align::Center);

        let replay_title_lbl = Label::new(Some("INSTANT REPLAY"));
        replay_title_lbl.style_context().add_class("card-title");

        let replay_icon_area = DrawingArea::new();
        replay_icon_area.set_size_request(76, 76);
        replay_icon_area.set_valign(gtk::Align::Center);
        replay_icon_area.set_halign(gtk::Align::Center);
        let r_state_clone = Arc::clone(&replay_active_state);
        replay_icon_area.connect_draw(move |w, cr| {
            let width = w.allocated_width() as f64;
            let height = w.allocated_height() as f64;
            draw_replay_icon(cr, width, height, r_state_clone.load(Ordering::Relaxed));
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
        replay_card_inner.pack_start(&replay_icon_area, false, false, 6);
        replay_card_inner.pack_start(&replay_status_lbl, false, false, 0);
        replay_card_inner.pack_start(&replay_sub_lbl, false, false, 2);
        replay_card_btn.add(&replay_card_inner);

        // Instant Replay Attached Dropdown Menu
        let replay_revealer = Revealer::new();
        replay_revealer.set_transition_type(RevealerTransitionType::SlideDown);
        replay_revealer.set_transition_duration(150);

        let replay_menu_box = Box::new(Orientation::Vertical, 3);
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
        // CARD 2: RECORD
        // -------------------------------------------------------------------------
        let record_col = Box::new(Orientation::Vertical, 0);
        record_col.set_size_request(225, -1);

        let record_card_btn = Button::new();
        record_card_btn.style_context().add_class("card-btn");
        record_card_btn.set_size_request(225, 195);

        let record_card_inner = Box::new(Orientation::Vertical, 6);
        record_card_inner.set_valign(gtk::Align::Center);
        record_card_inner.set_halign(gtk::Align::Center);

        let record_title_lbl = Label::new(Some("RECORD"));
        record_title_lbl.style_context().add_class("card-title");

        let record_icon_area = DrawingArea::new();
        record_icon_area.set_size_request(76, 76);
        record_icon_area.set_valign(gtk::Align::Center);
        record_icon_area.set_halign(gtk::Align::Center);
        let rec_state_clone = Arc::clone(&record_active_state);
        record_icon_area.connect_draw(move |w, cr| {
            let width = w.allocated_width() as f64;
            let height = w.allocated_height() as f64;
            draw_record_icon(cr, width, height, rec_state_clone.load(Ordering::Relaxed));
            gtk::glib::Propagation::Proceed
        });

        let record_status_lbl = Label::new(Some("Not recording"));
        record_status_lbl.style_context().add_class("card-status");

        let record_sub_lbl = Label::new(Some(&format!("{} FPS • {} Mbps", config.fps, config.record_bitrate_kbps / 1000)));
        record_sub_lbl.style_context().add_class("card-sub");

        record_card_inner.pack_start(&record_title_lbl, false, false, 0);
        record_card_inner.pack_start(&record_icon_area, false, false, 6);
        record_card_inner.pack_start(&record_status_lbl, false, false, 0);
        record_card_inner.pack_start(&record_sub_lbl, false, false, 2);
        record_card_btn.add(&record_card_inner);

        // Record Attached Dropdown Menu
        let record_revealer = Revealer::new();
        record_revealer.set_transition_type(RevealerTransitionType::SlideDown);
        record_revealer.set_transition_duration(150);

        let record_menu_box = Box::new(Orientation::Vertical, 3);
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
        // CARD 3: SETTINGS
        // -------------------------------------------------------------------------
        let settings_col = Box::new(Orientation::Vertical, 0);
        settings_col.set_size_request(225, -1);

        let settings_card_btn = Button::new();
        settings_card_btn.style_context().add_class("card-btn");
        settings_card_btn.set_size_request(225, 195);

        let settings_card_inner = Box::new(Orientation::Vertical, 6);
        settings_card_inner.set_valign(gtk::Align::Center);
        settings_card_inner.set_halign(gtk::Align::Center);

        let settings_title_lbl = Label::new(Some("SETTINGS"));
        settings_title_lbl.style_context().add_class("card-title");

        let settings_icon_area = DrawingArea::new();
        settings_icon_area.set_size_request(76, 76);
        settings_icon_area.set_valign(gtk::Align::Center);
        settings_icon_area.set_halign(gtk::Align::Center);
        settings_icon_area.connect_draw(|w, cr| {
            let width = w.allocated_width() as f64;
            let height = w.allocated_height() as f64;
            draw_gear_icon(cr, width, height);
            gtk::glib::Propagation::Proceed
        });

        let settings_status_lbl = Label::new(Some("Preferences"));
        settings_status_lbl.style_context().add_class("card-status");

        let settings_sub_lbl = Label::new(Some("Audio, Quality & Hotkeys"));
        settings_sub_lbl.style_context().add_class("card-sub");

        settings_card_inner.pack_start(&settings_title_lbl, false, false, 0);
        settings_card_inner.pack_start(&settings_icon_area, false, false, 6);
        settings_card_inner.pack_start(&settings_status_lbl, false, false, 0);
        settings_card_inner.pack_start(&settings_sub_lbl, false, false, 2);
        settings_card_btn.add(&settings_card_inner);

        settings_col.pack_start(&settings_card_btn, false, false, 0);

        // Pack 3 columns
        cards_box.pack_start(&replay_col, false, false, 0);
        cards_box.pack_start(&record_col, false, false, 0);
        cards_box.pack_start(&settings_col, false, false, 0);

        hud_page.pack_start(&header_box, false, false, 0);
        hud_page.pack_start(&cards_box, true, true, 0);

        // =========================================================================
        // PAGE 2: REVAMPED CLEAN FROSTED GLASS SETTINGS PANEL
        // =========================================================================
        let settings_page = Box::new(Orientation::Vertical, 12);
        settings_page.style_context().add_class("settings-panel");
        settings_page.set_size_request(760, -1);

        // Header with Back Button (No "X" button)
        let settings_header = Box::new(Orientation::Horizontal, 12);
        let back_btn = Button::with_label("< Back to Overlay");
        back_btn.style_context().add_class("back-btn");

        let settings_page_title = Label::new(Some("RECORDER SETTINGS"));
        settings_page_title.style_context().add_class("settings-title");

        let settings_spacer = Box::new(Orientation::Horizontal, 0);

        let settings_esc_hint = Label::new(Some("Esc to Close"));
        settings_esc_hint.style_context().add_class("esc-pill");

        settings_header.pack_start(&back_btn, false, false, 0);
        settings_header.pack_start(&settings_page_title, false, false, 10);
        settings_header.pack_start(&settings_spacer, true, true, 0);
        settings_header.pack_start(&settings_esc_hint, false, false, 0);

        // Scrollable Body
        let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(440);
        scroll.set_max_content_height(480);

        let settings_body = Box::new(Orientation::Vertical, 10);
        settings_body.set_margin_start(4);
        settings_body.set_margin_end(4);

        // -------------------------------------------------------------------------
        // SECTION 1: DISPLAY & CAPTURE (With Mouse Cursor Switch!)
        // -------------------------------------------------------------------------
        let sec1_card = Box::new(Orientation::Vertical, 8);
        sec1_card.style_context().add_class("settings-section-card");

        let sec1_hdr = Label::new(Some("DISPLAY & CAPTURE"));
        sec1_hdr.style_context().add_class("section-header");
        sec1_hdr.set_halign(gtk::Align::Start);
        sec1_card.pack_start(&sec1_hdr, false, false, 0);

        // 1.1 Mouse Cursor Toggle Row
        let cursor_row = Box::new(Orientation::Horizontal, 12);
        let cursor_text_box = Box::new(Orientation::Vertical, 2);
        let cursor_title = Label::new(Some("Record Mouse Cursor"));
        cursor_title.style_context().add_class("setting-row-title");
        cursor_title.set_halign(gtk::Align::Start);
        let cursor_desc = Label::new(Some("Capture the mouse pointer in gameplay recordings and instant replays"));
        cursor_desc.style_context().add_class("sub-info-label");
        cursor_desc.set_halign(gtk::Align::Start);
        cursor_text_box.pack_start(&cursor_title, false, false, 0);
        cursor_text_box.pack_start(&cursor_desc, false, false, 0);

        let cursor_switch = Switch::new();
        cursor_switch.set_active(config.show_cursor);
        cursor_switch.set_valign(gtk::Align::Center);

        let cursor_sp = Box::new(Orientation::Horizontal, 0);
        cursor_row.pack_start(&cursor_text_box, false, false, 0);
        cursor_row.pack_start(&cursor_sp, true, true, 0);
        cursor_row.pack_start(&cursor_switch, false, false, 0);
        sec1_card.pack_start(&cursor_row, false, false, 4);

        let sec1_grid = gtk::Grid::new();
        sec1_grid.set_column_spacing(18);
        sec1_grid.set_row_spacing(8);

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
        let codec_lbl = Label::new(Some("Encoder Codec:"));
        codec_lbl.set_halign(gtk::Align::Start);
        let codec_combo = ComboBoxText::new();
        codec_combo.append(Some("h264"), "H.264 / AVC (Fast & Universal)");
        codec_combo.append(Some("hevc"), "HEVC / H.265 (High Efficiency)");
        codec_combo.append(Some("av1"), "AV1 (Next-Generation Quality)");
        codec_combo.set_active_id(Some(&config.video_codec));

        sec1_grid.attach(&fps_lbl, 0, 0, 1, 1);
        sec1_grid.attach(&fps_box, 1, 0, 1, 1);
        sec1_grid.attach(&bit_lbl, 0, 1, 1, 1);
        sec1_grid.attach(&bit_box, 1, 1, 1, 1);
        sec1_grid.attach(&codec_lbl, 0, 2, 1, 1);
        sec1_grid.attach(&codec_combo, 1, 2, 1, 1);
        sec1_card.pack_start(&sec1_grid, false, false, 2);
        settings_body.pack_start(&sec1_card, false, false, 0);

        // -------------------------------------------------------------------------
        // SECTION 2: INSTANT REPLAY BUFFER
        // -------------------------------------------------------------------------
        let sec2_card = Box::new(Orientation::Vertical, 8);
        sec2_card.style_context().add_class("settings-section-card");

        let sec2_hdr = Label::new(Some("INSTANT REPLAY BUFFER"));
        sec2_hdr.style_context().add_class("section-header");
        sec2_hdr.set_halign(gtk::Align::Start);
        sec2_card.pack_start(&sec2_hdr, false, false, 0);

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
        sec2_card.pack_start(&sec2_grid, false, false, 2);
        settings_body.pack_start(&sec2_card, false, false, 0);

        // -------------------------------------------------------------------------
        // SECTION 3: AUDIO ROUTING & LEVELS
        // -------------------------------------------------------------------------
        let sec3_card = Box::new(Orientation::Vertical, 8);
        sec3_card.style_context().add_class("settings-section-card");

        let sec3_hdr = Label::new(Some("AUDIO ROUTING & SOUND"));
        sec3_hdr.style_context().add_class("section-header");
        sec3_hdr.set_halign(gtk::Align::Start);
        sec3_card.pack_start(&sec3_hdr, false, false, 0);

        let sec3_grid = gtk::Grid::new();
        sec3_grid.set_column_spacing(18);
        sec3_grid.set_row_spacing(8);

        let audio_mode_lbl = Label::new(Some("Audio Mode:"));
        audio_mode_lbl.set_halign(gtk::Align::Start);
        let audio_mode_combo = ComboBoxText::new();
        audio_mode_combo.append(Some("system"), "System Sounds Only (Game / Desktop)");
        audio_mode_combo.append(Some("mic"), "Microphone Only");
        audio_mode_combo.append(Some("both"), "Both Combined (System Sounds + Microphone)");
        audio_mode_combo.append(Some("muted"), "Muted (No Audio)");
        audio_mode_combo.set_active_id(Some(&config.audio_mode));

        let audio_dev_lbl = Label::new(Some("Recording Device:"));
        audio_dev_lbl.set_halign(gtk::Align::Start);
        let audio_dev_combo = ComboBoxText::new();
        audio_dev_combo.append(Some("default"), "Default Recording Device");
        for dev in crate::capture::audio::list_input_devices() {
            audio_dev_combo.append(Some(&dev), &dev);
        }
        for app in crate::capture::audio::list_application_audio() {
            audio_dev_combo.append(Some(&format!("app:{}", app)), &format!("App: {}", app));
        }
        audio_dev_combo.set_active_id(Some(&config.audio_device));

        let sys_vol_lbl = Label::new(Some("System Volume:"));
        sys_vol_lbl.set_halign(gtk::Align::Start);
        let sys_vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 150.0, 5.0);
        sys_vol_scale.set_value((config.system_volume * 100.0).round() as f64);
        sys_vol_scale.set_size_request(240, -1);

        let sys_level_bar = LevelBar::new();
        sys_level_bar.set_min_value(0.0);
        sys_level_bar.set_max_value(1.0);
        sys_level_bar.set_size_request(240, 6);

        let mic_vol_lbl = Label::new(Some("Mic Volume:"));
        mic_vol_lbl.set_halign(gtk::Align::Start);
        let mic_vol_scale = Scale::with_range(Orientation::Horizontal, 0.0, 150.0, 5.0);
        mic_vol_scale.set_value((config.mic_volume * 100.0).round() as f64);
        mic_vol_scale.set_size_request(240, -1);

        let mic_level_bar = LevelBar::new();
        mic_level_bar.set_min_value(0.0);
        mic_level_bar.set_max_value(1.0);
        mic_level_bar.set_size_request(240, 6);

        sec3_grid.attach(&audio_mode_lbl, 0, 0, 1, 1);
        sec3_grid.attach(&audio_mode_combo, 1, 0, 1, 1);
        sec3_grid.attach(&audio_dev_lbl, 0, 1, 1, 1);
        sec3_grid.attach(&audio_dev_combo, 1, 1, 1, 1);
        sec3_grid.attach(&sys_vol_lbl, 0, 2, 1, 1);
        sec3_grid.attach(&sys_vol_scale, 1, 2, 1, 1);
        sec3_grid.attach(&sys_level_bar, 1, 3, 1, 1);
        sec3_grid.attach(&mic_vol_lbl, 0, 4, 1, 1);
        sec3_grid.attach(&mic_vol_scale, 1, 4, 1, 1);
        sec3_grid.attach(&mic_level_bar, 1, 5, 1, 1);
        sec3_card.pack_start(&sec3_grid, false, false, 2);
        settings_body.pack_start(&sec3_card, false, false, 0);

        // -------------------------------------------------------------------------
        // SECTION 4: STORAGE & SHORTCUTS
        // -------------------------------------------------------------------------
        let sec4_card = Box::new(Orientation::Vertical, 8);
        sec4_card.style_context().add_class("settings-section-card");

        let sec4_hdr = Label::new(Some("STORAGE & SHORTCUTS"));
        sec4_hdr.style_context().add_class("section-header");
        sec4_hdr.set_halign(gtk::Align::Start);
        sec4_card.pack_start(&sec4_hdr, false, false, 0);

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
        sec4_card.pack_start(&sec4_grid, false, false, 2);
        settings_body.pack_start(&sec4_card, false, false, 0);

        scroll.add(&settings_body);

        // Apply & Save Settings Button
        let apply_btn = Button::with_label("Apply & Save Settings");
        apply_btn.style_context().add_class("apply-save-btn");
        apply_btn.set_size_request(-1, 44);

        settings_page.pack_start(&settings_header, false, false, 0);
        settings_page.pack_start(&scroll, true, true, 4);
        settings_page.pack_start(&apply_btn, false, false, 4);

        // Add Pages to Stack
        stack.add_named(&hud_page, "hud");
        stack.add_named(&settings_page, "settings");
        window.add(&stack);

        // =========================================================================
        // CSS STYLING (Translucent Frosted Glass Theme)
        // =========================================================================
        let css_provider = CssProvider::new();
        let css = r#"
            window, window.background, .background {
                background-color: transparent !important;
                background: transparent !important;
                border: none !important;
                box-shadow: none !important;
            }
            .hud-wrapper {
                background-color: transparent;
                padding: 0px;
            }
            /* Floating Frosted Glass Top Bar */
            .header-bar {
                background-color: rgba(18, 26, 38, 0.72);
                border: 1px solid rgba(255, 255, 255, 0.15);
                border-radius: 12px;
                padding: 8px 18px;
                box-shadow: 0px 12px 32px rgba(0, 0, 0, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.16);
            }
            .scythe-badge,
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
                font-size: 12.5px;
                font-weight: 800;
                letter-spacing: 1.2px;
            }
            .status-banner {
                color: #76b900;
                font-size: 11.5px;
                font-weight: 700;
            }
            .esc-pill {
                background-color: rgba(255, 255, 255, 0.08);
                border: 1px solid rgba(255, 255, 255, 0.14);
                color: #94a3b8;
                font-size: 10.5px;
                font-weight: 700;
                border-radius: 6px;
                padding: 3px 8px;
            }
            /* Frosted Glass Action Cards */
            .card-btn {
                background-color: rgba(20, 28, 42, 0.68);
                border: 1px solid rgba(255, 255, 255, 0.15);
                border-radius: 16px;
                padding: 18px;
                box-shadow: 0px 16px 40px rgba(0, 0, 0, 0.55), inset 0 1px 0 rgba(255, 255, 255, 0.18);
            }
            .card-btn:hover {
                background-color: rgba(26, 38, 56, 0.82);
                border-color: #76b900;
                box-shadow: 0 20px 48px rgba(0, 0, 0, 0.65), 0 0 20px rgba(118, 185, 0, 0.28), inset 0 1px 0 rgba(255, 255, 255, 0.25);
            }
            .card-btn-active {
                background-color: rgba(26, 38, 56, 0.90);
                border-color: #76b900;
                border-bottom-left-radius: 0px;
                border-bottom-right-radius: 0px;
                box-shadow: 0 0 20px rgba(118, 185, 0, 0.35);
            }
            .card-title {
                color: #ffffff;
                font-size: 11.5px;
                font-weight: 800;
                letter-spacing: 0.8px;
            }
            .card-status {
                color: #94a3b8;
                font-size: 11.5px;
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
                font-size: 10.5px;
                font-weight: 500;
            }
            /* Attached Frosted Dropdown Menus */
            .dropdown-menu {
                background-color: rgba(16, 22, 34, 0.88);
                border: 1px solid rgba(255, 255, 255, 0.15);
                border-top: none;
                border-bottom-left-radius: 14px;
                border-bottom-right-radius: 14px;
                padding: 8px;
                box-shadow: 0px 20px 48px rgba(0, 0, 0, 0.75);
            }
            .dropdown-item {
                background-color: transparent;
                color: #e2e8f0;
                font-size: 12.5px;
                font-weight: 700;
                border-radius: 8px;
                border: 1px solid transparent;
                padding: 9px 14px;
            }
            .dropdown-item:hover {
                background-color: rgba(118, 185, 0, 0.15);
                border-color: rgba(118, 185, 0, 0.5);
                color: #76b900;
            }
            /* Frosted Settings Panel */
            .settings-panel {
                background-color: #0f1620;
                border: 1px solid rgba(255, 255, 255, 0.16);
                border-radius: 18px;
                padding: 20px 26px;
                box-shadow: 0px 24px 64px rgba(0, 0, 0, 0.8), inset 0 1px 0 rgba(255, 255, 255, 0.2);
            }
            .settings-section-card {
                background-color: #18202e;
                border: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 12px;
                padding: 14px 18px;
            }
            .settings-title {
                color: #ffffff;
                font-size: 13.5px;
                font-weight: 800;
                letter-spacing: 1px;
            }
            .setting-row-title {
                color: #ffffff;
                font-size: 13px;
                font-weight: 700;
            }
            .back-btn {
                background-color: rgba(118, 185, 0, 0.12);
                color: #76b900;
                font-size: 12px;
                font-weight: 800;
                border: 1px solid rgba(118, 185, 0, 0.45);
                border-radius: 8px;
                padding: 6px 14px;
            }
            .back-btn:hover {
                background-color: rgba(118, 185, 0, 0.22);
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
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 6px;
                color: #cbd5e1;
                font-size: 11px;
                font-weight: 600;
                padding: 4px 10px;
            }
            .preset-btn:hover {
                background-color: rgba(118, 185, 0, 0.22);
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
                padding: 10px 16px;
                box-shadow: 0 4px 16px rgba(118, 185, 0, 0.35);
            }
            .apply-save-btn:hover {
                background-color: #8ce000;
            }
            switch {
                border-radius: 14px;
                background-color: #1e2838;
                border: 1px solid rgba(255, 255, 255, 0.2);
            }
            switch:checked {
                background-color: #76b900;
                border-color: #8ce000;
            }
            switch slider {
                background-color: #ffffff;
                border-radius: 50%;
                min-width: 18px;
                min-height: 18px;
            }
            entry, spinbutton, combobox button {
                background-color: rgba(18, 24, 36, 0.7);
                color: #f1f5f9;
                border: 1px solid rgba(255, 255, 255, 0.14);
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
                color: #94a3b8;
                font-size: 10.5px;
                font-weight: 500;
            }
        "#;
        let _ = css_provider.load_from_data(css.as_bytes());
        if let Some(screen) = gdk::Screen::default() {
            StyleContext::add_provider_for_screen(
                &screen,
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
        window.style_context().add_provider(&css_provider, gtk::STYLE_PROVIDER_PRIORITY_USER);

        // =========================================================================
        // EVENT HANDLERS & NAVIGATION (Esc key closes cleanly)
        // =========================================================================
        let w_esc = window.clone();
        window.connect_key_press_event(move |_, key| {
            if key.keyval() == gdk::keys::constants::Escape {
                w_esc.close();
                std::process::exit(0);
            } else {
                gtk::glib::Propagation::Proceed
            }
        });

        // -------------------------------------------------------------------------
        // CARD INTERACTION: TOGGLE ATTACHED DROPDOWNS
        // -------------------------------------------------------------------------
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

        let stack_to_settings = stack.clone();
        let w_to_settings = window.clone();
        settings_card_btn.connect_clicked(move |_| {
            stack_to_settings.set_visible_child_name("settings");
            w_to_settings.resize(760, 560);
        });

        // Back to HUD from Settings
        let stack_to_hud = stack.clone();
        let w_to_hud = window.clone();
        back_btn.connect_clicked(move |_| {
            stack_to_hud.set_visible_child_name("hud");
            w_to_hud.resize(740, 280);
        });

        // Replay Menu Items
        let r_toggle_lbl = replay_toggle_item.clone();
        let r_status_lbl_sync = replay_status_lbl.clone();
        let r_state_sync = Arc::clone(&replay_active_state);
        let r_icon_sync = replay_icon_area.clone();
        let rev_r_hide = replay_revealer.clone();
        let card_r_deactivate = replay_card_btn.clone();
        replay_toggle_item.connect_clicked(move |_| {
            let mut cfg = ScytheConfig::load();
            cfg.replay_enabled = !cfg.replay_enabled;
            let _ = cfg.save();
            ScytheConfig::notify_daemon_reload();

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

        let stack_r_settings = stack.clone();
        let w_r_settings = window.clone();
        let rev_r_hide3 = replay_revealer.clone();
        let card_r_deactivate3 = replay_card_btn.clone();
        replay_settings_item.connect_clicked(move |_| {
            rev_r_hide3.set_reveal_child(false);
            card_r_deactivate3.style_context().remove_class("card-btn-active");
            stack_r_settings.set_visible_child_name("settings");
            w_r_settings.resize(760, 560);
        });

        // Record Menu Items
        let rev_rec_hide = record_revealer.clone();
        let card_rec_deactivate = record_card_btn.clone();
        record_toggle_item.connect_clicked(move |_| {
            let _ = ipc::send_command(Command::ToggleRecording);
            rev_rec_hide.set_reveal_child(false);
            card_rec_deactivate.style_context().remove_class("card-btn-active");
        });

        let stack_rec_settings = stack.clone();
        let w_rec_settings = window.clone();
        let rev_rec_hide2 = record_revealer.clone();
        let card_rec_deactivate2 = record_card_btn.clone();
        record_settings_item.connect_clicked(move |_| {
            rev_rec_hide2.set_reveal_child(false);
            card_rec_deactivate2.style_context().remove_class("card-btn-active");
            stack_rec_settings.set_visible_child_name("settings");
            w_rec_settings.resize(760, 560);
        });

        // Direct Mouse Cursor Switch Toggle
        let banner_cur = status_banner.clone();
        cursor_switch.connect_state_set(move |_, active| {
            let mut cfg = ScytheConfig::load();
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
            gtk::glib::Propagation::Proceed
        });

        // Apply & Save Settings
        let stack_after_save = stack.clone();
        let w_after_save = window.clone();
        let banner_after_save = status_banner.clone();
        let r_sub_lbl_sync = replay_sub_lbl.clone();
        let rec_sub_lbl_sync = record_sub_lbl.clone();
        let cur_sw_sync = cursor_switch.clone();

        apply_btn.connect_clicked(move |_| {
            let mut cfg = ScytheConfig::load();
            cfg.show_cursor = cur_sw_sync.is_active();
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
            ScytheConfig::notify_daemon_reload();

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
        let sys_level_sync = sys_level_bar.clone();
        let mic_level_sync = mic_level_bar.clone();

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = Arc::clone(&is_running);

        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
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

                // 3. Audio VU meter levels sync
                sys_level_sync.set_value(st.system_level_peak as f64);
                mic_level_sync.set_value(st.mic_level_peak as f64);
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
}
