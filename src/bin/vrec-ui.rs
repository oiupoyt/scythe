use vrec::ipc::{Command, send_command};
use std::env;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--save" => {
                let res = send_command(Command::SaveReplay);
                vrec::overlay::show_notification_overlay();
                return res;
            }
            "--menu" => {
                vrec::overlay::show_menu_overlay();
                return Ok(());
            }
            "--record" | "--toggle" => {
                return send_command(Command::ToggleRecording);
            }
            "--start" => {
                return send_command(Command::StartRecording);
            }
            "--stop" => {
                return send_command(Command::StopRecording);
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
                println!("  vrec-ui --menu         Open GPU Screen Recorder style menu overlay");
                println!("  vrec-ui --save         Save instant replay and show notification");
                println!("  vrec-ui --record       Toggle normal recording on/off");
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
    
    let manager = GlobalHotKeyManager::new().map_err(|e| format!("Failed to init GlobalHotKeyManager: {:?}", e))?;
    let config = vrec::config::VrecConfig::load();
    let save_hotkey = vrec::hotkey::parse_hotkey(&config.save_hotkey)
        .unwrap_or_else(|| HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR));
    let menu_hotkey = vrec::hotkey::parse_hotkey(&config.menu_hotkey)
        .unwrap_or_else(|| HotKey::new(Some(Modifiers::ALT), Code::KeyZ));
    
    let mut registered_count = 0;
    if let Err(e) = manager.register(save_hotkey) {
        eprintln!("Warning: Failed to register save hotkey ({}): {}", config.save_hotkey, e);
    } else {
        println!("Registered save replay hotkey: {}", config.save_hotkey);
        registered_count += 1;
    }

    if let Err(e) = manager.register(menu_hotkey) {
        eprintln!("Warning: Failed to register menu hotkey ({}): {}", config.menu_hotkey, e);
    } else {
        println!("Registered menu overlay hotkey: {}", config.menu_hotkey);
        registered_count += 1;
    }

    if registered_count == 0 {
        eprintln!("Note: Running without global hotkeys. You can bind `vrec-ui --menu` and `vrec-ui --save` to compositor shortcuts.");
    }

    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.recv()
            && event.state == global_hotkey::HotKeyState::Pressed {
                if event.id == save_hotkey.id() {
                    println!("Save replay hotkey pressed. Triggering SaveReplay...");
                    vrec::overlay::show_notification_overlay();
                    let _ = send_command(Command::SaveReplay);
                } else if event.id == menu_hotkey.id() {
                    println!("Menu hotkey pressed. Opening Overlay Menu...");
                    vrec::overlay::show_menu_overlay();
                }
            }
    }
}
