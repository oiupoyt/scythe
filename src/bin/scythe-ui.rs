#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use scythe::ipc::{Command, send_command, query_status};
use scythe::overlay::{show_shadowplay_toast, ToastIcon};
use std::env;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};

fn ensure_wayland_env() {
    scythe::overlay::ensure_wayland_env();
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

fn check_and_toggle_overlay() -> bool {
    let pid_path = scythe::ipc::get_overlay_pid_path();
    if pid_path.exists() {
        if let Ok(metadata) = std::fs::metadata(&pid_path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    // Debounce: if the overlay was spawned less than 400ms ago, this is a
                    // duplicate event from concurrent triggers. Do not kill it; simply exit.
                    if elapsed < std::time::Duration::from_millis(400) {
                        return true;
                    }
                }
            }
        }

        if let Ok(content) = std::fs::read_to_string(&pid_path) {
            #[cfg(unix)]
            if let Ok(pid) = content.trim().parse::<i32>() {
                let exists = unsafe { libc::kill(pid, 0) == 0 };
                if exists && pid != std::process::id() as i32 {
                    let proc_cmd = format!("/proc/{}/cmdline", pid);
                    let is_scythe = std::fs::read_to_string(&proc_cmd)
                        .map(|cmd| cmd.contains("scythe") || cmd.contains("vrec"))
                        .unwrap_or(false);
                    if is_scythe {
                        // Overlay already open: toggle it closed instantly!
                        unsafe { libc::kill(pid, libc::SIGTERM) };
                        let _ = std::fs::remove_file(&pid_path);
                        return true;
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    let my_pid = std::process::id().to_string();
    let _ = std::fs::write(&pid_path, my_pid);
    false
}

fn ensure_daemon_running_async() {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        if UnixStream::connect(scythe::ipc::get_socket_path()).is_ok()
            || UnixStream::connect(scythe::ipc::get_legacy_socket_path()).is_ok() {
            return;
        }
        if scythe::hyprland_binds::is_hyprland() {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "start", "xdg-desktop-portal-hyprland"])
                .status();
        }
    }
    let _ = get_daemon_cmd()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn ensure_daemon_running() {
    if query_status().is_err() {
        #[cfg(target_os = "linux")]
        {
            if scythe::hyprland_binds::is_hyprland() {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "start", "xdg-desktop-portal-hyprland"])
                    .status();
            }
        }
        let _ = get_daemon_cmd()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        for _ in 0..25 {
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
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_ACCESS_RIGHTS};

            if let Ok(handle) = OpenMutexW(SYNCHRONIZATION_ACCESS_RIGHTS(0x00100000), false, windows::core::w!("Global\\scythe_hotkeys_single_instance")) {
                let _ = CloseHandle(handle);
                return;
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

fn send_with_notification(cmd: Command, title: &str, subtitle: &str, icon: ToastIcon) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_daemon_running();
    match send_command(cmd) {
        Ok(()) => {
            show_shadowplay_toast(title, subtitle, icon);
            Ok(())
        }
        Err(e) => {
            show_shadowplay_toast("SCYTHE", "Error: failed to connect to daemon", ToastIcon::Error);
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
                show_shadowplay_toast("RECORDING", "Recording saved", ToastIcon::Save);
            } else {
                show_shadowplay_toast("RECORDING", "Recording started", ToastIcon::Record);
            }
            Ok(())
        }
        Err(e) => {
            show_shadowplay_toast("SCYTHE", "Error: failed to connect to daemon", ToastIcon::Error);
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
                show_shadowplay_toast("MOUSE CURSOR", "Visible in recording", ToastIcon::Cursor);
            } else {
                show_shadowplay_toast("MOUSE CURSOR", "Hidden from recording", ToastIcon::Cursor);
            }
            Ok(())
        }
        Err(e) => {
            show_shadowplay_toast("SCYTHE", "Error: failed to connect to daemon", ToastIcon::Error);
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
                return send_with_notification(Command::SaveReplay, "INSTANT REPLAY", "Saved to Videos", ToastIcon::Replay);
            }
            "--notify-save" => {
                show_shadowplay_toast("INSTANT REPLAY", "Saved to Videos", ToastIcon::Replay);
                return Ok(());
            }
            "--notify-start" => {
                show_shadowplay_toast("RECORDING", "Recording started", ToastIcon::Record);
                return Ok(());
            }
            "--notify-stop" => {
                show_shadowplay_toast("RECORDING", "Recording saved", ToastIcon::Save);
                return Ok(());
            }
            "--toast" => {
                let title = args.get(2).map(|s| s.as_str()).unwrap_or("SCYTHE");
                let subtitle = args.get(3).map(|s| s.as_str()).unwrap_or("");
                let icon_name = args.get(4).map(|s| s.as_str()).unwrap_or("info");
                scythe::overlay_egui::run_egui_toast(title, subtitle, ToastIcon::from_name(icon_name));
                return Ok(());
            }
            "--menu" => {
                ensure_daemon_running_async();
                ensure_hotkeys_running();
                if check_and_toggle_overlay() {
                    return Ok(());
                }
                scythe::overlay_egui::run_egui_overlay();
                return Ok(());
            }
            "--egui" => {
                ensure_daemon_running_async();
                ensure_hotkeys_running();
                if check_and_toggle_overlay() {
                    return Ok(());
                }
                scythe::overlay_egui::run_egui_overlay();
                return Ok(());
            }
            "--gtk" => {
                ensure_daemon_running();
                ensure_hotkeys_running();
                #[cfg(target_os = "linux")]
                scythe::overlay::show_menu_overlay();
                #[cfg(not(target_os = "linux"))]
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
                return send_with_notification(Command::StartRecording, "RECORDING", "Recording started", ToastIcon::Record);
            }
            "--stop" => {
                return send_with_notification(Command::StopRecording, "RECORDING", "Recording saved", ToastIcon::Save);
            }
            "--reload" => {
                ensure_daemon_running();
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
                println!("  scythe-ui --gtk          Open legacy GTK layer-shell overlay (Linux)");
                println!("  scythe-ui --hotkeys      Run global hotkey manager in background");
                println!("  scythe-ui --status       Query current daemon status");
                println!("  scythe-ui --save         Save instant replay and show notification");
                println!("  scythe-ui --notify-save  Show instant replay saved notification toast");
                println!("  scythe-ui --notify-start Show recording started notification toast");
                println!("  scythe-ui --notify-stop  Show recording saved notification toast");
                println!("  scythe-ui --toast <t> <s> Show custom ShadowPlay notification toast");
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
        ensure_daemon_running_async();
        ensure_hotkeys_running();
        if check_and_toggle_overlay() {
            return Ok(());
        }
        scythe::overlay_egui::run_egui_overlay();
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

    let mut current_config = scythe::config::ScytheConfig::load();
    let mut registered_hotkeys: Vec<HotKey> = Vec::new();

    let register_all = |mgr: &GlobalHotKeyManager, cfg: &scythe::config::ScytheConfig, reg: &mut Vec<HotKey>| -> (Option<u32>, Option<u32>, Option<u32>, Option<u32>) {
        for hk in reg.drain(..) {
            let _ = mgr.unregister(hk);
        }
        let is_on_hyprland = scythe::hyprland_binds::is_hyprland();
        let m_menu = scythe::hotkey::parse_hotkey(&cfg.menu_hotkey).or_else(|| Some(HotKey::new(Some(Modifiers::ALT), Code::KeyZ)));
        let m_save = scythe::hotkey::parse_hotkey(&cfg.save_hotkey).or_else(|| Some(HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR)));
        let m_rec = scythe::hotkey::parse_hotkey(&cfg.record_hotkey).or_else(|| Some(HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F9)));
        let m_cur = scythe::hotkey::parse_hotkey(&cfg.cursor_hotkey).or_else(|| Some(HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::F10)));

        let mut id_menu = None;
        let mut id_save = None;
        let mut id_rec = None;
        let mut id_cur = None;

        if !is_on_hyprland {
            if let Some(hk) = m_menu {
                id_menu = Some(hk.id());
                let _ = mgr.register(hk);
                reg.push(hk);
            }
        }
        if let Some(hk) = m_save {
            id_save = Some(hk.id());
            let _ = mgr.register(hk);
            reg.push(hk);
        }
        if let Some(hk) = m_rec {
            id_rec = Some(hk.id());
            let _ = mgr.register(hk);
            reg.push(hk);
        }
        if let Some(hk) = m_cur {
            id_cur = Some(hk.id());
            let _ = mgr.register(hk);
            reg.push(hk);
        }
        (id_menu, id_save, id_rec, id_cur)
    };

    let (mut id_menu, mut id_save, mut id_rec, mut id_cur) = register_all(&manager, &current_config, &mut registered_hotkeys);

    println!("Listening for global hotkeys (Overlay: {}, Replay: {}, Record: {}, Cursor: {})...",
        current_config.menu_hotkey, current_config.save_hotkey, current_config.record_hotkey, current_config.cursor_hotkey);

    let receiver = GlobalHotKeyEvent::receiver();
    let mut last_config_check = std::time::Instant::now();

    loop {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                PeekMessageW, TranslateMessage, DispatchMessageW, MSG, PM_REMOVE,
            };
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if last_config_check.elapsed() > std::time::Duration::from_millis(800) {
            last_config_check = std::time::Instant::now();
            let new_cfg = scythe::config::ScytheConfig::load();
            if new_cfg.menu_hotkey != current_config.menu_hotkey
                || new_cfg.save_hotkey != current_config.save_hotkey
                || new_cfg.record_hotkey != current_config.record_hotkey
                || new_cfg.cursor_hotkey != current_config.cursor_hotkey
            {
                current_config = new_cfg;
                let (im, is, ir, ic) = register_all(&manager, &current_config, &mut registered_hotkeys);
                id_menu = im;
                id_save = is;
                id_rec = ir;
                id_cur = ic;
            }
        }

        while let Ok(event) = receiver.try_recv() {
            if Some(event.id) == id_menu {
                std::thread::spawn(|| {
                    ensure_daemon_running();
                    let _ = get_ui_cmd().arg("--menu").spawn();
                });
            } else if Some(event.id) == id_save {
                std::thread::spawn(|| {
                    let _ = send_with_notification(Command::SaveReplay, "INSTANT REPLAY", "Saved to Videos", ToastIcon::Replay);
                });
            } else if Some(event.id) == id_rec {
                std::thread::spawn(|| {
                    let _ = handle_toggle_recording();
                });
            } else if Some(event.id) == id_cur {
                std::thread::spawn(|| {
                    let _ = handle_toggle_cursor();
                });
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
