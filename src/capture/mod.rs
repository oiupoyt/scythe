use std::os::unix::io::RawFd;

pub mod wayland;
pub mod x11;

#[derive(Debug)]
pub enum Frame {
    /// Zero-copy DMA-BUF file descriptor. Used by Wayland/PipeWire.
    DmaBuf {
        width: u32,
        height: u32,
        format: u32,
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
}

pub trait FrameSource {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error>>;
}
