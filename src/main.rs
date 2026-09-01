pub mod capture;
pub mod debug;

use capture::{Frame, FrameSource};
use debug::DebugWindow;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    println!("Detected session type: {}", session_type);

    let mut source: Box<dyn FrameSource> = if session_type.to_lowercase() == "wayland" {
        println!("Initializing Wayland capture...");
        Box::new(capture::wayland::WaylandCapture::new().await?)
    } else {
        println!("Initializing X11 capture...");
        Box::new(capture::x11::X11Capture::new()?)
    };

    let mut window: Option<DebugWindow> = None;

    println!("Starting capture loop...");
    loop {
        match source.next_frame() {
            Ok(Frame::Raw { width, height, data, stride }) => {
                if window.is_none() {
                    window = Some(DebugWindow::new(width as usize, height as usize)?);
                }

                if let Some(win) = &mut window {
                    if !win.is_open() {
                        break;
                    }
                    // For minifb, we need to cast &[u8] to &[u32].
                    // Assuming BGRA or similar 4-byte format.
                    let u32_data: Vec<u32> = data
                        .chunks_exact(4)
                        .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        .collect();
                    
                    win.update(&u32_data, width as usize, height as usize)?;
                }
            }
            Ok(Frame::DmaBuf { .. }) => {
                println!("Wayland/PipeWire DMA-BUF received. Phase 1 debug viewer not fully implemented for DMA-BUFs yet.");
                break;
            }
            Err(e) => {
                eprintln!("Capture error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
