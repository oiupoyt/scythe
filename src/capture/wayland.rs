use crate::capture::{Frame, FrameSource};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use std::os::unix::io::{RawFd, IntoRawFd};

pub struct WaylandCapture {
    fd: RawFd,
    node_id: u32,
}

impl WaylandCapture {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;
        
        let select_opts = ashpd::desktop::screencast::SelectSourcesOptions::default()
            .set_multiple(true)
            .set_cursor_mode(CursorMode::Hidden)
            .set_sources(SourceType::Monitor | SourceType::Window)
            .set_persist_mode(PersistMode::DoNot);
            
        proxy.select_sources(&session, select_opts).await?;
        
        let response = proxy.start(&session, None, Default::default()).await?.response()?;
        
        let stream = response.streams().first().ok_or("No stream")?;
        let node_id = stream.pipe_wire_node_id();
        
        let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await?;
        
        Ok(Self {
            fd: fd.into_raw_fd(),
            node_id,
        })
    }
}

impl FrameSource for WaylandCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        Err("Wayland/Pipewire frame extraction not fully implemented yet".into())
    }
}
