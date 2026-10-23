pub use crate::ipc::ToastIcon;

#[cfg(unix)]
fn find_gsr_notify() -> Option<std::path::PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let local_bin = std::path::PathBuf::from(home).join(".local/bin/gsr-notify");
        if local_bin.exists() {
            return Some(local_bin);
        }
    }
    let usr_bin = std::path::PathBuf::from("/usr/bin/gsr-notify");
    if usr_bin.exists() {
        return Some(usr_bin);
    }
    let usr_local_bin = std::path::PathBuf::from("/usr/local/bin/gsr-notify");
    if usr_local_bin.exists() {
        return Some(usr_local_bin);
    }
    None
}

pub fn show_shadowplay_toast(title: &str, subtitle: &str, icon: ToastIcon) {
    #[cfg(unix)]
    {
        if let Some(gsr_bin) = find_gsr_notify() {
            let _ = std::process::Command::new("pkill")
                .args(["-SIGINT", "-f", "gsr-notify"])
                .status();

            let cfg = crate::config::ScytheConfig::load();
            let bg_hex = match cfg.accent_color.to_lowercase().as_str() {
                "amd" => "DD0031",
                "intel" => "086DB7",
                "blue" => "38BDF8",
                "green" | "nvidia" => "76B900",
                "yellow" => "FAC015",
                "purple" => "A855F7",
                "pink" => "F43F5E",
                _ => "76B900",
            };

            let (icon_arg, icon_color, bg_color) = match icon {
                ToastIcon::Record => (Some("record"), "FFFFFF", bg_hex),
                ToastIcon::Replay | ToastIcon::Save => (Some("replay"), "FFFFFF", bg_hex),
                ToastIcon::Screenshot => (Some("screenshot"), "FFFFFF", bg_hex),
                ToastIcon::Error => (None, "FF0000", "FF0000"),
                _ => (None, "FFFFFF", bg_hex),
            };

            let text = if subtitle.is_empty() {
                title.to_string()
            } else if title.is_empty() {
                subtitle.to_string()
            } else {
                format!("{}: {}", title, subtitle)
            };

            let mut cmd = std::process::Command::new(gsr_bin);
            cmd.args([
                "--text", &text,
                "--timeout", "3.0",
                "--icon-color", icon_color,
                "--bg-color", bg_color,
            ]);
            if let Some(ic) = icon_arg {
                cmd.args(["--icon", ic]);
            }
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());

            if cmd.spawn().is_ok() {
                return;
            }
        }
    }

    let icon_str = match icon {
        ToastIcon::Replay => "replay",
        ToastIcon::Record => "record",
        ToastIcon::Save => "save",
        ToastIcon::Cursor => "cursor",
        ToastIcon::Screenshot => "screenshot",
        ToastIcon::Error => "error",
        ToastIcon::Info => "info",
    };
    let toast_argv = ["--toast", title, subtitle, icon_str];
    if let Ok(self_exe) = std::env::current_exe() {
        let _ = std::process::Command::new(self_exe)
            .args(toast_argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    } else {
        let _ = std::process::Command::new("scythe")
            .args(toast_argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

pub fn ensure_wayland_env() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY").is_err()
            && let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                let candidates = ["wayland-1", "wayland-0", "wayland-2"];
                for c in &candidates {
                    let socket = std::path::Path::new(&runtime_dir).join(c);
                    if socket.exists() {
                        unsafe {
                            std::env::set_var("WAYLAND_DISPLAY", c);
                        }
                        break;
                    }
                }
        }
    }
}
