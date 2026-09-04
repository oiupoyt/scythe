use crate::capture::{Frame, FrameSource};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;

pub struct WaylandCapture {
    stream: crate::capture::wayland_stream::PipeWireStream,
}

impl WaylandCapture {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_cursor(true).await
    }

    pub async fn new_with_cursor(show_cursor: bool) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let proxy = Screencast::new().await.map_err(|e| {
            format!(
                "Failed to initialize XDG ScreenCast portal ({:?}).\n\
                 Ensure xdg-desktop-portal and your compositor's backend are installed and running:\n\
                 - Hyprland: xdg-desktop-portal-hyprland\n\
                 - Sway / wlroots: xdg-desktop-portal-wlr\n\
                 - GNOME: xdg-desktop-portal-gnome\n\
                 - KDE Plasma: xdg-desktop-portal-kde",
                e
            )
        })?;

        let session = proxy.create_session(Default::default()).await.map_err(|e| {
            format!("Failed to create ScreenCast portal session: {:?}", e)
        })?;

        let cursor_mode = if show_cursor {
            CursorMode::Embedded
        } else {
            CursorMode::Hidden
        };
        
        let select_opts = ashpd::desktop::screencast::SelectSourcesOptions::default()
            .set_multiple(false)
            .set_cursor_mode(cursor_mode)
            .set_sources(SourceType::Monitor | SourceType::Monitor)
            .set_persist_mode(PersistMode::DoNot);
            
        proxy.select_sources(&session, select_opts).await.map_err(|e| {
            format!("Failed to select ScreenCast sources: {:?}", e)
        })?;
        
        let response = proxy.start(&session, None, Default::default()).await
            .map_err(|e| format!("Failed to start ScreenCast session: {:?}", e))?
            .response()
            .map_err(|e| format!("Failed to get ScreenCast response: {:?}", e))?;
        
        let stream = response.streams().first().ok_or("No ScreenCast streams returned by portal")?;
        let node_id = stream.pipe_wire_node_id();
        
        let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await
            .map_err(|e| format!("Failed to open PipeWire remote descriptor: {:?}", e))?;
        
        let stream = crate::capture::wayland_stream::PipeWireStream::new(node_id, fd)?;
        
        Ok(Self { stream })
    }
}

impl FrameSource for WaylandCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        self.stream.next_frame()
    }
}
