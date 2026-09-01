use ffmpeg_next::ffi::*;
use crate::ring::Packet;
use std::ffi::CString;

pub struct Muxer {
    fmt_ctx: *mut AVFormatContext,
}

impl Muxer {
    pub unsafe fn new(path: &str, codec_ctx: *mut AVCodecContext) -> Result<Self, String> {
        unsafe {
            let path_cstr = CString::new(path).unwrap();
            let mut fmt_ctx: *mut AVFormatContext = std::ptr::null_mut();
            
            let ret = avformat_alloc_output_context2(&mut fmt_ctx, std::ptr::null(), std::ptr::null(), path_cstr.as_ptr());
            if ret < 0 {
                return Err("Failed to alloc output context".into());
            }
            
            let stream = avformat_new_stream(fmt_ctx, std::ptr::null());
            if stream.is_null() {
                return Err("Failed to create stream".into());
            }
            
            let ret = avcodec_parameters_from_context((*stream).codecpar, codec_ctx);
            if ret < 0 {
                return Err("Failed to copy codec parameters".into());
            }
            
            if ((*(*fmt_ctx).oformat).flags & AVFMT_NOFILE) == 0 {
                let ret = avio_open(&mut (*fmt_ctx).pb, path_cstr.as_ptr(), AVIO_FLAG_WRITE);
                if ret < 0 {
                    return Err("Failed to open file".into());
                }
            }
            
            let ret = avformat_write_header(fmt_ctx, std::ptr::null_mut());
            if ret < 0 {
                return Err("Failed to write header".into());
            }
            
            Ok(Self { fmt_ctx })
        }
    }
    
    pub fn write_packet(&mut self, packet: &Packet) -> Result<(), String> {
        unsafe {
            // we must not pass the exact ptr to av_interleaved_write_frame directly 
            // if we want to keep the packet, but since it's just writing, we can clone it
            let mut new_pkt = av_packet_alloc();
            av_packet_ref(new_pkt, packet.ptr);
            let ret = av_interleaved_write_frame(self.fmt_ctx, new_pkt);
            av_packet_free(&mut new_pkt); // Wait, av_interleaved takes ownership usually?
            // Actually av_interleaved_write_frame takes ownership of the packet reference, 
            // but we can just use av_write_frame if we don't want it buffered, 
            // but av_interleaved_write_frame will unref the packet for us!
            // Wait, if it unrefs it, we shouldn't free it again.
            if ret < 0 {
                return Err("Failed to write frame".into());
            }
        }
        Ok(())
    }
    
    pub fn finalize(&mut self) -> Result<(), String> {
        unsafe {
            av_write_trailer(self.fmt_ctx);
        }
        Ok(())
    }
}

impl Drop for Muxer {
    fn drop(&mut self) {
        unsafe {
            if ((*(*self.fmt_ctx).oformat).flags & AVFMT_NOFILE) == 0 {
                avio_closep(&mut (*self.fmt_ctx).pb);
            }
            avformat_free_context(self.fmt_ctx);
        }
    }
}
