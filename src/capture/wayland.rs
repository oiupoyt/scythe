use crate::capture::{Frame, FrameSource};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use std::os::unix::io::{RawFd, IntoRawFd};

pub struct WaylandCapture {
    stream: crate::capture::wayland_stream::PipeWireStream,
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
        
        let stream = crate::capture::wayland_stream::PipeWireStream::new(node_id, fd)?;
        
        Ok(Self { stream })
    }
}

impl FrameSource for WaylandCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        self.stream.next_frame()
    }
}
