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
fn default_lang() -> String { "en".to_string() }
fn default_theme() -> String { "dark".to_string() }
fn default_save_hotkey() -> String { "Ctrl+Shift+R".to_string() }
fn default_menu_hotkey() -> String { "Alt+Z".to_string() }
fn default_record_hotkey() -> String { "Ctrl+Shift+F9".to_string() }
fn default_cursor_hotkey() -> String { "Ctrl+Shift+F10".to_string() }
fn default_mic_volume() -> f32 { 0.60 }
fn default_system_volume() -> f32 { 1.00 }
fn default_accent_color() -> String { "blue".to_string() }

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
    
    #[serde(default)]
    pub record_enabled: bool,
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
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub ui_color_theme: String,
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
}

pub type VrecConfig = ScytheConfig;

impl Default for ScytheConfig {
    fn default() -> Self {
        Self {
            accent_color: default_accent_color(),
            replay_enabled: true,
            replay_duration_sec: 60,
            replay_bitrate_kbps: 18000,
            
            record_enabled: false,
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
            language: "en".to_string(),
            ui_color_theme: "dark".to_string(),
            save_hotkey: "Ctrl+Shift+R".to_string(),
            menu_hotkey: "Alt+Z".to_string(),
            record_hotkey: "Ctrl+Shift+F9".to_string(),
            cursor_hotkey: "Ctrl+Shift+F10".to_string(),
            auto_check_updates: true,
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
        if let Some(sub) = trimmed.strip_prefix("~/") {
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
Exec=scythe-daemon\n\
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
Exec=scythe-ui --menu\n\
Terminal=false\n\
Hidden=false\n\
X-GNOME-Autostart-enabled=true\n";
                let _ = fs::write(&overlay_desktop, content);
            } else {
                let _ = fs::remove_file(&overlay_desktop);
            }

            // 3. Hyprland execs.lua integration if present
            let hypr_execs_lua = home.join(".config").join("hypr").join("hyprland").join("execs.lua");
            if hypr_execs_lua.exists() {
                if let Ok(mut text) = fs::read_to_string(&hypr_execs_lua) {
                    let mut modified = false;

                    let daemon_cmd_pattern = "scythe-daemon";
                    let daemon_cmd_line = "    hl.exec_cmd(\"scythe-daemon\")\n";
                    if self.autostart_replay {
                        if !text.contains(daemon_cmd_pattern) {
                            if let Some(pos) = text.rfind("end)") {
                                text.insert_str(pos, daemon_cmd_line);
                                modified = true;
                            }
                        }
                    } else if text.contains(daemon_cmd_pattern) {
                        text = text.lines()
                            .filter(|l| !l.contains(daemon_cmd_pattern))
                            .collect::<Vec<_>>()
                            .join("\n") + "\n";
                        modified = true;
                    }

                    let overlay_cmd_pattern = "scythe-ui --menu";
                    let overlay_cmd_line = "    hl.exec_cmd(\"scythe-ui --menu\")\n";
                    if self.autostart_overlay {
                        if !text.contains(overlay_cmd_pattern) {
                            if let Some(pos) = text.rfind("end)") {
                                text.insert_str(pos, overlay_cmd_line);
                                modified = true;
                            }
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
    }

    pub fn notify_daemon_reload() {
        let _ = crate::ipc::send_command(crate::ipc::Command::ReloadConfig);
    }

    /// Formats video filenames with informative local timestamps, e.g. "Replay-18-30-00_05-09-2026.mp4"
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
}
