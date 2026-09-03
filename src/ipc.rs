use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SaveReplay,
    StartRecording,
    StopRecording,
    ToggleRecording,
    ToggleAudio,
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
}

pub fn send_command(cmd: Command) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::env;
    use std::os::unix::net::UnixStream;
    use std::io::Write;

    let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
    let mut stream = UnixStream::connect(&socket_path)?;
    let payload = serde_json::to_vec(&cmd)?;
    let len_buf = (payload.len() as u32).to_le_bytes();
    stream.write_all(&len_buf)?;
    stream.write_all(&payload)?;
    Ok(())
}

pub fn query_status() -> Result<DaemonStatus, Box<dyn std::error::Error + Send + Sync>> {
    use std::env;
    use std::os::unix::net::UnixStream;
    use std::io::{Read, Write};

    let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
    let mut stream = UnixStream::connect(&socket_path)?;
    let payload = serde_json::to_vec(&Command::GetStatus)?;
    let len_buf = (payload.len() as u32).to_le_bytes();
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
