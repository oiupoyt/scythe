use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VrecConfig {
    pub replay_enabled: bool,
    pub replay_duration_sec: u32,
    pub replay_bitrate_kbps: u32,
    
    pub record_enabled: bool,
    pub record_bitrate_kbps: u32,
    pub fps: u32,
    pub video_codec: String,
    pub output_directory: String,
    pub audio_device: String,
    
    pub autostart: bool,
    pub language: String,
    pub ui_color_theme: String,
    pub save_hotkey: String,
    pub menu_hotkey: String,
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

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists()
            && let Ok(data) = fs::read_to_string(&path)
            && let Ok(config) = serde_json::from_str(&data) {
                return config;
            }
        Self::default()
    }

    pub fn resolve_save_path(&self, filename: &str) -> String {
        let p = std::path::Path::new(&self.output_directory);
        let _ = fs::create_dir_all(p);
        p.join(filename).to_string_lossy().to_string()
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _ = fs::create_dir_all(&self.output_directory);
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        let _ = self.update_autostart();
        Ok(())
    }

    pub fn update_autostart(&self) -> Result<(), std::io::Error> {
        let mut autostart_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        autostart_dir.push("autostart");
        let desktop_file = autostart_dir.join("vrec.desktop");

        if self.autostart {
            fs::create_dir_all(&autostart_dir)?;
            let content = "[Desktop Entry]\n\
                           Type=Application\n\
                           Name=vrec Screen Recorder\n\
                           Comment=Hardware-accelerated screen recorder and instant replay\n\
                           Exec=sh -c \"vrec-daemon & vrec-ui\"\n\
                           Terminal=false\n\
                           Categories=AudioVideo;Recorder;\n";
            fs::write(desktop_file, content)?;
        } else if desktop_file.exists() {
            let _ = fs::remove_file(desktop_file);
        }
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
        assert!(cfg.replay_enabled);
        assert_eq!(cfg.replay_duration_sec, 60);
        assert_eq!(cfg.fps, 60);
        assert!(cfg.output_directory.contains("vrec"));
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: VrecConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.menu_hotkey, "Alt+Z");
        assert_eq!(parsed.save_hotkey, "Ctrl+Shift+R");
    }
}
