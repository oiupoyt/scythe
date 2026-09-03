use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SaveReplay,
    StartRecording,
    StopRecording,
    ToggleRecording,
    ToggleAudio,
    CycleAudioMode,
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
}

fn default_audio_mode_str() -> String {
    "system".to_string()
}

pub fn send_command(cmd: Command) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Write;

    let payload = serde_json::to_vec(&cmd)?;
    let len_buf = (payload.len() as u32).to_le_bytes();

    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        let socket_path = format!("{}/vrec.sock", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
        let mut stream = UnixStream::connect(&socket_path)?;
        stream.write_all(&len_buf)?;
        stream.write_all(&payload)?;
    }

    #[cfg(windows)]
    {
        use std::net::TcpStream;
        let mut stream = TcpStream::connect("127.0.0.1:42069")?;
        stream.write_all(&len_buf)?;
        stream.write_all(&payload)?;
    }

    Ok(())
}

pub fn query_status() -> Result<DaemonStatus, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::{Read, Write};

    let payload = serde_json::to_vec(&Command::GetStatus)?;
    let len_buf = (payload.len() as u32).to_le_bytes();

    #[cfg(unix)]
    let mut stream = {
        use std::os::unix::net::UnixStream;
        let socket_path = format!("{}/vrec.sock", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
        UnixStream::connect(&socket_path)?
    };

    #[cfg(windows)]
    let mut stream = {
        use std::net::TcpStream;
        TcpStream::connect("127.0.0.1:42069")?
    };

    stream.write_all(&len_buf)?;
    stream.write_all(&payload)?;

    let mut resp_len_buf = [0u8; 4];
    stream.read_exact(&mut resp_len_buf)?;
    let resp_len = u32::from_le_bytes(resp_len_buf) as usize;
    let mut resp_payload = vec![0u8; resp_len];
    stream.read_exact(&mut resp_payload)?;
    let status: DaemonStatus = serde_json::from_slice(&resp_payload)?;
    Ok(status)
}
