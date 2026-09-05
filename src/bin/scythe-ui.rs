#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use scythe::ipc::{Command, send_command, query_status};
use scythe::overlay::show_notification;
use std::env;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};

fn ensure_wayland_env() {
    #[cfg(target_os = "linux")]
    {
        let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        unsafe {
            if env::var("WAYLAND_DISPLAY").is_err()
                && let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("wayland-") && !name.ends_with(".lock") {
                            env::set_var("WAYLAND_DISPLAY", &name);
                            break;
                        }
                    }
            }
            if env::var("DISPLAY").is_err() && std::path::Path::new("/tmp/.X11-unix/X0").exists() {
                env::set_var("DISPLAY", ":0");
            }
            if env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
                let bus_path = format!("{}/bus", runtime_dir);
                if std::path::Path::new(&bus_path).exists() {
                    env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", bus_path));
                }
            }
            if env::var("XDG_CURRENT_DESKTOP").is_err() && env::var("WAYLAND_DISPLAY").is_ok() {
                env::set_var("XDG_CURRENT_DESKTOP", "Hyprland");
            }
            if env::var("XDG_SESSION_TYPE").map(|s| s == "tty" || s.is_empty()).unwrap_or(true) {
                if env::var("WAYLAND_DISPLAY").is_ok() {
                    env::set_var("XDG_SESSION_TYPE", "wayland");
                } else if env::var("DISPLAY").is_ok() {
                    env::set_var("XDG_SESSION_TYPE", "x11");
                }
            }
        }
    }
}

fn get_daemon_cmd() -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = if let Ok(mut path) = std::env::current_exe() {
        path.pop();
        #[cfg(target_os = "windows")]
        let primary = path.join("scythe-daemon.exe");
        #[cfg(target_os = "windows")]
        let fallback = path.join("vrec-daemon.exe");
        #[cfg(not(target_os = "windows"))]
        let primary = path.join("scythe-daemon");
        #[cfg(not(target_os = "windows"))]
        let fallback = path.join("vrec-daemon");

        if primary.exists() {
            std::process::Command::new(primary)
        } else if fallback.exists() {
            std::process::Command::new(fallback)
        } else {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("scythe-daemon.exe")
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::process::Command::new("scythe-daemon")
            }
        }
    } else {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("scythe-daemon.exe")
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("scythe-daemon")
        }
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd
}

fn get_ui_cmd() -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = if let Ok(path) = std::env::current_exe() {
        std::process::Command::new(path)
    } else {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("scythe-ui.exe")
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("scythe-ui")
        }
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd
}

fn ensure_daemon_running() {
    if query_status().is_err() {
        let _ = get_daemon_cmd()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        for _ in 0..15 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if query_status().is_ok() {
                break;
            }
        }
    }
}

fn ensure_hotkeys_running() {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            use windows::Win32::Foundation::{CloseHandle, GetLastError, WIN32_ERROR};
            use windows::Win32::System::Threading::CreateMutexW;

            if let Ok(handle) = CreateMutexW(None, false, windows::core::w!("Global\\scythe_hotkeys_single_instance")) {
                if GetLastError() == WIN32_ERROR(183) { // ERROR_ALREADY_EXISTS
                    let _ = CloseHandle(handle);
                    return;
                }
                let _ = CloseHandle(handle);
            }
        }

        let _ = get_ui_cmd()
            .arg("--hotkeys")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
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
            show_notification("Error: failed to connect to scythe-daemon");
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
            show_notification("Error: failed to connect to scythe-daemon");
            Err(e)
        }
    }
}

