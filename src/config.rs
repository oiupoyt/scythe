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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VrecConfig {
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
    #[serde(default = "VrecConfig::default_output_directory")]
    pub output_directory: String,
    #[serde(default = "default_audio_device")]
    pub audio_device: String,
    #[serde(default = "default_audio_mode")]
    pub audio_mode: String,
    
    #[serde(default)]
    pub autostart: bool,
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
}

impl Default for VrecConfig {
    fn default() -> Self {
        Self {
            replay_enabled: true,
            replay_duration_sec: 60,
            replay_bitrate_kbps: 18000,
            
            record_enabled: false,
            record_bitrate_kbps: 20000,
            fps: 60,
            video_codec: "h264".to_string(),
            output_directory: Self::default_output_directory(),
            audio_device: "default".to_string(),
            audio_mode: "system".to_string(),
            
            autostart: false,
            language: "en".to_string(),
            ui_color_theme: "dark".to_string(),
            save_hotkey: "Ctrl+Shift+R".to_string(),
            menu_hotkey: "Alt+Z".to_string(),
            record_hotkey: "Ctrl+Shift+F9".to_string(),
        }
    }
}

impl VrecConfig {
    pub fn default_output_directory() -> String {
        let mut p = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("vrec");
        p.to_string_lossy().to_string()
    }

    pub fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("vrec");
        path.push("config.json");
        path
    }

    pub fn resolve_save_path(&self, filename: &str) -> String {
        let mut p = PathBuf::from(&self.output_directory);
        p.push(filename);
        p.to_string_lossy().to_string()
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&content) {
                return cfg;
            }
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
        fs::write(path, json)?;
        Ok(())
    }

    pub fn notify_daemon_reload() {
        let _ = crate::ipc::send_command(crate::ipc::Command::ReloadConfig);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults_and_json() {
        let cfg = VrecConfig::default();
        assert_eq!(cfg.replay_duration_sec, 60);
        assert_eq!(cfg.replay_bitrate_kbps, 18000);
        assert_eq!(cfg.fps, 60);
        assert_eq!(cfg.video_codec, "h264");
        assert_eq!(cfg.audio_mode, "system");
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: VrecConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.replay_duration_sec, 60);
        assert_eq!(parsed.audio_mode, "system");
    }
}
