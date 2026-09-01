use minifb::{Window, WindowOptions};
use std::time::Duration;

pub fn show_saved_overlay() {
    let width = 300;
    let height = 100;
    
    let mut window = match Window::new(
        "VRec Overlay",
        width,
        height,
        WindowOptions {
            borderless: true,
            title: false,
            resize: false,
            scale: minifb::Scale::X1,
            scale_mode: minifb::ScaleMode::Stretch,
            topmost: true,
            transparency: true,
            none: false,
        }
    ) {
        Ok(win) => win,
        Err(e) => {
            eprintln!("Failed to create overlay window: {}", e);
            return;
        }
    };
    
    // Create a semi-transparent green box or just dark gray
    let buffer: Vec<u32> = vec![0xFF222222; width * height];
    
    window.set_target_fps(60);
    
    let start_time = std::time::Instant::now();
    let display_duration = Duration::from_secs(2);
    
    while window.is_open() && start_time.elapsed() < display_duration {
        // We could render text here, but for now we just show a flash/banner.
        if let Err(e) = window.update_with_buffer(&buffer, width, height) {
            eprintln!("Overlay update error: {}", e);
            break;
        }
    }
}
