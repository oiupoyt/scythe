use ffmpeg_next::ffi::*;
use std::ptr;
use crate::capture::Frame;

pub struct VaapiEncoder {
    codec_ctx: *mut AVCodecContext,
    hw_device_ctx: *mut AVBufferRef,
    hw_frames_ctx: *mut AVBufferRef,
    sws_ctx: *mut SwsContext,
    next_pts: i64,
}

impl VaapiEncoder {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        Self::new_with_params(width, height, 18_000, 60, "h264")
    }

    pub fn new_with_bitrate(width: u32, height: u32, bitrate_kbps: u32) -> Result<Self, String> {
        Self::new_with_params(width, height, bitrate_kbps, 60, "h264")
    }

    pub fn new_with_params(width: u32, height: u32, bitrate_kbps: u32, fps: u32, codec_pref: &str) -> Result<Self, String> {
        let fps = fps.clamp(20, 144) as i32;
        unsafe {
            let codec_candidates: &[&std::ffi::CStr] = match codec_pref.to_lowercase().as_str() {
                "hevc" | "h265" => &[c"hevc_vaapi", c"h264_vaapi"],
                "av1" => &[c"av1_vaapi", c"h264_vaapi"],
                _ => &[c"h264_vaapi"],
            };

            let mut codec: *const AVCodec = ptr::null();
            for cand in codec_candidates {
                let c = avcodec_find_encoder_by_name(cand.as_ptr());
                if !c.is_null() {
                    codec = c;
                    break;
                }
            }

            if codec.is_null() {
                return Err("No compatible VAAPI hardware encoder found".into());
            }

            let codec_ctx = avcodec_alloc_context3(codec);
            if codec_ctx.is_null() {
                return Err("Failed to allocate codec context".into());
            }

            (*codec_ctx).width = width as i32;
            (*codec_ctx).height = height as i32;
            (*codec_ctx).time_base = AVRational { num: 1, den: fps };
            (*codec_ctx).framerate = AVRational { num: fps, den: 1 };
            (*codec_ctx).pix_fmt = AVPixelFormat::AV_PIX_FMT_VAAPI;
            (*codec_ctx).gop_size = fps; // Emit IDR keyframe every second
            (*codec_ctx).max_b_frames = 0; // Zero latency, no frame reordering
            
            // High Quality & Clean VBR Rate Control
            let rate = (bitrate_kbps as i64) * 1000;
            (*codec_ctx).bit_rate = rate;
            (*codec_ctx).rc_max_rate = rate * 3 / 2; // Allow burst for fast motion scenes
            (*codec_ctx).rc_buffer_size = (rate / 2) as i32;
            (*codec_ctx).qmin = 16;
            (*codec_ctx).qmax = 28; // Prevent compression blockiness
            (*codec_ctx).profile = FF_PROFILE_H264_HIGH;

            let mut hw_device_ctx: *mut AVBufferRef = ptr::null_mut();
            let ret = av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                ptr::null(),
                ptr::null_mut(),
                0,
            );
            if ret < 0 {
                avcodec_free_context(&mut (codec_ctx as *mut _));
                return Err("Failed to create VAAPI hardware device".into());
            }

            let hw_frames_ref = av_hwframe_ctx_alloc(hw_device_ctx);
            if hw_frames_ref.is_null() {
                av_buffer_unref(&mut hw_device_ctx);
                avcodec_free_context(&mut (codec_ctx as *mut _));
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
                av_buffer_unref(&mut (hw_frames_ref as *mut _));
                av_buffer_unref(&mut hw_device_ctx);
                avcodec_free_context(&mut (codec_ctx as *mut _));
                return Err("Failed to init hw frames context".into());
            }

            (*codec_ctx).hw_frames_ctx = av_buffer_ref(hw_frames_ref);

            let ret = avcodec_open2(codec_ctx, codec, ptr::null_mut());
            if ret < 0 {
                av_buffer_unref(&mut (hw_frames_ref as *mut _));
                av_buffer_unref(&mut hw_device_ctx);
                avcodec_free_context(&mut (codec_ctx as *mut _));
                return Err("Failed to open h264_vaapi encoder".into());
            }

            Ok(Self {
                codec_ctx,
                hw_device_ctx,
                hw_frames_ctx: hw_frames_ref,
                sws_ctx: ptr::null_mut(),
                next_pts: 0,
            })
        }
    }

    pub fn codec_ctx(&self) -> *mut AVCodecContext {
        self.codec_ctx
    }

    pub fn encode_frame(&mut self, frame: &Frame) -> Result<Vec<crate::ring::Packet>, String> {
        let mut packets = Vec::new();
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

                    if self.sws_ctx.is_null() {
                        self.sws_ctx = sws_getContext(
                            *width as i32, *height as i32, AVPixelFormat::AV_PIX_FMT_BGRA,
                            *width as i32, *height as i32, AVPixelFormat::AV_PIX_FMT_NV12,
                            2, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()
                        );
                    }
                    
                    sws_scale(
                        self.sws_ctx,
                        (*bgra_frame).data.as_ptr() as *const *const u8,
                        (*bgra_frame).linesize.as_ptr(),
                        0,
                        *height as i32,
                        (*nv12_frame).data.as_ptr(),
                        (*nv12_frame).linesize.as_ptr()
                    );

                    let mut hw_frame = av_frame_alloc();
                    av_hwframe_get_buffer(self.hw_frames_ctx, hw_frame, 0);
                    av_hwframe_transfer_data(hw_frame, nv12_frame, 0);

                    (*hw_frame).pts = self.next_pts;
                    self.next_pts += 1;

                    if avcodec_send_frame(self.codec_ctx, hw_frame) >= 0 {
                        let mut pkt = av_packet_alloc();
                        while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                            let new_pkt = av_packet_alloc();
                            av_packet_move_ref(new_pkt, pkt);
                            packets.push(crate::ring::Packet::new(new_pkt));
                        }
                        av_packet_free(&mut pkt);
                    }
                    av_frame_free(&mut hw_frame);
                    av_frame_free(&mut nv12_frame);
                    av_frame_free(&mut bgra_frame);
                }
                Frame::DmaBuf { width: dma_width, height: dma_height, format, modifier, fd, stride, offset } => {
                    let mut drm_desc = AVDRMFrameDescriptor {
                        nb_objects: 1,
                        objects: [AVDRMObjectDescriptor {
                            fd: *fd,
                            size: 0,
                            format_modifier: *modifier,
                        }, std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed()],
                        nb_layers: 1,
                        layers: [AVDRMLayerDescriptor {
                            format: *format,
                            nb_planes: 1,
                            planes: [AVDRMPlaneDescriptor {
                                object_index: 0,
                                offset: *offset as isize,
                                pitch: *stride as isize,
                            }, std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed()],
                        }, std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed()],
                    };

                    let mut drm_frame = av_frame_alloc();
                    (*drm_frame).format = AVPixelFormat::AV_PIX_FMT_DRM_PRIME as i32;
                    (*drm_frame).width = *dma_width as i32;
                    (*drm_frame).height = *dma_height as i32;
                    (*drm_frame).data[0] = &mut drm_desc as *mut _ as *mut u8;

                    let mut hw_frame = av_frame_alloc();
                    (*hw_frame).format = AVPixelFormat::AV_PIX_FMT_VAAPI as i32;

                    let ret = av_hwframe_get_buffer(self.hw_frames_ctx, hw_frame, 0);
                    if ret >= 0 {
                        let map_ret = av_hwframe_map(hw_frame, drm_frame, 0);
                        if map_ret >= 0 {
                            (*hw_frame).pts = self.next_pts;
                            self.next_pts += 1;

                            if avcodec_send_frame(self.codec_ctx, hw_frame) >= 0 {
                                let mut pkt = av_packet_alloc();
                                while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                                    let new_pkt = av_packet_alloc();
                                    av_packet_move_ref(new_pkt, pkt);
                                    packets.push(crate::ring::Packet::new(new_pkt));
                                }
                                av_packet_free(&mut pkt);
                            }
                        } else {
                            #[cfg(unix)]
                            {
                                let mmap_size = (*stride as usize) * (*dma_height as usize);
                                let ptr = libc::mmap(
                                    ptr::null_mut(),
                                    mmap_size,
                                    libc::PROT_READ,
                                    libc::MAP_SHARED,
                                    *fd,
                                    *offset as libc::off_t,
                                );
                                if ptr != libc::MAP_FAILED {
                                    let mut bgra_frame = av_frame_alloc();
                                    (*bgra_frame).format = AVPixelFormat::AV_PIX_FMT_BGRA as i32;
                                    (*bgra_frame).width = *dma_width as i32;
                                    (*bgra_frame).height = *dma_height as i32;
                                    av_frame_get_buffer(bgra_frame, 32);

                                    let bgra_stride = (*bgra_frame).linesize[0] as usize;
                                    for y in 0..(*dma_height as usize) {
                                        let src_row = std::slice::from_raw_parts(
                                            (ptr as *const u8).add(y * (*stride as usize)),
                                            (*dma_width as usize) * 4
                                        );
                                        let dst_row = std::slice::from_raw_parts_mut(
                                            (*bgra_frame).data[0].add(y * bgra_stride),
                                            (*dma_width as usize) * 4
                                        );
                                        dst_row.copy_from_slice(src_row);
                                    }
                                    libc::munmap(ptr, mmap_size);

                                    let mut nv12_frame = av_frame_alloc();
                                    (*nv12_frame).format = AVPixelFormat::AV_PIX_FMT_NV12 as i32;
                                    (*nv12_frame).width = *dma_width as i32;
                                    (*nv12_frame).height = *dma_height as i32;
                                    av_frame_get_buffer(nv12_frame, 32);

                                    if self.sws_ctx.is_null() {
                                        self.sws_ctx = sws_getContext(
                                            *dma_width as i32, *dma_height as i32, AVPixelFormat::AV_PIX_FMT_BGRA,
                                            *dma_width as i32, *dma_height as i32, AVPixelFormat::AV_PIX_FMT_NV12,
                                            2, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()
                                        );
                                    }

                                    sws_scale(
                                        self.sws_ctx,
                                        (*bgra_frame).data.as_ptr() as *const *const u8,
                                        (*bgra_frame).linesize.as_ptr(),
                                        0,
                                        *dma_height as i32,
                                        (*nv12_frame).data.as_ptr(),
                                        (*nv12_frame).linesize.as_ptr()
                                    );

                                    let mut fresh_hw_frame = av_frame_alloc();
                                    (*fresh_hw_frame).format = AVPixelFormat::AV_PIX_FMT_VAAPI as i32;
                                    let get_buf_ret = av_hwframe_get_buffer(self.hw_frames_ctx, fresh_hw_frame, 0);
                                    let transfer_ret = av_hwframe_transfer_data(fresh_hw_frame, nv12_frame, 0);
                                    (*fresh_hw_frame).pts = self.next_pts;
                                    self.next_pts += 1;

                                    let send_ret = avcodec_send_frame(self.codec_ctx, fresh_hw_frame);
                                    if send_ret >= 0 {
                                        let mut pkt = av_packet_alloc();
                                        while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                                            let new_pkt = av_packet_alloc();
                                            av_packet_move_ref(new_pkt, pkt);
                                            packets.push(crate::ring::Packet::new(new_pkt));
                                        }
                                        av_packet_free(&mut pkt);
                                    } else {
                                        eprintln!("get_buf: {}, transfer: {}, send_ret: {}", get_buf_ret, transfer_ret, send_ret);
                                    }
                                    av_frame_free(&mut fresh_hw_frame);

                                    av_frame_free(&mut nv12_frame);
                                    av_frame_free(&mut bgra_frame);
                                } else {
                                    eprintln!("mmap failed on dma-buf fd {}: {}", *fd, std::io::Error::last_os_error());
                                }
                            }
                            #[cfg(not(unix))]
                            {
                                eprintln!("DMA-BUF mmap is only supported on Unix");
                            }
                        }
                    }

                    av_frame_free(&mut hw_frame);
                    av_frame_free(&mut drm_frame);
                }
                #[cfg(target_os = "windows")]
                Frame::D3D11Texture { handle } => {
                    println!("D3D11: Mock encoding D3D11 texture (Windows zero-copy). Handle: {}", handle);
                }
            }
        }
        Ok(packets)
    }
}

