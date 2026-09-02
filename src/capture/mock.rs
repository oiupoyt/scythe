use super::{Frame, FrameSource};
use std::time::Duration;

pub struct MockCapture {
    width: u32,
    height: u32,
    frame_count: u64,
}

impl MockCapture {
    pub fn new() -> Self {
        Self { width: 1920, height: 1080, frame_count: 0 }
    }
}

impl FrameSource for MockCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        std::thread::sleep(Duration::from_millis(16)); // ~60fps
        let mut data = vec![0u8; (self.width * self.height * 4) as usize];
        
        // Fill with a changing color
        let color = (self.frame_count % 255) as u8;
        for chunk in data.chunks_exact_mut(4) {
            chunk[0] = color; // B
            chunk[1] = color; // G
            chunk[2] = 255;   // R
            chunk[3] = 255;   // A
        }
        
        self.frame_count += 1;

        Ok(Frame::Raw {
            width: self.width,
            height: self.height,
            stride: self.width * 4,
            data,
        })
    }
}
