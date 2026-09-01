use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    SaveReplay,
    StopDaemon,
    ShowOverlay,
}
