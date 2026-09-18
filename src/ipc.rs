use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SaveReplay,
    StartRecording,
    StopRecording,
    ToggleRecording,
    StartStreaming,
    StopStreaming,
    ToggleStreaming,
    ToggleAudio,
    CycleAudioMode,
    ToggleCursor,
    ReloadConfig,
    StopDaemon,
    ShowOverlay,
    GetStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DaemonStatus {
    pub is_recording: bool,
    pub recording_duration_sec: u64,
    pub is_replay_active: bool,
    pub audio_muted: bool,
    #[serde(default = "default_audio_mode_str")]
    pub audio_mode: String,
    #[serde(default = "default_true")]
    pub show_cursor: bool,
    #[serde(default = "default_mic_volume")]
    pub mic_volume: f32,
    #[serde(default = "default_system_volume")]
    pub system_volume: f32,
    #[serde(default)]
    pub mic_level_peak: f32,
    #[serde(default)]
    pub system_level_peak: f32,
    #[serde(default)]
    pub is_streaming: bool,
    #[serde(default)]
    pub streaming_duration_sec: u64,
}

fn default_true() -> bool {
    true
}

fn default_mic_volume() -> f32 {
    0.60
}

fn default_system_volume() -> f32 {
    1.00
}

fn default_audio_mode_str() -> String {
    "system".to_string()
}

#[cfg(unix)]
pub fn get_socket_path() -> String {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    format!("{}/scythe.sock", runtime_dir)
}

#[cfg(unix)]
pub fn get_legacy_socket_path() -> String {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    format!("{}/vrec.sock", runtime_dir)
}

#[cfg(windows)]
pub fn get_ipc_port_path() -> std::path::PathBuf {
    std::env::temp_dir().join("scythe-ipc.port")
}

#[cfg(windows)]
pub fn read_active_ipc_port() -> u16 {
    if let Ok(content) = std::fs::read_to_string(get_ipc_port_path()) {
        if let Ok(port) = content.trim().parse::<u16>() {
            return port;
        }
    }
    let legacy = std::env::temp_dir().join("vrec-ipc.port");
    if let Ok(content) = std::fs::read_to_string(legacy) {
        if let Ok(port) = content.trim().parse::<u16>() {
            return port;
        }
    }
    42069
}

pub fn is_daemon_running() -> bool {
    query_status().is_ok()
}

pub fn send_command(cmd: Command) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Write;
    use std::time::Duration;

    let payload = serde_json::to_vec(&cmd)?;
    let len_buf = (payload.len() as u32).to_le_bytes();

    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(60));
        }

        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;
            let socket_path = get_socket_path();
            let connect_res = UnixStream::connect(&socket_path)
                .or_else(|_| UnixStream::connect(get_legacy_socket_path()));
            match connect_res {
                Ok(mut stream) => {
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
                    if stream.write_all(&len_buf).is_ok() && stream.write_all(&payload).is_ok() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    last_err = Some(Box::new(e) as Box<dyn std::error::Error + Send + Sync>);
                }
            }
        }

        #[cfg(windows)]
        {
            use std::net::{SocketAddr, TcpStream};
            let active_port = read_active_ipc_port();
            let candidate_ports = [active_port, 42069, 42070, 42071, 42072];
            for &port in &candidate_ports {
                if let Ok(addr) = format!("127.0.0.1:{}", port).parse::<SocketAddr>() {
                    if let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) {
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(600)));
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
                        if stream.write_all(&len_buf).is_ok() && stream.write_all(&payload).is_ok() {
                            return Ok(());
                        }
                    }
                }
            }
            last_err = Some("Could not connect to daemon on any TCP IPC port".into());
        }
    }

    Err(last_err.unwrap_or_else(|| "Failed to communicate with scythe-daemon".into()))
}

