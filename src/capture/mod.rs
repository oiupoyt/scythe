use std::os::unix::io::RawFd;

pub mod wayland;
pub mod wayland_stream;
pub mod x11;
#[cfg(target_os = "windows")]
pub mod windows;

#[derive(Debug)]
pub enum Frame {
    /// Zero-copy DMA-BUF file descriptor. Used by Wayland/PipeWire.
    DmaBuf {
        width: u32,
        height: u32,
        format: u32, modifier: u64,
        fd: RawFd,
        stride: u32,
        offset: u32,
    },
    /// Raw memory buffer. Used by X11 fallback.
    Raw {
        width: u32,
        height: u32,
        stride: u32,
        data: Vec<u8>,
    },
    /// Zero-copy DirectX 11 Texture handle. Used by Windows DXGI capture.
    #[cfg(target_os = "windows")]
    D3D11Texture {
        handle: usize, // Placeholder for ID3D11Texture2D raw pointer
    },
}

pub trait FrameSource: Send {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>>;
}