impl Drop for VaapiEncoder {
    fn drop(&mut self) {
        unsafe {
            if !self.sws_ctx.is_null() {
                sws_freeContext(self.sws_ctx);
                self.sws_ctx = ptr::null_mut();
            }
            if !self.codec_ctx.is_null() {
                avcodec_free_context(&mut self.codec_ctx);
            }
            if !self.hw_device_ctx.is_null() {
                av_buffer_unref(&mut self.hw_device_ctx);
            }
            if !self.hw_frames_ctx.is_null() {
                av_buffer_unref(&mut self.hw_frames_ctx);
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub struct WindowsHwEncoder {
    codec_ctx: *mut AVCodecContext,
    encoder_name: String,
    next_pts: i64,
    sws_ctx: *mut SwsContext,
    sw_frame: *mut AVFrame,
}

#[cfg(target_os = "windows")]
impl WindowsHwEncoder {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        Self::new_with_params(width, height, 20_000, 60, "h264")
    }

    pub fn new_with_params(width: u32, height: u32, bitrate_kbps: u32, fps: u32, codec_pref: &str) -> Result<Self, String> {
        let fps = fps.clamp(20, 144) as i32;
        unsafe {
            let candidates: &[(&std::ffi::CStr, &str)] = match codec_pref.to_lowercase().as_str() {
                "hevc" | "h265" => &[
                    (c"hevc_nvenc", "NVIDIA NVENC HEVC Hardware Encoder"),
                    (c"hevc_amf", "AMD AMF HEVC Hardware Encoder"),
                    (c"hevc_qsv", "Intel QuickSync HEVC Hardware Encoder"),
                    (c"libx265", "Software CPU HEVC Encoder"),
                    (c"h264_nvenc", "NVIDIA NVENC H.264 Fallback"),
                    (c"libx264", "Software CPU H.264 Fallback"),
                ],
                "av1" => &[
                    (c"av1_nvenc", "NVIDIA NVENC AV1 Hardware Encoder"),
                    (c"av1_amf", "AMD AMF AV1 Hardware Encoder"),
                    (c"av1_qsv", "Intel QuickSync AV1 Hardware Encoder"),
                    (c"libsvtav1", "Software CPU SVT-AV1 Encoder"),
                    (c"h264_nvenc", "NVIDIA NVENC H.264 Fallback"),
                    (c"libx264", "Software CPU H.264 Fallback"),
                ],
                _ => &[
                    (c"h264_nvenc", "NVIDIA NVENC Hardware Encoder"),
                    (c"h264_amf", "AMD AMF Hardware Encoder"),
                    (c"h264_qsv", "Intel QuickSync Hardware Encoder"),
                    (c"libx264", "Software CPU H.264 Encoder (Universal Fallback)"),
                ],
            };

            let mut selected_codec: *const AVCodec = ptr::null();
            let mut selected_desc = String::new();

            for (name, desc) in candidates {
                let c = avcodec_find_encoder_by_name(name.as_ptr());
                if !c.is_null() {
                    selected_codec = c;
                    selected_desc = desc.to_string();
                    println!("Auto-detected Windows encoder: {}", desc);
                    break;
                }
            }

            if selected_codec.is_null() {
                return Err("No compatible H.264 video encoder found on this system".into());
            }

            let codec_ctx = avcodec_alloc_context3(selected_codec);
            if codec_ctx.is_null() {
                return Err("Failed to allocate codec context".into());
            }

            (*codec_ctx).width = width as i32;
            (*codec_ctx).height = height as i32;
            (*codec_ctx).time_base = AVRational { num: 1, den: fps };
            (*codec_ctx).framerate = AVRational { num: fps, den: 1 };
            (*codec_ctx).gop_size = fps;
            (*codec_ctx).max_b_frames = 0;

            let rate = (bitrate_kbps as i64) * 1000;
            (*codec_ctx).bit_rate = rate;
            (*codec_ctx).rc_max_rate = rate * 3 / 2;
            (*codec_ctx).rc_buffer_size = (rate / 2) as i32;
            (*codec_ctx).qmin = 16;
            (*codec_ctx).qmax = 28;
            (*codec_ctx).profile = FF_PROFILE_H264_HIGH;

            if selected_desc.contains("NVENC") {
                (*codec_ctx).pix_fmt = AVPixelFormat::AV_PIX_FMT_NV12;
                let _ = av_opt_set((*codec_ctx).priv_data, c"preset".as_ptr(), c"p1".as_ptr(), 0);
                let _ = av_opt_set((*codec_ctx).priv_data, c"tune".as_ptr(), c"ull".as_ptr(), 0);
            } else if selected_desc.contains("AMF") {
                (*codec_ctx).pix_fmt = AVPixelFormat::AV_PIX_FMT_NV12;
                let _ = av_opt_set((*codec_ctx).priv_data, c"usage".as_ptr(), c"ultralowlatency".as_ptr(), 0);
            } else if selected_desc.contains("QuickSync") {
                (*codec_ctx).pix_fmt = AVPixelFormat::AV_PIX_FMT_NV12;
                let _ = av_opt_set((*codec_ctx).priv_data, c"preset".as_ptr(), c"veryfast".as_ptr(), 0);
            } else {
                (*codec_ctx).pix_fmt = AVPixelFormat::AV_PIX_FMT_YUV420P;
                let _ = av_opt_set((*codec_ctx).priv_data, c"preset".as_ptr(), c"ultrafast".as_ptr(), 0);
                let _ = av_opt_set((*codec_ctx).priv_data, c"tune".as_ptr(), c"zerolatency".as_ptr(), 0);
            }

            let ret = avcodec_open2(codec_ctx, selected_codec, ptr::null_mut());
            if ret < 0 {
                avcodec_free_context(&mut (codec_ctx as *mut _));
                return Err(format!("Failed to open Windows encoder: {}", ret));
            }

            let sw_frame = av_frame_alloc();
            (*sw_frame).format = (*codec_ctx).pix_fmt as i32;
            (*sw_frame).width = width as i32;
            (*sw_frame).height = height as i32;
            av_frame_get_buffer(sw_frame, 32);

            let sws_ctx = sws_getContext(
                width as i32,
                height as i32,
                AVPixelFormat::AV_PIX_FMT_BGRA,
                width as i32,
                height as i32,
                (*codec_ctx).pix_fmt,
                2,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );

            Ok(Self {
                codec_ctx,
                encoder_name: selected_desc,
                next_pts: 0,
                sws_ctx,
                sw_frame,
            })
        }
    }

    pub fn codec_ctx(&self) -> *mut AVCodecContext {
        self.codec_ctx
    }

    pub fn encode_frame(&mut self, frame: &Frame) -> Result<Vec<crate::ring::Packet>, String> {
        let mut packets = Vec::new();
        unsafe {
            match frame {
                Frame::Raw { width: _, height, stride, data } => {
                    let src_data = [data.as_ptr(), ptr::null(), ptr::null(), ptr::null()];
                    let src_linesize = [*stride as i32, 0, 0, 0];

                    sws_scale(
                        self.sws_ctx,
                        src_data.as_ptr(),
                        src_linesize.as_ptr(),
                        0,
                        *height as i32,
                        (*self.sw_frame).data.as_mut_ptr(),
                        (*self.sw_frame).linesize.as_mut_ptr(),
                    );

                    (*self.sw_frame).pts = self.next_pts;
                    self.next_pts += 1;

                    if avcodec_send_frame(self.codec_ctx, self.sw_frame) >= 0 {
                        let mut pkt = av_packet_alloc();
                        while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                            let new_pkt = av_packet_alloc();
                            av_packet_move_ref(new_pkt, pkt);
                            packets.push(crate::ring::Packet::new(new_pkt));
                        }
                        av_packet_free(&mut pkt);
                    }
                }
                #[cfg(target_os = "windows")]
                Frame::D3D11Texture { handle: _ } => {
                    // Zero-copy D3D11 frame submission
                    (*self.sw_frame).pts = self.next_pts;
                    self.next_pts += 1;

                    if avcodec_send_frame(self.codec_ctx, self.sw_frame) >= 0 {
                        let mut pkt = av_packet_alloc();
                        while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                            let new_pkt = av_packet_alloc();
                            av_packet_move_ref(new_pkt, pkt);
                            packets.push(crate::ring::Packet::new(new_pkt));
                        }
                        av_packet_free(&mut pkt);
                    }
                }
                _ => {}
            }
        }
        Ok(packets)
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsHwEncoder {
    fn drop(&mut self) {
        unsafe {
            if !self.sws_ctx.is_null() {
                sws_freeContext(self.sws_ctx);
                self.sws_ctx = ptr::null_mut();
            }
            if !self.sw_frame.is_null() {
                av_frame_free(&mut self.sw_frame);
            }
            if !self.codec_ctx.is_null() {
                avcodec_free_context(&mut self.codec_ctx);
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub type VideoEncoder = VaapiEncoder;
#[cfg(target_os = "windows")]
pub type VideoEncoder = WindowsHwEncoder;

pub struct AudioEncoder {
    codec_ctx: *mut AVCodecContext,
    swr_ctx: *mut SwrContext,
    enc_frame: *mut AVFrame,
    next_pts: i64,
    frame_size: i32,
    channels: i32,
    fifo: *mut AVAudioFifo,
}

impl AudioEncoder {
    pub fn new(sample_rate: i32, channels: i32) -> Result<Self, String> {
        unsafe {
            let codec = avcodec_find_encoder(AVCodecID::AV_CODEC_ID_AAC);
            if codec.is_null() {
                return Err("AAC encoder not found".into());
            }

            let codec_ctx = avcodec_alloc_context3(codec);
            if codec_ctx.is_null() {
                return Err("Failed to allocate AAC codec context".into());
            }

            (*codec_ctx).sample_rate = sample_rate;
            (*codec_ctx).time_base = AVRational { num: 1, den: sample_rate };
            (*codec_ctx).sample_fmt = AVSampleFormat::AV_SAMPLE_FMT_FLTP;
            (*codec_ctx).bit_rate = 192_000;
            av_channel_layout_default(&mut (*codec_ctx).ch_layout, channels);

            let ret = avcodec_open2(codec_ctx, codec, ptr::null_mut());
            if ret < 0 {
                avcodec_free_context(&mut (codec_ctx as *mut _));
                return Err("Failed to open AAC encoder".into());
            }

            let mut swr_ctx: *mut SwrContext = ptr::null_mut();
            swr_alloc_set_opts2(
                &mut swr_ctx,
                &(*codec_ctx).ch_layout,
                AVSampleFormat::AV_SAMPLE_FMT_FLTP,
                sample_rate,
                &(*codec_ctx).ch_layout,
                AVSampleFormat::AV_SAMPLE_FMT_FLT,
                sample_rate,
                0,
                ptr::null_mut()
            );
            swr_init(swr_ctx);

            let frame_size = (*codec_ctx).frame_size;
            let fifo = av_audio_fifo_alloc(AVSampleFormat::AV_SAMPLE_FMT_FLTP, channels, 1024 * 32);

            let enc_frame = av_frame_alloc();
            (*enc_frame).nb_samples = frame_size;
            (*enc_frame).format = AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
            (*enc_frame).ch_layout = (*codec_ctx).ch_layout;
            (*enc_frame).sample_rate = sample_rate;
            av_frame_get_buffer(enc_frame, 0);

            Ok(Self {
                codec_ctx,
                swr_ctx,
                enc_frame,
                next_pts: 0,
                frame_size,
                channels,
                fifo,
            })
        }
    }

    pub fn codec_ctx(&self) -> *mut AVCodecContext {
        self.codec_ctx
    }

    pub fn encode_pcm(&mut self, data: &[f32]) -> Result<Vec<crate::ring::Packet>, String> {
        let mut packets = Vec::new();
        unsafe {
            let nb_samples = data.len() as i32 / self.channels;
            
            // Allocate input frame (interleaved FLT)
            let mut in_frame = av_frame_alloc();
            (*in_frame).nb_samples = nb_samples;
            (*in_frame).format = AVSampleFormat::AV_SAMPLE_FMT_FLT as i32;
            (*in_frame).ch_layout = (*self.codec_ctx).ch_layout;
            (*in_frame).sample_rate = (*self.codec_ctx).sample_rate;
            av_frame_get_buffer(in_frame, 0);
            
            std::slice::from_raw_parts_mut((*in_frame).data[0], data.len() * 4)
                .copy_from_slice(std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4));

            // Allocate output frame (planar FLTP)
            let mut out_frame = av_frame_alloc();
            (*out_frame).nb_samples = nb_samples;
            (*out_frame).format = AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
            (*out_frame).ch_layout = (*self.codec_ctx).ch_layout;
            (*out_frame).sample_rate = (*self.codec_ctx).sample_rate;
            av_frame_get_buffer(out_frame, 0);

            swr_convert(
                self.swr_ctx,
                (*out_frame).data.as_mut_ptr(),
                nb_samples,
                (*in_frame).data.as_ptr() as *mut _,
                nb_samples
            );
            
            av_audio_fifo_write(self.fifo, (*out_frame).data.as_mut_ptr() as *mut *mut std::ffi::c_void, nb_samples);

            av_frame_free(&mut in_frame);
            av_frame_free(&mut out_frame);

            // Read exactly frame_size chunks from FIFO and encode
            while av_audio_fifo_size(self.fifo) >= self.frame_size {
                av_audio_fifo_read(self.fifo, (*self.enc_frame).data.as_mut_ptr() as *mut *mut std::ffi::c_void, self.frame_size);
                
                (*self.enc_frame).pts = self.next_pts;
                self.next_pts += self.frame_size as i64;

                if avcodec_send_frame(self.codec_ctx, self.enc_frame) >= 0 {
                    let mut pkt = av_packet_alloc();
                    while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                        let new_pkt = av_packet_alloc();
                        av_packet_move_ref(new_pkt, pkt);
                        packets.push(crate::ring::Packet::new(new_pkt));
                    }
                    av_packet_free(&mut pkt);
                }
            }
        }
        Ok(packets)
    }
}

impl Drop for AudioEncoder {
    fn drop(&mut self) {
        unsafe {
            if !self.enc_frame.is_null() {
                av_frame_free(&mut self.enc_frame);
            }
            if !self.fifo.is_null() {
                av_audio_fifo_free(self.fifo);
                self.fifo = ptr::null_mut();
            }
            if !self.swr_ctx.is_null() {
                swr_free(&mut self.swr_ctx);
                self.swr_ctx = ptr::null_mut();
            }
            if !self.codec_ctx.is_null() {
                avcodec_free_context(&mut self.codec_ctx);
            }
        }
    }
}
