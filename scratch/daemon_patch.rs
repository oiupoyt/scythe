use std::fs;

fn main() {
    let mut code = fs::read_to_string("src/bin/vrec-daemon.rs").unwrap();
    
    code = code.replace(
        "let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);",
        "let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);\n    let (audio_tx, audio_rx) = bounded::<Vec<f32>>(100);\n    let audio_info = vrec::capture::audio::AudioCapture::new(audio_tx).ok();"
    );
    
    code = code.replace(
        "let codec_ctx_ptr = encoder.codec_ctx() as usize;",
        r#"let codec_ctx_ptr = encoder.codec_ctx() as usize;
        
        let mut audio_encoder = if let Some(ref info) = audio_info {
            vrec::encoder::AudioEncoder::new(info.1 as i32, info.2 as i32).ok()
        } else { None };
        let audio_codec_ctx_ptr = audio_encoder.as_ref().map(|e| e.codec_ctx() as usize);"#
    );
    
    code = code.replace(
        "let mut ring = HeapRb::<Packet>::new((config.replay_duration_sec * 60).max(60) as usize);",
        "let mut ring = HeapRb::<Packet>::new((config.replay_duration_sec * 120).max(120) as usize);"
    );
    
    code = code.replace(
        "let new_capacity = (config.replay_duration_sec * 60).max(60) as usize;",
        "let new_capacity = (config.replay_duration_sec * 120).max(120) as usize;"
    );
    
    code = code.replace(
        "let mut muxer = unsafe { Muxer::new(&name, codec_ctx, audio_codec_ctx).unwrap() };",
        "let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);\n                    let mut muxer = unsafe { Muxer::new(&name, codec_ctx, audio_codec_ctx).unwrap() };"
    );
    
    code = code.replace(
        "normal_muxer = unsafe { Muxer::new(&name, codec_ctx, audio_codec_ctx).ok() };",
        "let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);\n                                normal_muxer = unsafe { Muxer::new(&name, codec_ctx, audio_codec_ctx).ok() };"
    );
    
    // Add the audio pumping logic inside the while let Ok(frame) loop
    code = code.replace(
        "if let Ok(packets) = encoder.encode_frame(&frame) {",
        r#"while let Ok(audio_chunk) = audio_rx.try_recv() {
                if let Some(enc) = audio_encoder.as_mut() {
                    if let Ok(audio_packets) = enc.encode_pcm(&audio_chunk) {
                        for mut pkt in audio_packets {
                            pkt.set_stream_index(1);
                            if config.replay_enabled {
                                ring.push_overwrite(pkt.clone());
                            }
                            if normal_recording && !normal_waiting_keyframe {
                                if let Some(muxer) = normal_muxer.as_mut() {
                                    let _ = muxer.write_packet(&pkt);
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(packets) = encoder.encode_frame(&frame) {"#
    );
    
    // For video packets, set stream index to 0
    code = code.replace(
        "for pkt in packets {",
        "for mut pkt in packets {\n                    pkt.set_stream_index(0);"
    );
    
    fs::write("src/bin/vrec-daemon.rs", code).unwrap();
}
