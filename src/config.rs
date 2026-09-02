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
    
    pub autostart: bool,
    pub language: String,
    pub ui_color_theme: String,
    pub save_hotkey: String,
    pub menu_hotkey: String,
}

impl Default for VrecConfig {
    fn default() -> Self {
        Self {
            replay_enabled: true,
            replay_duration_sec: 60,
            replay_bitrate_kbps: 10000,
            
            record_enabled: false,
            record_bitrate_kbps: 15000,
            
            autostart: false,
            language: "en".to_string(),
            ui_color_theme: "dark".to_string(),
            save_hotkey: "Ctrl+Shift+R".to_string(),
            menu_hotkey: "Alt+Z".to_string(),
        }
    }
}

impl VrecConfig {
    pub fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("vrec");
        path.push("config.json");
        path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(data) = fs::read_to_string(path) {
                if let Ok(config) = serde_json::from_str(&data) {
                    return config;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    pub fn notify_daemon_reload() {
        use std::env;
        use std::os::unix::net::UnixStream;
        use std::io::Write;
        let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
        if let Ok(mut stream) = UnixStream::connect(&socket_path) {
            if let Ok(payload) = serde_json::to_vec(&crate::ipc::Command::ReloadConfig) {
                let len_buf = (payload.len() as u32).to_le_bytes();
                let _ = stream.write_all(&len_buf);
                let _ = stream.write_all(&payload);
            }
        }
    }
}
