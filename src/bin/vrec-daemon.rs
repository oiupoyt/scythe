use vrec::capture::{Frame, FrameSource};
use vrec::encoder::VaapiEncoder;
use vrec::ring::Packet;
use vrec::muxer::Muxer;
use vrec::ipc::Command;
use ringbuf::HeapRb;
use ringbuf::traits::{RingBuffer, Consumer, Observer};
use crossbeam_channel::bounded;
use std::thread;
use std::env;
use std::os::unix::net::UnixListener;
use std::io::Read;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    println!("Detected session type: {}", session_type);

    let mut source: Box<dyn FrameSource> = if std::env::args().any(|a| a == "--mock") {
        println!("Initializing MOCK capture...");
        Box::new(vrec::capture::mock::MockCapture::new())
    } else if session_type.to_lowercase() == "wayland" {
        println!("Initializing Wayland capture...");
        Box::new(vrec::capture::wayland::WaylandCapture::new().await?)
    } else {
        println!("Initializing X11 capture...");
        Box::new(vrec::capture::x11::X11Capture::new()?)
    };

    let first_frame = source.next_frame()?;
    let (width, height) = match &first_frame {
        Frame::Raw { width, height, .. } => (*width, *height),
        Frame::DmaBuf { width, height, .. } => (*width, *height),
    };

    let (frame_tx, frame_rx) = bounded::<Frame>(5);
    let (cmd_tx, cmd_rx) = bounded::<Command>(10);
    let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);
    let (audio_tx, audio_rx) = bounded::<Vec<f32>>(100);
    let audio_info = vrec::capture::audio::AudioCapture::new(audio_tx).ok();
    
    let _ = frame_tx.send(first_frame);

    let capture_tx = frame_tx.clone();
    thread::spawn(move || {
        loop {
            match source.next_frame() {
                Ok(frame) => {
                    if capture_tx.send(frame).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Capture error: {}", e);
                    break;
                }
            }
        }
    });

    thread::spawn(move || {
        let mut encoder = VaapiEncoder::new(width, height).expect("Failed to init encoder");
        let codec_ctx_ptr = encoder.codec_ctx() as usize;
        
        let mut audio_encoder = if let Some(ref info) = audio_info {
            vrec::encoder::AudioEncoder::new(info.1 as i32, info.2 as i32).ok()
        } else { None };
        let audio_codec_ctx_ptr = audio_encoder.as_ref().map(|e| e.codec_ctx() as usize); 
        
        let mut config = vrec::config::VrecConfig::load();
        let mut ring = HeapRb::<Packet>::new((config.replay_duration_sec * 120).max(120) as usize);
        let mut normal_muxer: Option<Muxer> = None;
        let mut normal_recording = false;
        let mut normal_waiting_keyframe = false;

        thread::spawn(move || {
            while let Ok(drain) = mux_rx.recv() {
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                let name = format!("replay_{}.mp4", ts);
                println!("Saving replay to {}...", name);
                let mut start_idx = 0;
                for (i, p) in drain.iter().enumerate() {
                    if p.is_keyframe() {
                        start_idx = i;
                        break;
                    }
                }
                
                if start_idx < drain.len() {
                    let codec_ctx = codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext;
                    let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);
                    let mut muxer = unsafe { Muxer::new(&name, codec_ctx, audio_codec_ctx).unwrap() };
                    for p in drain.into_iter().skip(start_idx) {
                        let _ = muxer.write_packet(&p);
                    }
                    let _ = muxer.finalize();
                    println!("Replay saved!");
                } else {
                    println!("No keyframe found in buffer, skipping save.");
                }
            }
        });

        while let Ok(frame) = frame_rx.recv() {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    Command::ReloadConfig => {
                        config = vrec::config::VrecConfig::load();
                        println!("Daemon config reloaded!");
                        let new_capacity = (config.replay_duration_sec * 120).max(120) as usize;
                        if ring.capacity().get() != new_capacity {
                            ring = HeapRb::<Packet>::new(new_capacity);
                            println!("Replay buffer resized to {} frames.", new_capacity);
                        }
                    },
                    Command::SaveReplay => {
                        if config.replay_enabled {
                            let drain = ring.iter().cloned().collect::<Vec<_>>();
                            let _ = mux_tx.try_send(drain);
                        }
                    },
                    Command::StartRecording => {
                        if config.record_enabled && !normal_recording {
                            normal_recording = true;
                            normal_waiting_keyframe = true;
                            println!("StartRecording requested, waiting for keyframe...");
                        }
                    },
                    Command::StopRecording => {
                        if let Some(mut m) = normal_muxer.take() {
                            let _ = m.finalize();
                            println!("Stopped normal recording.");
                        }
                        normal_recording = false;
                    },
                    _ => {}
                }
            }

            while let Ok(audio_chunk) = audio_rx.try_recv() {
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

            if let Ok(packets) = encoder.encode_frame(&frame) {
                for mut pkt in packets {
                    pkt.set_stream_index(0);
                    if config.replay_enabled {
                        ring.push_overwrite(pkt.clone());
                    }
                    if normal_recording {
                        if normal_waiting_keyframe {
                            if pkt.is_keyframe() {
                                normal_waiting_keyframe = false;
                                let codec_ctx = codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext;
                                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                                let name = format!("record_{}.mp4", ts);
                                let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);
                                normal_muxer = unsafe { Muxer::new(&name, codec_ctx, audio_codec_ctx).ok() };
                                println!("Started normal recording to {}", name);
                            }
                        }
                        
                        if !normal_waiting_keyframe {
                            if let Some(muxer) = normal_muxer.as_mut() {
                                let _ = muxer.write_packet(&pkt);
                            }
                        }
                    }
                }
            }
        }
    });

    let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
    let _ = std::fs::remove_file(&socket_path); 
    let listener = UnixListener::bind(&socket_path)?;
    println!("Daemon listening on IPC socket: {}", socket_path);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut len_buf = [0u8; 4];
                if stream.read_exact(&mut len_buf).is_ok() {
                    let len = u32::from_le_bytes(len_buf) as usize;
                    let mut payload = vec![0u8; len];
                    if stream.read_exact(&mut payload).is_ok()
                        && let Ok(cmd) = serde_json::from_slice::<Command>(&payload) {
                            match cmd {
                                Command::StopDaemon => {
                                    break;
                                },
                                other => {
                                    let _ = cmd_tx.try_send(other);
                                }
                            }
                        }
                }
            }
            Err(err) => {
                eprintln!("Socket error: {}", err);
            }
        }
    }

    Ok(())
}