#[cfg(windows)]
pub fn query_status_port(port: u16) -> Result<DaemonStatus, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::{Read, Write};
    use std::time::Duration;
    use std::net::{SocketAddr, TcpStream};

    let payload = serde_json::to_vec(&Command::GetStatus)?;
    let len_buf = (payload.len() as u32).to_le_bytes();

    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200))?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));

    stream.write_all(&len_buf)?;
    stream.write_all(&payload)?;

    let mut resp_len_buf = [0u8; 4];
    stream.read_exact(&mut resp_len_buf)?;
    let resp_len = u32::from_le_bytes(resp_len_buf) as usize;
    let mut resp_payload = vec![0u8; resp_len];
    stream.read_exact(&mut resp_payload)?;
    let status = serde_json::from_slice::<DaemonStatus>(&resp_payload)?;
    Ok(status)
}

pub fn query_status() -> Result<DaemonStatus, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::{Read, Write};
    use std::time::Duration;

    let payload = serde_json::to_vec(&Command::GetStatus)?;
    let len_buf = (payload.len() as u32).to_le_bytes();

    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(60));
        }

        #[cfg(unix)]
        let stream_res = {
            use std::os::unix::net::UnixStream;
            let socket_path = get_socket_path();
            UnixStream::connect(&socket_path)
                .or_else(|_| UnixStream::connect(get_legacy_socket_path()))
        };

        #[cfg(windows)]
        let stream_res = {
            use std::net::{SocketAddr, TcpStream};
            let active_port = read_active_ipc_port();
            let candidate_ports = [active_port, 42069, 42070, 42071, 42072];
            let mut conn = None;
            for &port in &candidate_ports {
                if let Ok(addr) = format!("127.0.0.1:{}", port).parse::<SocketAddr>() {
                    if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(150)) {
                        conn = Some(stream);
                        break;
                    }
                }
            }
            conn.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "No active daemon port reachable"))
        };

        match stream_res {
            Ok(mut stream) => {
                let _ = stream.set_write_timeout(Some(Duration::from_millis(600)));
                let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));

                if stream.write_all(&len_buf).is_err() || stream.write_all(&payload).is_err() {
                    continue;
                }

                let mut resp_len_buf = [0u8; 4];
                if stream.read_exact(&mut resp_len_buf).is_err() {
                    continue;
                }
                let resp_len = u32::from_le_bytes(resp_len_buf) as usize;
                let mut resp_payload = vec![0u8; resp_len];
                if stream.read_exact(&mut resp_payload).is_err() {
                    continue;
                }
                if let Ok(status) = serde_json::from_slice::<DaemonStatus>(&resp_payload) {
                    return Ok(status);
                }
            }
            Err(e) => {
                last_err = Some(Box::new(e) as Box<dyn std::error::Error + Send + Sync>);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| "Failed to query status from scythe-daemon".into()))
}

pub fn get_overlay_pid_path() -> std::path::PathBuf {
    #[cfg(unix)]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        std::path::PathBuf::from(runtime_dir).join("scythe-overlay.pid")
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("scythe-overlay.pid")
    }
}

pub fn clean_overlay_pid() {
    let _ = std::fs::remove_file(get_overlay_pid_path());
}

pub fn get_toast_pid_path() -> std::path::PathBuf {
    #[cfg(unix)]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        std::path::PathBuf::from(runtime_dir).join("scythe-toast.pid")
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("scythe-toast.pid")
    }
}

pub fn clean_toast_pid() {
    let _ = std::fs::remove_file(get_toast_pid_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_command_and_status_serde() {
        let cmd = Command::ToggleStreaming;
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Command::ToggleStreaming);

        let status = DaemonStatus {
            is_recording: true,
            recording_duration_sec: 42,
            is_replay_active: true,
            audio_muted: false,
            audio_mode: "system".to_string(),
            show_cursor: true,
            mic_volume: 0.6,
            system_volume: 1.0,
            mic_level_peak: 0.2,
            system_level_peak: 0.8,
            is_streaming: true,
            streaming_duration_sec: 120,
        };
        let status_json = serde_json::to_string(&status).unwrap();
        let parsed_status: DaemonStatus = serde_json::from_str(&status_json).unwrap();
        assert!(parsed_status.is_streaming);
        assert_eq!(parsed_status.streaming_duration_sec, 120);
    }
}
