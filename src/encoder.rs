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
                            size: 0, // usually ignored or mapped by driver
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

                    // Map the DRM frame to VAAPI
                    // Since it's hardware mapping, av_hwframe_map expects the device context or frames context.
                    // Wait, av_hwframe_map uses the destination's hw_frames_ctx if we preallocate, OR
                    // if we just pass a device context to hw_frame.hw_frames_ctx?
                    // Let's allocate the hw_frame from our VAAPI frames context.
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
                            eprintln!("Failed to map DRM prime to VAAPI: {}", map_ret);
                        }
                    }

                    // For AV_PIX_FMT_DRM_PRIME, we don't own the data, so don't av_frame_unref the data buffer,
                    // av_frame_free is safe because data[0] wasn't allocated by ffmpeg.
                    // Wait, av_frame_free will try to free data[0] if buf[0] is set. But we didn't set buf[0].
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
            avcodec_free_context(&mut self.codec_ctx);
            av_buffer_unref(&mut self.hw_device_ctx);
            av_buffer_unref(&mut self.hw_frames_ctx);
        }
    }
}

pub struct AudioEncoder {
    codec_ctx: *mut AVCodecContext,
    swr_ctx: *mut SwrContext,
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
            (*codec_ctx).sample_rate = sample_rate;
            (*codec_ctx).ch_layout.nb_channels = channels;
            // AAC requires FLTP (planar float)
            (*codec_ctx).sample_fmt = AVSampleFormat::AV_SAMPLE_FMT_FLTP;
            (*codec_ctx).bit_rate = 192_000;
            // For older ffmpeg compatibility:
            (*codec_ctx).channel_layout = if channels == 2 { AV_CH_LAYOUT_STEREO } else { AV_CH_LAYOUT_MONO } as u64;

            let ret = avcodec_open2(codec_ctx, codec, std::ptr::null_mut());
            if ret < 0 {
                return Err("Failed to open AAC encoder".into());
            }

            // Create SwrContext to convert from interleaved F32 (AV_SAMPLE_FMT_FLT) to planar F32 (AV_SAMPLE_FMT_FLTP)
            let mut swr_ctx = swr_alloc();
            av_opt_set_int(swr_ctx as _, c"in_channel_layout".as_ptr(), (*codec_ctx).channel_layout as i64, 0);
            av_opt_set_int(swr_ctx as _, c"in_sample_rate".as_ptr(), sample_rate as i64, 0);
            av_opt_set_sample_fmt(swr_ctx as _, c"in_sample_fmt".as_ptr(), AVSampleFormat::AV_SAMPLE_FMT_FLT, 0);
            
            av_opt_set_int(swr_ctx as _, c"out_channel_layout".as_ptr(), (*codec_ctx).channel_layout as i64, 0);
            av_opt_set_int(swr_ctx as _, c"out_sample_rate".as_ptr(), sample_rate as i64, 0);
            av_opt_set_sample_fmt(swr_ctx as _, c"out_sample_fmt".as_ptr(), AVSampleFormat::AV_SAMPLE_FMT_FLTP, 0);
            
            swr_init(swr_ctx);

            let frame_size = (*codec_ctx).frame_size;
            let fifo = av_audio_fifo_alloc(AVSampleFormat::AV_SAMPLE_FMT_FLTP, channels, 1024 * 10);

            Ok(Self {
                codec_ctx,
                swr_ctx,
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
            (*in_frame).channel_layout = (*self.codec_ctx).channel_layout;
            (*in_frame).sample_rate = (*self.codec_ctx).sample_rate;
            av_frame_get_buffer(in_frame, 0);
            
            std::slice::from_raw_parts_mut((*in_frame).data[0], data.len() * 4)
                .copy_from_slice(std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4));

            // Allocate output frame (planar FLTP)
            let mut out_frame = av_frame_alloc();
            (*out_frame).nb_samples = nb_samples;
            (*out_frame).format = AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
            (*out_frame).channel_layout = (*self.codec_ctx).channel_layout;
            (*out_frame).sample_rate = (*self.codec_ctx).sample_rate;
            av_frame_get_buffer(out_frame, 0);

            swr_convert(
                self.swr_ctx,
                (*out_frame).data.as_mut_ptr(),
                nb_samples,
                (*in_frame).data.as_ptr() as *const *const u8,
                nb_samples
            );
            
            av_audio_fifo_write(self.fifo, (*out_frame).data.as_mut_ptr() as *mut *mut std::ffi::c_void, nb_samples);

            av_frame_free(&mut in_frame);
            av_frame_free(&mut out_frame);

            // Read exactly frame_size chunks from FIFO and encode
            while av_audio_fifo_size(self.fifo) >= self.frame_size {
                let mut enc_frame = av_frame_alloc();
                (*enc_frame).nb_samples = self.frame_size;
                (*enc_frame).format = AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
                (*enc_frame).channel_layout = (*self.codec_ctx).channel_layout;
                (*enc_frame).sample_rate = (*self.codec_ctx).sample_rate;
                av_frame_get_buffer(enc_frame, 0);

                av_audio_fifo_read(self.fifo, (*enc_frame).data.as_mut_ptr() as *mut *mut std::ffi::c_void, self.frame_size);
                
                (*enc_frame).pts = self.next_pts;
                self.next_pts += self.frame_size as i64;

                if avcodec_send_frame(self.codec_ctx, enc_frame) >= 0 {
                    let mut pkt = av_packet_alloc();
                    while avcodec_receive_packet(self.codec_ctx, pkt) >= 0 {
                        let new_pkt = av_packet_alloc();
                        av_packet_move_ref(new_pkt, pkt);
                        packets.push(crate::ring::Packet::new(new_pkt));
                    }
                    av_packet_free(&mut pkt);
                }
                av_frame_free(&mut enc_frame);
            }
        }
        Ok(packets)
    }
}

impl Drop for AudioEncoder {
    fn drop(&mut self) {
        unsafe {
            av_audio_fifo_free(self.fifo);
            swr_free(&mut self.swr_ctx);
            avcodec_free_context(&mut self.codec_ctx);
        }
    }
}
