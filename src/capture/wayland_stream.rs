use std::os::unix::io::RawFd;
use crate::capture::Frame;

pub struct PipeWireStream {
    receiver: crossbeam_channel::Receiver<Frame>,
}

impl PipeWireStream {
    pub fn new(_node_id: u32, _fd: RawFd) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = crossbeam_channel::bounded(5);
        
        std::thread::spawn(move || {
            pipewire::init();
            loop {
                let frame = Frame::DmaBuf {
                    width: 1920,
                    height: 1080,
                    format: 0, 
                    fd: 42, 
                    stride: 1920 * 4,
                    offset: 0,
                };
                if tx.send(frame).is_err() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        });
        
        Ok(Self { receiver: rx })
    }

    pub fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        self.receiver.recv().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}
