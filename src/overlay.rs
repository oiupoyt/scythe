pub use crate::ipc::ToastIcon;

pub fn show_shadowplay_toast(title: &str, subtitle: &str, icon: ToastIcon) {
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
