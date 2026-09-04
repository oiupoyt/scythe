use std::process::Command;
use std::env;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use crate::config::VrecConfig;

pub fn is_hyprland() -> bool {
    env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
        || env::var("XDG_CURRENT_DESKTOP")
            .map(|d| d.to_lowercase().contains("hyprland"))
            .unwrap_or(false)
}

/// Convert standard hotkey format (e.g. "Ctrl+Shift+R") to Hyprland bind format ("CONTROL_SHIFT, R")
pub fn hotkey_to_hyprland(hotkey: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = hotkey.split('+').map(|s| s.trim()).collect();
    if parts.is_empty() {
        return None;
    }

    let mut mods = Vec::new();
    let mut key = "";

    for (i, &part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            key = part;
        } else {
            let m = match part.to_lowercase().as_str() {
                "ctrl" | "control" => "CONTROL",
                "shift" => "SHIFT",
                "alt" => "ALT",
                "super" | "win" | "meta" => "SUPER",
                _ => continue,
            };
            mods.push(m);
        }
    }

    if key.is_empty() {
        return None;
    }

    let mod_str = if mods.is_empty() {
        "".to_string()
    } else {
        mods.join("_")
    };

    Some((mod_str, key.to_string()))
}

/// Dynamically inject binds and window rules into running Hyprland without touching hyprland.conf
pub fn register_hyprland_binds(config: &VrecConfig) {
    if !is_hyprland() {
        return;
    }

    // Register overlay window rules so the UI floats and centers cleanly
    let rules = [
        "float, class:^(vrec-overlay)$",
        "center, class:^(vrec-overlay)$",
        "pin, class:^(vrec-overlay)$",
        "stayfocused, class:^(vrec-overlay)$",
        "noborder, class:^(vrec-overlay)$",
    ];
    for rule in rules {
        let _ = Command::new("hyprctl")
            .args(["keyword", "windowrulev2", rule])
            .output();
    }

    let binds = [
        (&config.menu_hotkey, "exec, vrec-ui --menu"),
        (&config.save_hotkey, "exec, vrec-ui --save"),
        (&config.record_hotkey, "exec, vrec-ui --record"),
    ];

    for (hotkey, action) in binds {
        if let Some((mods, key)) = hotkey_to_hyprland(hotkey) {
            let bind_arg = if mods.is_empty() {
                format!("{}, {}", key, action)
            } else {
                format!("{}, {}, {}", mods, key, action)
            };
            let res = Command::new("hyprctl")
                .args(["keyword", "bind", &bind_arg])
                .output();
            if let Ok(out) = res {
                if !out.status.success() {
                    eprintln!("hyprctl keyword bind note: {:?}", String::from_utf8_lossy(&out.stderr));
                } else {
                    println!("Hyprland dynamic bind active: {}", bind_arg);
                }
            }
        }
    }
}

/// Dynamically remove binds from running Hyprland
pub fn unregister_hyprland_binds(config: &VrecConfig) {
    if !is_hyprland() {
        return;
    }

    let hotkeys = [
        &config.menu_hotkey,
        &config.save_hotkey,
        &config.record_hotkey,
    ];

    for hotkey in hotkeys {
        if let Some((mods, key)) = hotkey_to_hyprland(hotkey) {
            let unbind_arg = if mods.is_empty() {
                key
            } else {
                format!("{}, {}", mods, key)
            };
            let _ = Command::new("hyprctl")
                .args(["keyword", "unbind", &unbind_arg])
                .output();
        }
    }
}

/// Find Hyprland socket2 for live event listening
fn get_hyprland_socket2() -> Option<PathBuf> {
    let sig = env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let xdg_runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
    
    let path = PathBuf::from(format!("{}/hypr/{}/.socket2.sock", xdg_runtime, sig));
    if path.exists() {
        return Some(path);
    }
    let fallback = PathBuf::from(format!("/tmp/hypr/{}/.socket2.sock", sig));
    if fallback.exists() {
        return Some(fallback);
    }
    None
}

/// Background watcher for Hyprland config reloads so dynamic binds are never lost
pub fn spawn_hyprland_reload_watcher() {
    if !is_hyprland() {
        return;
    }

    std::thread::spawn(|| {
        loop {
            if let Some(sock_path) = get_hyprland_socket2() {
                if let Ok(stream) = UnixStream::connect(&sock_path) {
                    let reader = BufReader::new(stream);
                    for line in reader.lines().map_while(Result::ok) {
                        if line.starts_with("configreloaded") {
                            println!("Hyprland config reload detected! Re-registering vrec dynamic binds...");
                            let config = VrecConfig::load();
                            register_hyprland_binds(&config);
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_to_hyprland_conversion() {
        assert_eq!(
            hotkey_to_hyprland("Ctrl+Shift+R"),
            Some(("CONTROL_SHIFT".to_string(), "R".to_string()))
        );
        assert_eq!(
            hotkey_to_hyprland("Alt+Z"),
            Some(("ALT".to_string(), "Z".to_string()))
        );
        assert_eq!(
            hotkey_to_hyprland("Ctrl+Shift+F9"),
            Some(("CONTROL_SHIFT".to_string(), "F9".to_string()))
        );
        assert_eq!(
            hotkey_to_hyprland("F12"),
            Some(("".to_string(), "F12".to_string()))
        );
        assert_eq!(
            hotkey_to_hyprland("Super+Alt+S"),
            Some(("SUPER_ALT".to_string(), "S".to_string()))
        );
    }
}

