use crate::capture::{Frame, FrameSource};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;

pub struct WaylandCapture {
    _proxy: Screencast,
    _session: ashpd::desktop::Session<Screencast>,
    stream: crate::capture::wayland_stream::PipeWireStream,
    show_cursor: bool,
    last_frame: Option<Frame>,
    last_reconnect_attempt: std::time::Instant,
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
            .set_sources(ashpd::enumflags2::BitFlags::from_flag(SourceType::Monitor))
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
        
        Ok(Self {
            _proxy: proxy,
            _session: session,
            stream,
            show_cursor,
            last_frame: None,
            last_reconnect_attempt: std::time::Instant::now() - std::time::Duration::from_secs(5),
        })
    }

    fn try_reconnect(&mut self) -> bool {
        if self.last_reconnect_attempt.elapsed() < std::time::Duration::from_millis(1500) {
            return false;
        }
        self.last_reconnect_attempt = std::time::Instant::now();
        eprintln!("Wayland PipeWire stream disconnected. Attempting auto-reconnect...");

        let cursor = self.show_cursor;
        // Run on an independent OS thread so block_on never conflicts with Tokio runtime threads
        let res = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?;
            rt.block_on(WaylandCapture::new_with_cursor(cursor)).ok()
        }).join();

        if let Ok(Some(new_cap)) = res {
            eprintln!("Wayland ScreenCast session reconnected successfully.");
            self._proxy = new_cap._proxy;
            self._session = new_cap._session;
            self.stream = new_cap.stream;
            true
        } else {
            false
        }
    }
}

impl FrameSource for WaylandCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        match self.stream.next_frame() {
            Ok(f) => {
                self.last_frame = Some(f.clone());
                Ok(f)
            }
            Err(e) => {
                if self.try_reconnect() {
                    return self.stream.next_frame();
                }
                if let Some(ref f) = self.last_frame {
                    return Ok(f.clone());
                }
                Err(e)
            }
        }
    }

    fn next_frame_timeout(&mut self, timeout: std::time::Duration) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        match self.stream.next_frame_timeout(timeout) {
            Ok(f) => {
                self.last_frame = Some(f.clone());
                Ok(f)
            }
            Err(e) => {
                if self.try_reconnect() {
                    return self.stream.next_frame_timeout(timeout);
                }
                if let Some(ref f) = self.last_frame {
                    return Ok(f.clone());
                }
                Err(e)
            }
        }
    }
}
