use xcb::{x, shm, Connection};
use crate::capture::{Frame, FrameSource};

pub struct X11Capture {
    conn: Connection,
    root: x::Window,
    width: u16,
    height: u16,
    shm_seg: Option<shm::Seg>,
    shm_addr: *mut u8,
    shm_size: usize,
    shmid: i32,
}

unsafe impl Send for X11Capture {}

impl X11Capture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (conn, screen_num) = Connection::connect(None)?;
        let (root, width, height) = {
            let setup = conn.get_setup();
            let screen = setup.roots().nth(screen_num as usize).ok_or("No screen found")?;
            (screen.root(), screen.width_in_pixels(), screen.height_in_pixels())
        };
        
        let size = (width as usize) * (height as usize) * 4;

        // Try initializing MIT-SHM for zero-socket high performance
        let (shm_seg, shm_addr, shmid) = unsafe {
            let id = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o777);
            if id >= 0 {
                let addr = libc::shmat(id, std::ptr::null(), 0) as *mut u8;
                if addr != (-1isize as *mut u8) {
                    let seg: shm::Seg = conn.generate_id();
                    let attach_cookie = conn.send_request_checked(&shm::Attach {
                        shmseg: seg,
                        shmid: id as u32,
                        read_only: false,
                    });
                    if conn.check_request(attach_cookie).is_ok() {
                        println!("X11 MIT-SHM shared memory capture initialized (zero socket overhead): {}x{}", width, height);
                        (Some(seg), addr, id)
                    } else {
                        libc::shmdt(addr as *const _);
                        libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                        (None, std::ptr::null_mut(), -1)
                    }
                } else {
                    libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                    (None, std::ptr::null_mut(), -1)
                }
            } else {
                (None, std::ptr::null_mut(), -1)
            }
        };

        Ok(Self {
            conn,
            root,
            width,
            height,
            shm_seg,
            shm_addr,
            shm_size: size,
            shmid,
        })
    }
}

impl FrameSource for X11Capture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        let stride = (self.width as u32) * 4;

        if let Some(seg) = self.shm_seg && !self.shm_addr.is_null() {
            let cookie = self.conn.send_request(&shm::GetImage {
                drawable: x::Drawable::Window(self.root),
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
                plane_mask: u32::MAX,
                format: x::ImageFormat::ZPixmap as u8,
                shmseg: seg,
                offset: 0,
            });

            self.conn.wait_for_reply(cookie)?;

            let data = unsafe {
                std::slice::from_raw_parts(self.shm_addr, self.shm_size).to_vec()
            };

            return Ok(Frame::Raw {
                width: self.width as u32,
                height: self.height as u32,
                stride,
                data,
            });
        }

        // Fallback to standard GetImage if SHM is not available
        let cookie = self.conn.send_request(&x::GetImage {
            format: x::ImageFormat::ZPixmap,
            drawable: x::Drawable::Window(self.root),
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
            plane_mask: u32::MAX,
        });
        
        let reply = self.conn.wait_for_reply(cookie)?;
        let data = reply.data().to_vec();
        
        Ok(Frame::Raw {
            width: self.width as u32,
            height: self.height as u32,
            stride,
            data,
        })
    }
}

impl Drop for X11Capture {
    fn drop(&mut self) {
        if let Some(seg) = self.shm_seg {
            let _ = self.conn.send_request(&shm::Detach { shmseg: seg });
        }
        if !self.shm_addr.is_null() && self.shm_addr != (-1isize as *mut u8) {
            unsafe {
                libc::shmdt(self.shm_addr as *const _);
            }
        }
        if self.shmid >= 0 {
            unsafe {
                libc::shmctl(self.shmid, libc::IPC_RMID, std::ptr::null_mut());
            }
        }
    }
}
