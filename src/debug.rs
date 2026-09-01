use minifb::{Window, WindowOptions};

pub struct DebugWindow {
    window: Window,
}

impl DebugWindow {
    pub fn new(width: usize, height: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let window = Window::new(
            "vrec debug capture",
            width,
            height,
            WindowOptions::default(),
        )?;
        Ok(Self { window })
    }

    pub fn update(&mut self, buffer: &[u32], width: usize, height: usize) -> Result<(), Box<dyn std::error::Error>> {
        self.window.update_with_buffer(buffer, width, height)?;
        Ok(())
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open() && !self.window.is_key_down(minifb::Key::Escape)
    }
}
