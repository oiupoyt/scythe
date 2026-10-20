use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_true() -> bool { true }
fn default_replay_duration() -> u32 { 60 }
fn default_replay_bitrate() -> u32 { 18000 }
fn default_record_bitrate() -> u32 { 20000 }
fn default_fps() -> u32 { 60 }
fn default_video_codec() -> String { "h264".to_string() }
fn default_audio_device() -> String { "default".to_string() }
fn default_audio_mode() -> String { "system".to_string() }
fn default_save_hotkey() -> String { "Ctrl+Shift+R".to_string() }
fn default_menu_hotkey() -> String { "Alt+Z".to_string() }
fn default_record_hotkey() -> String { "Ctrl+Shift+F9".to_string() }
fn default_cursor_hotkey() -> String { "Ctrl+Shift+F10".to_string() }
fn default_mic_volume() -> f32 { 0.60 }
fn default_system_volume() -> f32 { 1.00 }
fn default_accent_color() -> String { "blue".to_string() }

fn default_stream_service() -> String { "twitch".to_string() }
fn default_stream_url() -> String { "rtmp://live.twitch.tv/app/".to_string() }
fn default_screenshot_hotkey() -> String { "Ctrl+Shift+S".to_string() }
fn default_screenshot_format() -> String { "png".to_string() }
fn default_video_quality_mode() -> String { "cbr".to_string() }
fn default_color_range() -> String { "limited".to_string() }
fn default_audio_codec() -> String { "opus".to_string() }
fn default_screenshot_quality() -> u32 { 90 }
fn default_screenshot_preset() -> String { "balanced".to_string() }
fn default_pause_hotkey() -> String { "Ctrl+Shift+F8".to_string() }
fn default_mute_mic_hotkey() -> String { "Ctrl+Shift+M".to_string() }
fn default_mute_sys_hotkey() -> String { "Ctrl+Shift+N".to_string() }
fn default_screenshot_resolution() -> String { "native".to_string() }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScytheConfig {
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default = "default_true")]
    pub replay_enabled: bool,
    #[serde(default = "default_replay_duration")]
    pub replay_duration_sec: u32,
    #[serde(default = "default_replay_bitrate")]
    pub replay_bitrate_kbps: u32,
    
    #[serde(default = "default_record_bitrate")]
    pub record_bitrate_kbps: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    #[serde(default = "ScytheConfig::default_output_directory")]
    pub output_directory: String,
    #[serde(default = "default_true")]
    pub show_cursor: bool,
    #[serde(default = "default_audio_device")]
    pub audio_device: String,
    #[serde(default = "default_audio_mode")]
    pub audio_mode: String,
    #[serde(default = "default_mic_volume")]
    pub mic_volume: f32,
    #[serde(default = "default_system_volume")]
    pub system_volume: f32,
    
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub autostart_replay: bool,
    #[serde(default)]
    pub autostart_overlay: bool,
    #[serde(default = "default_save_hotkey")]
    pub save_hotkey: String,
    #[serde(default = "default_menu_hotkey")]
    pub menu_hotkey: String,
    #[serde(default = "default_record_hotkey")]
    pub record_hotkey: String,
    #[serde(default = "default_cursor_hotkey")]
    pub cursor_hotkey: String,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub do_not_ask_updates: bool,
    #[serde(default = "default_stream_service")]
    pub stream_service: String,
    #[serde(default = "default_stream_url")]
    pub stream_url: String,
    #[serde(default)]
    pub stream_key: String,

    #[serde(default = "default_screenshot_hotkey")]
    pub screenshot_hotkey: String,
    #[serde(default = "ScytheConfig::default_screenshot_directory")]
    pub screenshot_directory: String,
    #[serde(default = "default_screenshot_format")]
    pub screenshot_format: String,
    #[serde(default)]
    pub screenshot_cursor: bool,

    #[serde(default = "default_video_quality_mode")]
    pub video_quality_mode: String,
    #[serde(default = "default_color_range")]
    pub color_range: String,
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    #[serde(default)]
    pub record_low_power: bool,
    #[serde(default = "default_true")]
    pub show_replay_notifications: bool,

    #[serde(default = "default_true")]
    pub restart_replay_on_save: bool,
    #[serde(default)]
    pub autostart_power_only: bool,

    #[serde(default = "default_screenshot_quality")]
    pub screenshot_quality: u32,
    #[serde(default = "default_screenshot_preset")]
    pub screenshot_preset: String,
    #[serde(default = "default_pause_hotkey")]
    pub pause_hotkey: String,
    #[serde(default = "default_mute_mic_hotkey")]
    pub mute_mic_hotkey: String,
    #[serde(default = "default_mute_sys_hotkey")]
    pub mute_sys_hotkey: String,
    #[serde(default = "default_screenshot_resolution")]
    pub screenshot_resolution: String,
    #[serde(default)]
    pub use_monospace_font: bool,
}

