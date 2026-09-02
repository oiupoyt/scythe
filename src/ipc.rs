use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    SaveReplay,
    StartRecording,
    StopRecording,
    ReloadConfig,
    StopDaemon,
    ShowOverlay,
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
