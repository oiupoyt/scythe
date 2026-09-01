use vrec::ipc::Command;
use std::os::unix::net::UnixStream;
use std::io::Write;
use std::env;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};

fn send_command(cmd: Command) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
    let mut stream = UnixStream::connect(&socket_path)?;
    let payload = serde_json::to_vec(&cmd)?;
    let len_buf = (payload.len() as u32).to_le_bytes();
    stream.write_all(&len_buf)?;
    stream.write_all(&payload)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Starting vrec UI/Hotkey process...");
    
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "".to_string()).to_lowercase();
    
    let is_gnome = desktop.contains("gnome") || desktop.contains("ubuntu");
    let is_kde = desktop.contains("kde");
    
    if session_type.to_lowercase() == "wayland" {
        if is_gnome || is_kde {
            println!("WARNING: wlr-layer-shell unsupported on GNOME/KDE Wayland. Degrading to hotkey-only mode.");
            println!("Note: Global hotkeys may also fail to register on some Wayland compositors.");
        } else {
            println!("wlroots-based compositor detected. (wlr-layer-shell overlay would be spawned here).");
        }
    } else {
        println!("X11 session detected. (X11 override-redirect overlay would be spawned here).");
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
        if let Ok(event) = receiver.recv()
            && event.state == global_hotkey::HotKeyState::Pressed {
                if event.id == save_hotkey.id() {
                    println!("Hotkey triggered! Sending SaveReplay command to daemon...");
                    let _ = send_command(Command::SaveReplay);
                } else if event.id == menu_hotkey.id() {
                    println!("Alt+Z triggered! Opening GUI Overlay...");
                    if env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string()).to_lowercase() != "wayland" {
                        vrec::overlay::show_saved_overlay();
                    }
                }
            }
    }
}