pub type VrecConfig = ScytheConfig;

impl Default for ScytheConfig {
    fn default() -> Self {
        Self {
            accent_color: default_accent_color(),
            replay_enabled: true,
            replay_duration_sec: 60,
            replay_bitrate_kbps: 18000,
            
            record_bitrate_kbps: 20000,
            fps: 60,
            video_codec: "h264".to_string(),
            output_directory: Self::default_output_directory(),
            show_cursor: true,
            audio_device: "default".to_string(),
            audio_mode: "system".to_string(),
            mic_volume: default_mic_volume(),
            system_volume: default_system_volume(),
            
            autostart: false,
            autostart_replay: false,
            autostart_overlay: false,
            save_hotkey: "Ctrl+Shift+R".to_string(),
            menu_hotkey: "Alt+Z".to_string(),
            record_hotkey: "Ctrl+Shift+F9".to_string(),
            cursor_hotkey: "Ctrl+Shift+F10".to_string(),
            auto_check_updates: true,
            do_not_ask_updates: false,
            stream_service: "twitch".to_string(),
            stream_url: "rtmp://live.twitch.tv/app/".to_string(),
            stream_key: String::new(),

            screenshot_hotkey: default_screenshot_hotkey(),
            screenshot_directory: Self::default_screenshot_directory(),
            screenshot_format: default_screenshot_format(),
            screenshot_cursor: false,

            video_quality_mode: default_video_quality_mode(),
            color_range: default_color_range(),
            audio_codec: default_audio_codec(),
            record_low_power: false,
            show_replay_notifications: true,

            restart_replay_on_save: true,
            autostart_power_only: false,

            screenshot_quality: default_screenshot_quality(),
            screenshot_preset: default_screenshot_preset(),
            pause_hotkey: default_pause_hotkey(),
            mute_mic_hotkey: default_mute_mic_hotkey(),
            mute_sys_hotkey: default_mute_sys_hotkey(),
            screenshot_resolution: default_screenshot_resolution(),
            use_monospace_font: false,
        }
    }
}

impl ScytheConfig {
    pub fn default_output_directory() -> String {
        if let Some(video) = dirs::video_dir() {
            return video.to_string_lossy().to_string();
        }
        if let Some(home) = dirs::home_dir() {
            return home.join("Videos").to_string_lossy().to_string();
        }
        "~/Videos".to_string()
    }

    pub fn default_screenshot_directory() -> String {
        if let Some(pic) = dirs::picture_dir() {
            return pic.join("Screenshots").to_string_lossy().to_string();
        }
        if let Some(home) = dirs::home_dir() {
            return home.join("Pictures").join("Screenshots").to_string_lossy().to_string();
        }
        Self::default_output_directory()
    }

