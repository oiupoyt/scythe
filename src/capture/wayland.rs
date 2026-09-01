use crate::capture::{Frame, FrameSource};
use ashpd::desktop::screencast::{CursorMode, PersistMode, Screencast, SourceType};
use ashpd::WindowIdentifier;
use std::os::unix::io::RawFd;

pub struct WaylandCapture {
    fd: RawFd,
    node_id: u32,
}

impl WaylandCapture {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session().await?;
        proxy.select_sources(
            &session,
            CursorMode::Hidden,
            SourceType::Monitor | SourceType::Window,
            true,
            None,
            PersistMode::DoNot,
        ).await?;
        let response = proxy.start(&session, &WindowIdentifier::default()).await?;
        
        // This is as far as we go with ashpd, next we need pipewire
        let stream = response.streams().first().ok_or("No stream")?;
        let node_id = stream.pipe_wire_node_id();
        
        let fd = proxy.open_pipe_wire_remote(&session).await?;
        
        Ok(Self {
            fd: fd.into_raw_fd(),
            node_id,
        })
    }
}

impl FrameSource for WaylandCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error>> {
        Err("Wayland/Pipewire frame extraction not fully implemented yet".into())
    }
}
