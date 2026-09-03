use vrec::capture::{Frame, FrameSource};
use vrec::encoder::VaapiEncoder;
use vrec::ring::Packet;
use vrec::muxer::Muxer;
use vrec::ipc::{Command, DaemonStatus};
use ringbuf::HeapRb;
use ringbuf::traits::{RingBuffer, Consumer, Observer};
use crossbeam_channel::bounded;
use std::thread;
use std::env;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    println!("Detected session type: {}", session_type);

    let mut source: Box<dyn FrameSource> = if std::env::args().any(|a| a == "--mock") {
        println!("Initializing MOCK capture...");
        Box::new(vrec::capture::mock::MockCapture::new())
    } else if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            println!("Initializing Windows DXGI capture...");
            Box::new(vrec::capture::windows::WindowsCapture::new()?)
        }
        #[cfg(not(target_os = "windows"))]
        {
            unreachable!()
        }
    } else if session_type.to_lowercase() == "wayland" {
        #[cfg(target_os = "linux")]
        {
            println!("Initializing Wayland capture...");
            Box::new(vrec::capture::wayland::WaylandCapture::new().await?)
        }
        #[cfg(not(target_os = "linux"))]
        {
            unreachable!()
        }
    } else {
        #[cfg(target_os = "linux")]
        {
            println!("Initializing X11 capture...");
            Box::new(vrec::capture::x11::X11Capture::new()?)
        }
        #[cfg(not(target_os = "linux"))]
        {
            unreachable!()
        }
    };

    let first_frame = source.next_frame()?;
    let (width, height) = match &first_frame {
        Frame::Raw { width, height, .. } => (*width, *height),
        Frame::DmaBuf { width, height, .. } => (*width, *height),
        #[cfg(target_os = "windows")]
        Frame::D3D11Texture { .. } => (1920, 1080),
    };

    let (frame_tx, frame_rx) = bounded::<Frame>(5);
    let (cmd_tx, cmd_rx) = bounded::<Command>(10);
    let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);
    let (audio_tx, audio_rx) = bounded::<Vec<f32>>(100);
    let audio_info = vrec::capture::audio::AudioCapture::new(audio_tx).ok();

    let is_recording_state = Arc::new(AtomicBool::new(false));
    let record_start_state = Arc::new(AtomicU64::new(0));
    let replay_enabled_state = Arc::new(AtomicBool::new(true));
    let audio_muted_state = Arc::new(AtomicBool::new(false));

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

    let rec_state_clone = Arc::clone(&is_recording_state);
    let rec_start_clone = Arc::clone(&record_start_state);
    let replay_state_clone = Arc::clone(&replay_enabled_state);
    let audio_muted_clone = Arc::clone(&audio_muted_state);

    thread::spawn(move || {
        let mut config = vrec::config::VrecConfig::load();
        replay_state_clone.store(config.replay_enabled, Ordering::SeqCst);

        let mut encoder = VaapiEncoder::new_with_params(width, height, config.record_bitrate_kbps, config.fps)
            .expect("Failed to init encoder");
        let codec_ctx_ptr = encoder.codec_ctx() as usize;
        
        let mut audio_encoder = if let Some(ref info) = audio_info {
            vrec::encoder::AudioEncoder::new(info.1 as i32, info.2 as i32).ok()
        } else {
            None
        };
        let audio_codec_ctx_ptr = audio_encoder.as_ref().map(|e| e.codec_ctx() as usize); 
        
        let mut ring = HeapRb::<Packet>::new((config.replay_duration_sec * 120).max(120) as usize);
        let mut normal_muxer: Option<Muxer> = None;
        let mut normal_recording = false;
        let mut normal_waiting_keyframe = false;

        thread::spawn(move || {
            while let Ok(drain) = mux_rx.recv() {
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                let filename = format!("replay_{}.mp4", ts);
                let full_path = vrec::config::VrecConfig::load().resolve_save_path(&filename);
                println!("Saving replay to {}...", full_path);
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
                    let mut muxer = unsafe { Muxer::new(&full_path, codec_ctx, audio_codec_ctx).unwrap() };
                    for p in drain.into_iter().skip(start_idx) {
                        let _ = muxer.write_packet(&p);
                    }
                    let _ = muxer.finalize();
                    println!("Replay saved to {}!", full_path);
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
                        replay_state_clone.store(config.replay_enabled, Ordering::SeqCst);
                        println!("Daemon config reloaded!");
                        let new_capacity = (config.replay_duration_sec * 120).max(120) as usize;
                        if ring.capacity().get() != new_capacity {
                            ring = HeapRb::<Packet>::new(new_capacity);
                            println!("Replay buffer resized to {} packets.", new_capacity);
                        }
                    },
                    Command::SaveReplay => {
                        if config.replay_enabled {
                            let drain = ring.iter().cloned().collect::<Vec<_>>();
                            let _ = mux_tx.try_send(drain);
                        }
                    },
                    Command::StartRecording => {
                        if !normal_recording {
                            normal_recording = true;
                            normal_waiting_keyframe = true;
                            rec_state_clone.store(true, Ordering::SeqCst);
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                            rec_start_clone.store(now, Ordering::SeqCst);
                            println!("StartRecording requested, waiting for keyframe...");
                        }
                    },
                    Command::StopRecording => {
                        if let Some(mut m) = normal_muxer.take() {
                            let _ = m.finalize();
                            println!("Stopped normal recording.");
                        }
                        normal_recording = false;
                        normal_waiting_keyframe = false;
                        rec_state_clone.store(false, Ordering::SeqCst);
                        rec_start_clone.store(0, Ordering::SeqCst);
                    },
                    Command::ToggleRecording => {
                        if normal_recording {
                            if let Some(mut m) = normal_muxer.take() {
                                let _ = m.finalize();
                                println!("Stopped normal recording.");
                            }
                            normal_recording = false;
                            normal_waiting_keyframe = false;
                            rec_state_clone.store(false, Ordering::SeqCst);
                            rec_start_clone.store(0, Ordering::SeqCst);
                        } else {
                            normal_recording = true;
                            normal_waiting_keyframe = true;
                            rec_state_clone.store(true, Ordering::SeqCst);
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                            rec_start_clone.store(now, Ordering::SeqCst);
                            println!("ToggleRecording: StartRecording requested, waiting for keyframe...");
                        }
                    },
                    Command::ToggleAudio => {
                        let cur = audio_muted_clone.load(Ordering::SeqCst);
                        audio_muted_clone.store(!cur, Ordering::SeqCst);
                        println!("Audio mute toggled: {}", !cur);
                    },
                    _ => {}
                }
            }

            while let Ok(mut audio_chunk) = audio_rx.try_recv() {
                if audio_muted_clone.load(Ordering::Relaxed) {
                    for sample in audio_chunk.iter_mut() {
                        *sample = 0.0;
                    }
                }
                if let Some(enc) = audio_encoder.as_mut()
                    && let Ok(audio_packets) = enc.encode_pcm(&audio_chunk) {
                        for mut pkt in audio_packets {
                            pkt.set_stream_index(1);
                            if config.replay_enabled {
                                ring.push_overwrite(pkt.clone());
                            }
                            if normal_recording && !normal_waiting_keyframe
                                && let Some(muxer) = normal_muxer.as_mut() {
                                    let _ = muxer.write_packet(&pkt);
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
                        if normal_waiting_keyframe && pkt.is_keyframe() {
                            normal_waiting_keyframe = false;
                            let codec_ctx = codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext;
                            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                            let filename = format!("record_{}.mp4", ts);
                            let full_path = config.resolve_save_path(&filename);
                            let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);
                            normal_muxer = unsafe { Muxer::new(&full_path, codec_ctx, audio_codec_ctx).ok() };
                            println!("Started normal recording to {}", full_path);
                        }
                        
                        if !normal_waiting_keyframe
                            && let Some(muxer) = normal_muxer.as_mut() {
                                let _ = muxer.write_packet(&pkt);
                            }
                    }
                }
            }
        }
    });

    #[cfg(unix)]
    let listener = {
        let socket_path = format!("{}/vrec.sock", env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string()));
        let _ = std::fs::remove_file(&socket_path); 
        let l = std::os::unix::net::UnixListener::bind(&socket_path)?;
        println!("Daemon listening on IPC socket: {}", socket_path);
        l
    };

    #[cfg(windows)]
    let listener = {
        let l = std::net::TcpListener::bind("127.0.0.1:42069")?;
        println!("Daemon listening on TCP IPC: 127.0.0.1:42069");
        l
    };

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
                                Command::GetStatus => {
                                    let rec = is_recording_state.load(Ordering::SeqCst);
                                    let start_ts = record_start_state.load(Ordering::SeqCst);
                                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                                    let duration = if rec && start_ts > 0 {
                                        now.saturating_sub(start_ts)
                                    } else {
                                        0
                                    };
                                    let status = DaemonStatus {
                                        is_recording: rec,
                                        recording_duration_sec: duration,
                                        is_replay_active: replay_enabled_state.load(Ordering::SeqCst),
                                        audio_muted: audio_muted_state.load(Ordering::SeqCst),
                                    };
                                    if let Ok(resp) = serde_json::to_vec(&status) {
                                        let len_resp = (resp.len() as u32).to_le_bytes();
                                        let _ = stream.write_all(&len_resp);
                                        let _ = stream.write_all(&resp);
                                    }
                                },
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