    pub fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("scythe").join("config.json")
    }

    pub fn legacy_config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("vrec").join("config.json")
    }

    pub fn expand_tilde(path: &str) -> PathBuf {
        let trimmed = path.trim();
        if let Some(sub) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
            if let Some(home) = dirs::home_dir() {
                return home.join(sub);
            }
        } else if trimmed == "~"
            && let Some(home) = dirs::home_dir() {
                return home;
        }
        PathBuf::from(trimmed)
    }

    pub fn resolve_save_path(&self, filename: &str) -> String {
        let mut p = Self::expand_tilde(&self.output_directory);
        let _ = fs::create_dir_all(&p);
        p.push(filename);

        if !p.exists() {
            return p.to_string_lossy().to_string();
        }

        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("recording");
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
        let parent = p.parent().unwrap_or_else(|| std::path::Path::new("."));

        for counter in 1..10000 {
            let candidate = parent.join(format!("{}_{}.{}", stem, counter, ext));
            if !candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }

        p.to_string_lossy().to_string()
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(mut cfg) = serde_json::from_str::<Self>(&content) {
                if cfg.autostart && !cfg.autostart_replay {
                    cfg.autostart_replay = true;
                }
                return cfg;
        }
        let legacy = Self::legacy_config_path();
        if let Ok(content) = fs::read_to_string(&legacy)
            && let Ok(mut cfg) = serde_json::from_str::<Self>(&content) {
                // If the legacy config used the old default directory, update it to the new Scythe directory
                if cfg.output_directory.ends_with("/Videos/vrec") || cfg.output_directory.ends_with("\\Videos\\vrec") {
                    cfg.output_directory = Self::default_output_directory();
                }
                if cfg.autostart && !cfg.autostart_replay {
                    cfg.autostart_replay = true;
                }
                let _ = cfg.save();
                return cfg;
        }
        let default_cfg = Self::default();
        let _ = default_cfg.save();
        default_cfg
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, &json)?;

        // Also update legacy config if its directory exists, ensuring backward compatibility
        let legacy = Self::legacy_config_path();
        if let Some(parent) = legacy.parent()
            && parent.exists() {
                let _ = fs::write(legacy, &json);
        }

        self.sync_autostart();

        Ok(())
    }

    pub fn sync_autostart(&self) {
        #[cfg(unix)]
        {
            let home = match std::env::var("HOME") {
                Ok(h) => PathBuf::from(h),
                Err(_) => return,
            };

            let autostart_dir = home.join(".config").join("autostart");
            let _ = fs::create_dir_all(&autostart_dir);

            // 1. Replay daemon autostart entry
            let daemon_desktop = autostart_dir.join("scythe-daemon.desktop");
            if self.autostart_replay {
                let content = "[Desktop Entry]\n\
Type=Application\n\
Name=Scythe Replay Engine\n\
Comment=Scythe Background Screen Recorder & Instant Replay Daemon\n\
Icon=media-record\n\
Exec=scythe daemon\n\
Terminal=false\n\
Hidden=false\n\
X-GNOME-Autostart-enabled=true\n";
                let _ = fs::write(&daemon_desktop, content);
            } else {
                let _ = fs::remove_file(&daemon_desktop);
            }

            // 2. Overlay autostart entry
            let overlay_desktop = autostart_dir.join("scythe-overlay.desktop");
            if self.autostart_overlay {
                let content = "[Desktop Entry]\n\
Type=Application\n\
Name=Scythe Overlay\n\
Comment=Scythe Screen Recorder HUD Overlay\n\
Icon=media-record\n\
Exec=scythe --menu\n\
Terminal=false\n\
Hidden=false\n\
X-GNOME-Autostart-enabled=true\n";
                let _ = fs::write(&overlay_desktop, content);
            } else {
                let _ = fs::remove_file(&overlay_desktop);
            }

            // 3. Hyprland execs.lua integration if present
            let hypr_execs_lua = home.join(".config").join("hypr").join("hyprland").join("execs.lua");
            if hypr_execs_lua.exists()
                && let Ok(mut text) = fs::read_to_string(&hypr_execs_lua) {
                    let mut modified = false;

                    let daemon_cmd_pattern = "scythe daemon";
                    let daemon_cmd_line = "    hl.exec_cmd(\"scythe daemon\")\n";
                    if self.autostart_replay {
                        if !text.contains(daemon_cmd_pattern)
                            && let Some(pos) = text.rfind("end)") {
                                text.insert_str(pos, daemon_cmd_line);
                                modified = true;
                            }
                    } else if text.contains(daemon_cmd_pattern) {
                        text = text.lines()
                            .filter(|l| !l.contains(daemon_cmd_pattern))
                            .collect::<Vec<_>>()
                            .join("\n") + "\n";
                        modified = true;
                    }

                    let overlay_cmd_pattern = "scythe --menu";
                    let overlay_cmd_line = "    hl.exec_cmd(\"scythe --menu\")\n";
                    if self.autostart_overlay {
                        if !text.contains(overlay_cmd_pattern)
                            && let Some(pos) = text.rfind("end)") {
                                text.insert_str(pos, overlay_cmd_line);
                                modified = true;
                            }
                    } else if text.contains(overlay_cmd_pattern) {
                        text = text.lines()
                            .filter(|l| !l.contains(overlay_cmd_pattern))
                            .collect::<Vec<_>>()
                            .join("\n") + "\n";
                        modified = true;
                    }

                    if modified {
                        let _ = fs::write(&hypr_execs_lua, text);
                    }
                }
        }
    }

    /// Enable or disable session autostart for the daemon and the overlay.
    ///
    /// Linux only: systemd user units and XDG autostart entries. Windows has no
    /// equivalent implementation yet, so the call is a no-op there.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub fn apply_autostart_system(enable_replay: bool, enable_overlay: bool) {
        #[cfg(target_os = "linux")]
        {
            // 1. systemd user service enable / disable
            if enable_replay {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "enable", "scythe-daemon.service"])
                    .status();
            } else {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "disable", "scythe-daemon.service"])
                    .status();
            }

            // 2. Standard XDG autostart desktop entry for session login
            if let Some(config_dir) = dirs::config_dir() {
                let autostart_dir = config_dir.join("autostart");
                let _ = std::fs::create_dir_all(&autostart_dir);
                let daemon_desktop = autostart_dir.join("scythe-daemon.desktop");
                let overlay_desktop = autostart_dir.join("scythe-overlay.desktop");

                if enable_replay {
                    let content = "[Desktop Entry]\nType=Application\nName=Scythe Replay Daemon\nExec=scythe daemon\nTerminal=false\nCategories=AudioVideo;Recorder;\nX-GNOME-Autostart-enabled=true\n";
                    let _ = std::fs::write(&daemon_desktop, content);
                } else {
                    let _ = std::fs::remove_file(&daemon_desktop);
                }

                if enable_overlay {
                    let content = "[Desktop Entry]\nType=Application\nName=Scythe Overlay\nExec=scythe --menu\nTerminal=false\nCategories=AudioVideo;Recorder;\nX-GNOME-Autostart-enabled=true\n";
                    let _ = std::fs::write(&overlay_desktop, content);
                } else {
                    let _ = std::fs::remove_file(&overlay_desktop);
                }
            }
        }
    }

    pub fn notify_daemon_reload() {
        std::thread::spawn(|| {
            let _ = crate::ipc::send_command(crate::ipc::Command::ReloadConfig);
        });
    }

    /// Formats video filenames with informative local timestamps, e.g. "Replay-18-30-00_05-09-2026.mp4"
    pub fn is_ac_power_connected() -> bool {
        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
                let mut has_power_status = false;
                for entry in entries.flatten() {
                    let path = entry.path();
                    // Check AC online status
                    let online_path = path.join("online");
                    if let Ok(content) = std::fs::read_to_string(&online_path) {
                        has_power_status = true;
                        if content.trim() == "1" {
                            return true;
                        }
                    }
                    // Check battery status
                    let status_path = path.join("status");
                    if let Ok(content) = std::fs::read_to_string(&status_path) {
                        let s = content.trim().to_lowercase();
                        if s == "charging" || s == "full" {
                            return true;
                        }
                    }
                }
                if has_power_status {
                    return false;
                }
            }
            true
        }

        #[cfg(target_os = "windows")]
        {
            unsafe {
                use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
                let mut status = SYSTEM_POWER_STATUS::default();
                if GetSystemPowerStatus(&mut status).is_ok() {
                    return status.ACLineStatus == 1;
                }
            }
            true
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        true
    }

    pub fn format_video_filename(prefix: &str, ext: &str) -> String {
        unsafe {
            let t = libc::time(std::ptr::null_mut());
            let mut tm = std::mem::zeroed::<libc::tm>();
            #[cfg(unix)]
            libc::localtime_r(&t, &mut tm);
            #[cfg(windows)]
            libc::localtime_s(&mut tm, &t);

            let hour = tm.tm_hour;
            let min = tm.tm_min;
            let sec = tm.tm_sec;
            let day = tm.tm_mday;
            let month = tm.tm_mon + 1;
            let year = tm.tm_year + 1900;

            format!("{}-{:02}-{:02}-{:02}_{:02}-{:02}-{:04}.{}", prefix, hour, min, sec, day, month, year, ext)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults_and_json() {
        let cfg = ScytheConfig::default();
        assert_eq!(cfg.accent_color, "blue");
        assert_eq!(cfg.replay_duration_sec, 60);
        assert_eq!(cfg.replay_bitrate_kbps, 18000);
        assert_eq!(cfg.fps, 60);
        assert_eq!(cfg.video_codec, "h264");
        assert_eq!(cfg.audio_mode, "system");
        assert!(cfg.show_cursor);
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: ScytheConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.accent_color, "blue");
        assert_eq!(parsed.replay_duration_sec, 60);
        assert_eq!(parsed.audio_mode, "system");
        assert!(parsed.show_cursor);
        assert!((parsed.mic_volume - 0.60).abs() < 1e-4);
        assert!((parsed.system_volume - 1.00).abs() < 1e-4);
        assert!(parsed.auto_check_updates);
        assert!(!parsed.do_not_ask_updates);
        assert_eq!(parsed.stream_service, "twitch");
        assert_eq!(parsed.stream_url, "rtmp://live.twitch.tv/app/");
        assert_eq!(parsed.stream_key, "");

        assert!(parsed.restart_replay_on_save);
        assert!(!parsed.autostart_power_only);
        assert_eq!(parsed.video_quality_mode, "cbr");
        assert_eq!(parsed.color_range, "limited");
        assert_eq!(parsed.audio_codec, "opus");
        assert!(!parsed.record_low_power);
        assert!(parsed.show_replay_notifications);
        assert_eq!(parsed.screenshot_hotkey, "Ctrl+Shift+S");
        assert_eq!(parsed.screenshot_format, "png");
        assert!(!parsed.screenshot_cursor);

        assert_eq!(parsed.screenshot_quality, 90);
        assert_eq!(parsed.screenshot_preset, "balanced");
        assert_eq!(parsed.pause_hotkey, "Ctrl+Shift+F8");
        assert_eq!(parsed.mute_mic_hotkey, "Ctrl+Shift+M");
        assert_eq!(parsed.mute_sys_hotkey, "Ctrl+Shift+N");
        assert_eq!(parsed.screenshot_resolution, "native");
        assert!(!parsed.use_monospace_font);
    }

    #[test]
    fn test_format_video_filename() {
        let name = ScytheConfig::format_video_filename("Replay", "mp4");
        assert!(name.starts_with("Replay-"));
        assert!(name.ends_with(".mp4"));
        // Check pattern Replay-HH-MM-SS_DD-MM-YYYY.mp4
        let without_ext = name.strip_suffix(".mp4").unwrap();
        let top_parts: Vec<&str> = without_ext.split('_').collect();
        assert_eq!(top_parts.len(), 2, "Filename underscore separation mismatch: {}", name);
        let time_parts: Vec<&str> = top_parts[0].split('-').collect();
        assert_eq!(time_parts.len(), 4, "Time parts mismatch: {}", top_parts[0]);
        assert_eq!(time_parts[0], "Replay");
        for part in &time_parts[1..] {
            assert!(part.chars().all(|c| c.is_ascii_digit()));
        }
        let date_parts: Vec<&str> = top_parts[1].split('-').collect();
        assert_eq!(date_parts.len(), 3, "Date parts mismatch: {}", top_parts[1]);
        for part in &date_parts {
            assert!(part.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn test_resolve_save_path_collision() {
        let temp_dir = std::env::temp_dir().join(format!("scythe_test_collision_{}", std::process::id()));
        let cfg = ScytheConfig {
            output_directory: temp_dir.to_string_lossy().to_string(),
            ..Default::default()
        };

        let path1 = cfg.resolve_save_path("test_clip.mp4");
        assert!(path1.ends_with("test_clip.mp4"));
        fs::write(&path1, b"dummy").unwrap();

        let path2 = cfg.resolve_save_path("test_clip.mp4");
        assert!(path2.ends_with("test_clip_1.mp4"), "Expected collision to append _1: {}", path2);
        fs::write(&path2, b"dummy2").unwrap();

        let path3 = cfg.resolve_save_path("test_clip.mp4");
        assert!(path3.ends_with("test_clip_2.mp4"), "Expected collision to append _2: {}", path3);

        let _ = fs::remove_dir_all(temp_dir);
    }
}
