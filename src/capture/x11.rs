use xcb::{x, Connection};
use crate::capture::{Frame, FrameSource};

pub struct X11Capture {
    conn: Connection,
    root: x::Window,
    width: u16,
    height: u16,
}

impl X11Capture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (conn, screen_num) = Connection::connect(None)?;
        let setup = conn.get_setup();
        let screen = setup.roots().nth(screen_num as usize).ok_or("No screen found")?;
        
        Ok(Self {
            root: screen.root(),
            width: screen.width_in_pixels(),
            height: screen.height_in_pixels(),
            conn,
        })
    }
}

impl FrameSource for X11Capture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error>> {
        let cookie = self.conn.send_request(&x::GetImage {
            format: x::ImageFormat::ZPixmap,
            drawable: x::Drawable::Window(self.root),
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
            plane_mask: std::u32::MAX,
        });
        
        let reply = self.conn.wait_for_reply(cookie)?;
        let data = reply.data().to_vec();
        
        let stride = (self.width as u32) * 4;
        
        Ok(Frame::Raw {
            width: self.width as u32,
            height: self.height as u32,
            stride,
            data,
        })
    }
}
