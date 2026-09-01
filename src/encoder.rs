use ffmpeg_next::ffi::*;
use std::ptr;
use crate::capture::Frame;

pub struct VaapiEncoder {
    codec_ctx: *mut AVCodecContext,
    hw_device_ctx: *mut AVBufferRef,
    hw_frames_ctx: *mut AVBufferRef,
    next_pts: i64,
}

impl VaapiEncoder {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            let codec = avcodec_find_encoder_by_name(b"h264_vaapi\0".as_ptr() as *const i8);
            if codec.is_null() {
                return Err("h264_vaapi encoder not found".into());
            }

            let codec_ctx = avcodec_alloc_context3(codec);
            if codec_ctx.is_null() {
                return Err("Failed to allocate codec context".into());
            }

            (*codec_ctx).width = width as i32;
            (*codec_ctx).height = height as i32;
            (*codec_ctx).time_base = AVRational { num: 1, den: 60 };
            (*codec_ctx).framerate = AVRational { num: 60, den: 1 };
            (*codec_ctx).pix_fmt = AVPixelFormat::AV_PIX_FMT_VAAPI;

            let mut hw_device_ctx: *mut AVBufferRef = ptr::null_mut();
            let ret = av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                ptr::null(),
                ptr::null_mut(),
                0,
            );
            if ret < 0 {
                return Err("Failed to create VAAPI hardware device".into());
            }

            let hw_frames_ref = av_hwframe_ctx_alloc(hw_device_ctx);
            if hw_frames_ref.is_null() {
                return Err("Failed to allocate hw frames context".into());
            }
            let frames_ctx = (*hw_frames_ref).data as *mut AVHWFramesContext;
            (*frames_ctx).format = AVPixelFormat::AV_PIX_FMT_VAAPI;
            (*frames_ctx).sw_format = AVPixelFormat::AV_PIX_FMT_NV12; 
            (*frames_ctx).width = width as i32;
            (*frames_ctx).height = height as i32;
            (*frames_ctx).initial_pool_size = 20;

            let ret = av_hwframe_ctx_init(hw_frames_ref);
            if ret < 0 {
                return Err("Failed to init hw frames context".into());
            }

            (*codec_ctx).hw_frames_ctx = av_buffer_ref(hw_frames_ref);

            let ret = avcodec_open2(codec_ctx, codec, ptr::null_mut());
            if ret < 0 {
                return Err("Failed to open h264_vaapi encoder".into());
            }

            Ok(Self {
                codec_ctx,
                hw_device_ctx,
                hw_frames_ctx: hw_frames_ref,
                next_pts: 0,
            })
        }
    }

    pub fn encode_frame(&mut self, frame: &Frame) -> Result<(), String> {
        unsafe {
            match frame {
                Frame::Raw { width, height, stride: in_stride, data } => {
                    let mut bgra_frame = av_frame_alloc();
                    (*bgra_frame).format = AVPixelFormat::AV_PIX_FMT_BGRA as i32;
                    (*bgra_frame).width = *width as i32;
                    (*bgra_frame).height = *height as i32;
                    av_frame_get_buffer(bgra_frame, 32);

                    let bgra_stride = (*bgra_frame).linesize[0] as usize;
                    for y in 0..(*height as usize) {
                        let src_row = &data[y * (*in_stride as usize) .. y * (*in_stride as usize) + (*width as usize) * 4];
                        let dst_row = std::slice::from_raw_parts_mut(
                            (*bgra_frame).data[0].add(y * bgra_stride),
                            (*width as usize) * 4
                        );
                        dst_row.copy_from_slice(src_row);
                    }

                    let mut nv12_frame = av_frame_alloc();
                    (*nv12_frame).format = AVPixelFormat::AV_PIX_FMT_NV12 as i32;
                    (*nv12_frame).width = *width as i32;
                    (*nv12_frame).height = *height as i32;
                    av_frame_get_buffer(nv12_frame, 32);

                    let sws_ctx = sws_getContext(
                        *width as i32, *height as i32, AVPixelFormat::AV_PIX_FMT_BGRA,
                        *width as i32, *height as i32, AVPixelFormat::AV_PIX_FMT_NV12,
                        2, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()
                    );
                    
                    sws_scale(
                        sws_ctx,
                        (*bgra_frame).data.as_ptr() as *const *const u8,
                        (*bgra_frame).linesize.as_ptr(),
                        0,
                        *height as i32,
                        (*nv12_frame).data.as_ptr(),
                        (*nv12_frame).linesize.as_ptr()
                    );
                    sws_freeContext(sws_ctx);

                    let mut hw_frame = av_frame_alloc();
                    av_hwframe_get_buffer(self.hw_frames_ctx, hw_frame, 0);
                    av_hwframe_transfer_data(hw_frame, nv12_frame, 0);

                    (*hw_frame).pts = self.next_pts;
                    self.next_pts += 1;

                    if avcodec_send_frame(self.codec_ctx, hw_frame) >= 0 {
                        let mut pkt = av_packet_alloc();
                        while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                            println!("VAAPI: encoded packet of size {}", (*pkt).size);
                            av_packet_unref(pkt);
                        }
                        av_packet_free(&mut pkt);
                    }
                    av_frame_free(&mut hw_frame);
                    av_frame_free(&mut nv12_frame);
                    av_frame_free(&mut bgra_frame);
                }
                Frame::DmaBuf { width: _, height: _, format: _, fd, stride: _, offset: _ } => {
                    println!("VAAPI: Mock encoding DMA-BUF frame (Wayland zero-copy). FD: {}", fd);
                }
            }
        }
        Ok(())
    }
}

impl Drop for VaapiEncoder {
    fn drop(&mut self) {
        unsafe {
            avcodec_free_context(&mut self.codec_ctx);
            av_buffer_unref(&mut self.hw_device_ctx);
            av_buffer_unref(&mut self.hw_frames_ctx);
        }
    }
}
