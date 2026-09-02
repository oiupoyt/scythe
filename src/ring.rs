use ffmpeg_next::ffi::*;

pub struct Packet {
    pub ptr: *mut AVPacket,
}

unsafe impl Send for Packet {}
unsafe impl Sync for Packet {}

impl Packet {
    pub fn new(ptr: *mut AVPacket) -> Self {
        Self { ptr }
    }
    
    pub fn stream_index(&self) -> i32 {
        unsafe { (*self.ptr).stream_index }
    }
    
    pub fn set_stream_index(&mut self, idx: i32) {
        unsafe { (*self.ptr).stream_index = idx; }
    }
    
    pub fn is_keyframe(&self) -> bool {
        unsafe {
            ((*self.ptr).flags & AV_PKT_FLAG_KEY) != 0
        }
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                let mut p = self.ptr;
                av_packet_free(&mut p);
                self.ptr = std::ptr::null_mut();
            }
        }
    }
}

impl Clone for Packet {
    fn clone(&self) -> Self {
        unsafe {
            let new_pkt = av_packet_alloc();
            av_packet_ref(new_pkt, self.ptr);
            Self { ptr: new_pkt }
        }
    }
}

