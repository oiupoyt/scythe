use ffmpeg_next::ffi::*;
use std::ptr;
use crate::capture::Frame;

pub struct VaapiEncoder {
    codec_ctx: *mut AVCodecContext,
    hw_device_ctx: *mut AVBufferRef,
    hw_frames_ctx: *mut AVBufferRef,
}

impl VaapiEncoder {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            // Find VAAPI encoder (h264_vaapi)
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

            // Init hardware device
            let mut hw_device_ctx: *mut AVBufferRef = ptr::null_mut();
            let ret = av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                ptr::null(), // default device
                ptr::null_mut(),
                0,
            );
            if ret < 0 {
                return Err("Failed to create VAAPI hardware device".into());
            }

            // Allocate hw frames context
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
            })
        }
    }

    pub fn encode_frame(&mut self, frame: &Frame) -> Result<(), String> {
        unsafe {
            match frame {
                Frame::Raw { width, height, stride, data } => {
                    // For Phase 2 X11 fallback:
                    // 1. Convert BGRA to NV12 (omitted/mocked for brevity if not using swscale yet)
                    // 2. Upload NV12 to a VAAPI AVFrame via av_hwframe_get_buffer and av_hwframe_transfer_data
                    // 3. Send to avcodec_send_frame
                    println!("VAAPI: Mock encoding Raw CPU frame (X11 fallback). Needs BGRA->NV12 swscale.");
                }
                Frame::DmaBuf { width, height, format, fd, stride, offset } => {
                    // For Phase 2 Wayland zero-copy:
                    // 1. Populate AVDRMFrameDescriptor with the FD and modifier.
                    // 2. Create an AVFrame with format AV_PIX_FMT_DRM_PRIME.
                    // 3. Use av_hwframe_map to map it to AV_PIX_FMT_VAAPI.
                    // 4. Send to avcodec_send_frame.
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
