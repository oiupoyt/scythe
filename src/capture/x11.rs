use xcb::{x, shm, Connection};
use crate::capture::{Frame, FrameSource};

/// Permissions for the shared-memory segment holding captured frames.
const SHM_SEGMENT_MODE: libc::c_int = 0o600;

/// Whether the segment is marked for removal as soon as both sides attach.
const MARK_SEGMENT_FOR_REMOVAL_AFTER_ATTACH: bool = true;

pub struct X11Capture {
    conn: Connection,
    root: x::Window,
    width: u16,
    height: u16,
    shm_seg: Option<shm::Seg>,
    shm_addr: *mut u8,
    shm_size: usize,
    shmid: i32,
    last_frame_instant: std::time::Instant,
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
            let id = libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | SHM_SEGMENT_MODE);
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
                        if MARK_SEGMENT_FOR_REMOVAL_AFTER_ATTACH {
                            // Both this process and the X server are attached,
                            // so the segment lives on until both detach. Marking
                            // it now means the kernel frees it when the daemon
                            // exits, even through process::exit or a crash,
                            // where Drop never runs.
                            libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut());
                        }
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
            last_frame_instant: std::time::Instant::now(),
        })
    }
}

impl FrameSource for X11Capture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        let target_frame_time = std::time::Duration::from_millis(16);
        let elapsed = self.last_frame_instant.elapsed();
        if elapsed < target_frame_time {
            std::thread::sleep(target_frame_time - elapsed);
        }
        self.last_frame_instant = std::time::Instant::now();

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
                data: std::sync::Arc::new(data),
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
            data: std::sync::Arc::new(data),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The segment holds the contents of the screen. Mode 0777 let any user
    /// on the machine read the screen, and write into frames being encoded.
    #[test]
    fn test_shm_segment_is_private_to_the_user() {
        assert_eq!(SHM_SEGMENT_MODE & 0o077, 0, "shm mode {:o} is open to other users", SHM_SEGMENT_MODE);
    }

    /// Drop never runs for the capture source: the daemon exits through
    /// process::exit and release builds abort on panic. Unless the segment
    /// is marked for removal right after attach, every daemon run leaks one
    /// screen-sized segment until reboot (28 x 8.3 MB found on one machine).
    #[test]
    fn test_shm_segment_is_marked_for_removal_after_attach() {
        let src = include_str!("x11.rs");
        assert!(MARK_SEGMENT_FOR_REMOVAL_AFTER_ATTACH, "segment is only removed in Drop, which never runs");
        let attach_ok = src.find("if conn.check_request(attach_cookie).is_ok() {").expect("attach check");
        let after = &src[attach_ok..attach_ok + 900];
        assert!(after.contains("IPC_RMID"), "no IPC_RMID on the successful attach path");
    }

    /// Creating a private segment, attaching it, and marking it for removal
    /// keeps it usable while attached and gone once detached.
    #[test]
    fn test_marked_segment_disappears_after_detach() {
        unsafe {
            let id = libc::shmget(libc::IPC_PRIVATE, 4096, libc::IPC_CREAT | SHM_SEGMENT_MODE);
            assert!(id >= 0, "shmget failed");
            let addr = libc::shmat(id, std::ptr::null(), 0) as *mut u8;
            assert_ne!(addr, (-1isize) as *mut u8, "shmat failed");
            assert_eq!(libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()), 0);
            *addr = 42;
            assert_eq!(*addr, 42, "segment unusable after IPC_RMID while attached");
            libc::shmdt(addr as *const _);
            let mut ds: libc::shmid_ds = std::mem::zeroed();
            assert_ne!(libc::shmctl(id, libc::IPC_STAT, &mut ds), 0, "segment still exists after the last detach");
        }
    }
}
