use vrec::ipc::{Command, send_command, query_status};
use vrec::overlay::{show_menu_overlay, show_notification};
use std::env;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};

fn ensure_wayland_env() {
    #[cfg(target_os = "linux")]
    {
        let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        unsafe {
            if env::var("WAYLAND_DISPLAY").is_err() {
                if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("wayland-") && !name.ends_with(".lock") {
                            env::set_var("WAYLAND_DISPLAY", &name);
                            break;
                        }
                    }
                }
            }
            if env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
                let bus_path = format!("{}/bus", runtime_dir);
                if std::path::Path::new(&bus_path).exists() {
                    env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", bus_path));
                }
            }
            if env::var("XDG_CURRENT_DESKTOP").is_err() {
                env::set_var("XDG_CURRENT_DESKTOP", "Hyprland");
            }
            if env::var("XDG_SESSION_TYPE").map(|s| s == "tty" || s.is_empty()).unwrap_or(true) && env::var("WAYLAND_DISPLAY").is_ok() {
                env::set_var("XDG_SESSION_TYPE", "wayland");
            }
        }
    }
}

fn ensure_daemon_running() {
    if query_status().is_err() {
        println!("vrec-daemon not running. Auto-launching vrec-daemon...");
        let _ = std::process::Command::new("vrec-daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if query_status().is_ok() {
                println!("vrec-daemon successfully connected.");
                break;
            }
        }
    }
}

fn send_with_notification(cmd: Command, success_msg: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_daemon_running();
    match send_command(cmd) {
        Ok(()) => {
            show_notification(success_msg);
            Ok(())
        }
        Err(e) => {
            show_notification("Error: failed to connect to vrec-daemon");
            Err(e)
        }
    }
}

fn handle_toggle_recording() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_daemon_running();
    let status = query_status().ok();
    let is_rec = status.as_ref().map(|s| s.is_recording).unwrap_or(false);
    match send_command(Command::ToggleRecording) {
        Ok(()) => {
            if is_rec {
                show_notification("Recording saved");
            } else {
                show_notification("Recording started");
            }
            Ok(())
        }
        Err(e) => {
            show_notification("Error: failed to connect to vrec-daemon");
            Err(e)
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_wayland_env();

    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--save" => {
                return send_with_notification(Command::SaveReplay, "Replay saved");
            }
            "--menu" => {
                ensure_daemon_running();
                show_menu_overlay();
                return Ok(());
            }
            "--record" | "--toggle" => {
                return handle_toggle_recording();
            }
            "--start" => {
                return send_with_notification(Command::StartRecording, "Recording started");
            }
            "--stop" => {
                return send_with_notification(Command::StopRecording, "Recording saved");
            }
            "--reload" => {
                return send_command(Command::ReloadConfig);
            }
            "--quit" => {
                return send_command(Command::StopDaemon);
            }
            "--help" | "-h" => {
                println!("vrec-ui - UI Overlay and Hotkey listener for vrec");
                println!("Usage:");
                println!("  vrec-ui                Run global hotkey manager in background");
                println!("  vrec-ui --menu         Open overlay menu");
                println!("  vrec-ui --save         Save instant replay and show notification");
                println!("  vrec-ui --record       Toggle normal recording on/off (with notification)");
                println!("  vrec-ui --start        Start normal recording");
                println!("  vrec-ui --stop         Stop normal recording");
                println!("  vrec-ui --reload       Reload daemon configuration");
                println!("  vrec-ui --quit         Stop background daemon");
                return Ok(());
            }
            _ => {}
        }
    }

    println!("Starting vrec UI/Hotkey process...");
    
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "".to_string()).to_lowercase();
    
    let is_gnome = desktop.contains("gnome") || desktop.contains("ubuntu");
    let is_kde = desktop.contains("kde");
    
    if session_type.to_lowercase() == "wayland" {
        if is_gnome || is_kde {
            println!("Note: Wayland native layer shell used. Ensure your compositor allows it.");
            println!("Tip: Global hotkeys may be restricted by Wayland compositors.");
            println!("     You can bind `vrec-ui --menu` or `vrec-ui --save` in your system shortcut settings.");
        } else {
            println!("wlroots/Hyprland compositor detected.");
        }
    } else {
        println!("X11 session detected.");
    }
    
    let manager = GlobalHotKeyManager::new()?;
    let hotkey_menu = HotKey::new(Some(Modifiers::ALT), Code::KeyZ);
    let hotkey_save = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    let hotkey_record = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F9);

    let _ = manager.register(hotkey_menu);
    let _ = manager.register(hotkey_save);
    let _ = manager.register(hotkey_record);

    println!("Listening for global hotkeys (Alt+Z for overlay, Ctrl+Shift+R for replay, Ctrl+Shift+F9 for recording)...");

    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.try_recv() {
            if event.id == hotkey_menu.id() {
                ensure_daemon_running();
                show_menu_overlay();
            } else if event.id == hotkey_save.id() {
                let _ = send_with_notification(Command::SaveReplay, "Replay saved");
            } else if event.id == hotkey_record.id() {
                let _ = handle_toggle_recording();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
