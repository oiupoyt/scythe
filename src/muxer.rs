use ffmpeg_next::ffi::*;
use crate::ring::Packet;
use std::ffi::CString;

pub struct Muxer {
    fmt_ctx: *mut AVFormatContext,
    video_time_base: AVRational,
    audio_time_base: Option<AVRational>,
}

impl Muxer {
    /// # Safety
    /// `codec_ctx` must be a valid, initialized pointer to an `AVCodecContext`.
    /// `audio_codec_ctx`, if provided, must also be a valid, initialized pointer.
    pub unsafe fn new(
        path: &str,
        codec_ctx: *mut AVCodecContext,
        audio_codec_ctx: Option<*mut AVCodecContext>,
    ) -> Result<Self, String> {
        unsafe {
            let path_cstr = CString::new(path).map_err(|e| e.to_string())?;
            let mut fmt_ctx: *mut AVFormatContext = std::ptr::null_mut();

            let ret = avformat_alloc_output_context2(
                &mut fmt_ctx,
                std::ptr::null(),
                std::ptr::null(),
                path_cstr.as_ptr(),
            );
            if ret < 0 || fmt_ctx.is_null() {
                return Err("Failed to alloc output context".into());
            }

            let stream = avformat_new_stream(fmt_ctx, std::ptr::null());
            if stream.is_null() {
                avformat_free_context(fmt_ctx);
                return Err("Failed to create video stream".into());
            }

            (*stream).id = 0;
            let ret = avcodec_parameters_from_context((*stream).codecpar, codec_ctx);
            if ret < 0 {
                avformat_free_context(fmt_ctx);
                return Err("Failed to copy video codec parameters".into());
            }
            let video_time_base = (*codec_ctx).time_base;
            (*stream).time_base = video_time_base;

            let mut audio_time_base = None;
            if let Some(actx) = audio_codec_ctx {
                let astream = avformat_new_stream(fmt_ctx, std::ptr::null());
                if !astream.is_null() {
                    (*astream).id = 1;
                    if avcodec_parameters_from_context((*astream).codecpar, actx) < 0 {
                        eprintln!("Warning: Failed to copy audio codec parameters");
                    }
                    let atb = (*actx).time_base;
                    (*astream).time_base = atb;
                    audio_time_base = Some(atb);
                }
            }

            if ((*(*fmt_ctx).oformat).flags & AVFMT_NOFILE) == 0 {
                let ret = avio_open(&mut (*fmt_ctx).pb, path_cstr.as_ptr(), AVIO_FLAG_WRITE);
                if ret < 0 {
                    avformat_free_context(fmt_ctx);
                    return Err("Failed to open output file".into());
                }
            }

            let ret = avformat_write_header(fmt_ctx, std::ptr::null_mut());
            if ret < 0 {
                if ((*(*fmt_ctx).oformat).flags & AVFMT_NOFILE) == 0 {
                    avio_closep(&mut (*fmt_ctx).pb);
                }
                avformat_free_context(fmt_ctx);
                return Err("Failed to write header".into());
            }

            Ok(Self {
                fmt_ctx,
                video_time_base,
                audio_time_base,
            })
        }
    }

    pub fn write_packet(&mut self, packet: &Packet) -> Result<(), String> {
        unsafe {
            if packet.ptr.is_null() {
                return Ok(());
            }

            let mut new_pkt = av_packet_alloc();
            if new_pkt.is_null() {
                return Err("Failed to allocate packet".into());
            }
            av_packet_ref(new_pkt, packet.ptr);

            let stream_idx = (*new_pkt).stream_index;
            if stream_idx >= 0 && (stream_idx as u32) < (*self.fmt_ctx).nb_streams {
                let out_stream = *(*self.fmt_ctx).streams.add(stream_idx as usize);
                let in_tb = if stream_idx == 0 {
                    self.video_time_base
                } else {
                    self.audio_time_base.unwrap_or(self.video_time_base)
                };
                av_packet_rescale_ts(new_pkt, in_tb, (*out_stream).time_base);
            }

            let ret = av_interleaved_write_frame(self.fmt_ctx, new_pkt);
            av_packet_free(&mut new_pkt);

            if ret < 0 {
                return Err(format!("Failed to write frame: {}", ret));
            }
        }
        Ok(())
    }

    pub fn finalize(&mut self) -> Result<(), String> {
        unsafe {
            let ret = av_write_trailer(self.fmt_ctx);
            if ret < 0 {
                return Err(format!("Failed to write trailer: {}", ret));
            }
        }
        Ok(())
    }
}

impl Drop for Muxer {
    fn drop(&mut self) {
        unsafe {
            if !self.fmt_ctx.is_null() {
                if !(*self.fmt_ctx).pb.is_null() && ((*(*self.fmt_ctx).oformat).flags & AVFMT_NOFILE) == 0 {
                    avio_closep(&mut (*self.fmt_ctx).pb);
                }
                avformat_free_context(self.fmt_ctx);
                self.fmt_ctx = std::ptr::null_mut();
            }
        }
    }
}
