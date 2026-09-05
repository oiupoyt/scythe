use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SaveReplay,
    StartRecording,
    StopRecording,
    ToggleRecording,
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
            let addr: SocketAddr = "127.0.0.1:42069".parse().unwrap();
            match TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
                Ok(mut stream) => {
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(600)));
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
                    if stream.write_all(&len_buf).is_ok() && stream.write_all(&payload).is_ok() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    last_err = Some(Box::new(e) as Box<dyn std::error::Error + Send + Sync>);
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| "Failed to communicate with scythe-daemon".into()))
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
            let addr: SocketAddr = "127.0.0.1:42069".parse().unwrap();
            TcpStream::connect_timeout(&addr, Duration::from_millis(300))
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
