#[cfg(unix)]
pub type NativeFd = std::os::unix::io::RawFd;
#[cfg(not(unix))]
pub type NativeFd = i32;

#[cfg(target_os = "linux")]
pub mod wayland;
#[cfg(target_os = "linux")]
pub mod wayland_stream;
#[cfg(target_os = "linux")]
pub mod x11;
#[cfg(target_os = "windows")]
pub mod windows;

#[derive(Debug)]
pub enum Frame {
    /// Zero-copy DMA-BUF file descriptor. Used by Wayland/PipeWire on Linux.
    DmaBuf {
        width: u32,
        height: u32,
        format: u32,
        modifier: u64,
        fd: NativeFd,
        stride: u32,
        offset: u32,
    },
    /// Raw memory buffer. Used by X11, Windows fallback, and mock capture.
    Raw {
        width: u32,
        height: u32,
        stride: u32,
        data: Vec<u8>,
    },
    /// Zero-copy DirectX 11 Texture handle. Used by Windows DXGI capture.
    #[cfg(target_os = "windows")]
    D3D11Texture {
        handle: usize,
    },
}

pub trait FrameSource: Send {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>>;
}

pub mod mock;
pub mod audio;
