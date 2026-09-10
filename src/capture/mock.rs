use super::{Frame, FrameSource};
use std::time::Duration;

use std::sync::Arc;

pub struct MockCapture {
    width: u32,
    height: u32,
    frame_count: u64,
    static_frame: Arc<Vec<u8>>,
}

impl MockCapture {
    pub fn new() -> Self {
        let width = 1920;
        let height = 1080;
        let mut data = vec![0u8; (width * height * 4) as usize];
        for chunk in data.chunks_exact_mut(4) {
            chunk[0] = 64;  // B
            chunk[1] = 64;  // G
            chunk[2] = 64;  // R
            chunk[3] = 255; // A
        }
        Self {
            width,
            height,
            frame_count: 0,
            static_frame: Arc::new(data),
        }
    }
}

impl Default for MockCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameSource for MockCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        std::thread::sleep(Duration::from_millis(16)); // ~60fps
        self.frame_count += 1;

        Ok(Frame::Raw {
            width: self.width,
            height: self.height,
            stride: self.width * 4,
            data: Arc::clone(&self.static_frame),
        })
    }
}
