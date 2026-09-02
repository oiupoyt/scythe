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
