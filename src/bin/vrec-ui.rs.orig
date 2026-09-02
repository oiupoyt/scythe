use vrec::ipc::Command;
use std::os::unix::net::UnixStream;
use std::io::Write;
use std::env;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};

fn send_command(cmd: Command) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
    let mut stream = UnixStream::connect(&socket_path)?;
    let payload = serde_json::to_vec(&cmd)?;
    let len_buf = (payload.len() as u32).to_le_bytes();
    stream.write_all(&len_buf)?;
    stream.write_all(&payload)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--save" => {
                vrec::overlay::show_notification_overlay();
                return send_command(Command::SaveReplay).map_err(|e| e.into());
            }
            "--menu" => {
                vrec::overlay::show_menu_overlay();
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
            println!("WARNING: Wayland native layer shell used. Ensure your compositor allows it.");
            println!("Note: Global hotkeys may fail to register on Wayland compositors.");
        } else {
            println!("wlroots-based compositor detected.");
        }
    } else {
        println!("X11 session detected.");
    }
    
    let manager = GlobalHotKeyManager::new().unwrap();
    let save_hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    let menu_hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyZ);
    
    if let Err(e) = manager.register(save_hotkey) {
        eprintln!("Failed to register global hotkey! Wayland compositor might be blocking it: {}", e);
        eprintln!("Fallback: running without global hotkey. Send commands via socket manually.");
    } else {
        manager.register(menu_hotkey).unwrap();
        println!("Registered global hotkeys:\n - Ctrl+Shift+R (Save Replay)\n - Alt+Z (Open GUI Menu)");
    }

    let receiver = GlobalHotKeyEvent::receiver();
    loop {
        if let Ok(event) = receiver.recv() {
            if event.state == global_hotkey::HotKeyState::Pressed {
                if event.id == save_hotkey.id() {
                    println!("Hotkey triggered! Sending SaveReplay command to daemon...");
                    vrec::overlay::show_notification_overlay();
                    let _ = send_command(Command::SaveReplay);
                } else if event.id == menu_hotkey.id() {
                    println!("Alt+Z triggered! Opening GUI Menu Overlay...");
                    vrec::overlay::show_menu_overlay();
                }
            }
        }
    }
}