fn handle_toggle_cursor() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_daemon_running();
    let current_cursor = query_status().ok().map(|s| s.show_cursor).unwrap_or(true);
    let next_cursor = !current_cursor;
    match send_command(Command::ToggleCursor) {
        Ok(()) => {
            if next_cursor {
                show_notification("Cursor: Visible in recording");
            } else {
                show_notification("Cursor: Hidden from recording");
            }
            Ok(())
        }
        Err(e) => {
            show_notification("Error: failed to connect to scythe-daemon");
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
                ensure_hotkeys_running();
                #[cfg(not(target_os = "linux"))]
                scythe::overlay_egui::run_egui_overlay();
                #[cfg(target_os = "linux")]
                scythe::overlay::show_menu_overlay();
                return Ok(());
            }
            "--egui" => {
                ensure_daemon_running();
                ensure_hotkeys_running();
                scythe::overlay_egui::run_egui_overlay();
                return Ok(());
            }
            "--hotkeys" | "--background" => {
                // fall through to hotkey loop
            }
            "--record" | "--toggle" => {
                return handle_toggle_recording();
            }
            "--cursor" | "--toggle-cursor" => {
                return handle_toggle_cursor();
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
            "--status" => {
                ensure_daemon_running();
                match query_status() {
                    Ok(st) => {
                        println!("Daemon Status:");
                        println!("  State: {}", if st.is_recording { format!("RECORDING ({}s)", st.recording_duration_sec) } else { "IDLE".to_string() });
                        println!("  Instant Replay: {}", if st.is_replay_active { "ACTIVE" } else { "OFF" });
                        println!("  Audio Mode: {}", st.audio_mode);
                        println!("  Mic Volume: {:.0}%", st.mic_volume * 100.0);
                        println!("  System Volume: {:.0}%", st.system_volume * 100.0);
                        println!("  Show Cursor: {}", st.show_cursor);
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to scythe daemon: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            "--quit" => {
                return send_command(Command::StopDaemon);
            }
            "--help" | "-h" => {
                println!("scythe-ui - UI Overlay and Hotkey listener for scythe");
                println!("Usage:");
                println!("  scythe-ui                Open the interactive overlay UI menu (default)");
                println!("  scythe-ui --menu         Open the interactive overlay UI menu");
                println!("  scythe-ui --hotkeys      Run global hotkey manager in background");
                println!("  scythe-ui --status       Query current daemon status");
                println!("  scythe-ui --save         Save instant replay and show notification");
                println!("  scythe-ui --record       Toggle normal recording on/off (with notification)");
                println!("  scythe-ui --cursor       Toggle mouse cursor recording on/off (with notification)");
                println!("  scythe-ui --start        Start normal recording");
                println!("  scythe-ui --stop         Stop normal recording");
                println!("  scythe-ui --reload       Reload daemon configuration");
                println!("  scythe-ui --quit         Stop background daemon");
                return Ok(());
            }
            _ => {}
        }
    } else {
        ensure_daemon_running();
        ensure_hotkeys_running();
        #[cfg(not(target_os = "linux"))]
        scythe::overlay_egui::run_egui_overlay();
        #[cfg(target_os = "linux")]
        scythe::overlay::show_menu_overlay();
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    let _mutex = unsafe {
        use windows::Win32::Foundation::{GetLastError, WIN32_ERROR};
        use windows::Win32::System::Threading::CreateMutexW;

        let handle = CreateMutexW(None, true, windows::core::w!("Global\\scythe_hotkeys_single_instance"));
        if GetLastError() == WIN32_ERROR(183) {
            return Ok(());
        }
        handle.ok()
    };

    println!("Starting scythe UI/Hotkey process...");
    
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "".to_string()).to_lowercase();
    
    let is_gnome = desktop.contains("gnome") || desktop.contains("ubuntu");
    let is_kde = desktop.contains("kde");
    
    if session_type.to_lowercase() == "wayland" {
        if is_gnome || is_kde {
            println!("Note: Wayland native layer shell used. Ensure your compositor allows it.");
            println!("Tip: Global hotkeys may be restricted by Wayland compositors.");
            println!("     You can bind `scythe-ui --menu` or `scythe-ui --save` in your system shortcut settings.");
        } else {
            println!("wlroots/Hyprland compositor detected.");
            scythe::hyprland_binds::register_hyprland_binds(&scythe::config::ScytheConfig::load());
            scythe::hyprland_binds::spawn_hyprland_reload_watcher();
        }
    } else {
        println!("X11 session detected.");
    }
    
    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to create global hotkey manager: {}", e);
            return Ok(());
        }
    };
    let hotkey_menu = HotKey::new(Some(Modifiers::ALT), Code::KeyZ);
    let hotkey_save = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    let hotkey_record = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F9);
    let hotkey_cursor = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F10);

    let _ = manager.register(hotkey_menu);
    let _ = manager.register(hotkey_save);
    let _ = manager.register(hotkey_record);
    let _ = manager.register(hotkey_cursor);

    println!("Listening for global hotkeys (Alt+Z for overlay, Ctrl+Shift+R for replay, Ctrl+Shift+F9 for recording, Ctrl+Shift+F10 for cursor)...");

    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.try_recv() {
            if event.id == hotkey_menu.id() {
                ensure_daemon_running();
                let _ = get_ui_cmd().arg("--menu").spawn();
            } else if event.id == hotkey_save.id() {
                let _ = send_with_notification(Command::SaveReplay, "Replay saved");
            } else if event.id == hotkey_record.id() {
                let _ = handle_toggle_recording();
            } else if event.id == hotkey_cursor.id() {
                let _ = handle_toggle_cursor();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
