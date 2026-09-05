#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use scythe::capture::{Frame, FrameSource};
use scythe::encoder::VideoEncoder;
use scythe::ring::Packet;
use scythe::muxer::Muxer;
use scythe::ipc::{Command, DaemonStatus};
use ringbuf::HeapRb;
use ringbuf::traits::{RingBuffer, Consumer, Observer};
use crossbeam_channel::bounded;
use std::thread;
use std::env;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

fn ensure_wayland_env() {
    #[cfg(target_os = "linux")]
    {
        let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        unsafe {
            if env::var("WAYLAND_DISPLAY").is_err()
                && let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with("wayland-") && !name.ends_with(".lock") {
                            env::set_var("WAYLAND_DISPLAY", &name);
                            println!("Auto-detected Wayland display: {}", name);
                            break;
                        }
                    }
            }
            if env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
                let bus_path = format!("{}/bus", runtime_dir);
                if std::path::Path::new(&bus_path).exists() {
                    env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", bus_path));
                }
            }
            if env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
                let hypr_dir = std::path::Path::new(&runtime_dir).join("hypr");
                if let Ok(entries) = std::fs::read_dir(&hypr_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            let sig = entry.file_name().to_string_lossy().to_string();
                            if !sig.is_empty() {
                                env::set_var("HYPRLAND_INSTANCE_SIGNATURE", &sig);
                                println!("Auto-detected Hyprland instance signature: {}", sig);
                                break;
                            }
                        }
                    }
                }
            }
            if env::var("XDG_CURRENT_DESKTOP").is_err() {
                env::set_var("XDG_CURRENT_DESKTOP", "Hyprland");
            }
            if env::var("XDG_SESSION_TYPE").map(|s| s == "tty" || s.is_empty()).unwrap_or(true) && env::var("WAYLAND_DISPLAY").is_ok() {
                env::set_var("XDG_SESSION_TYPE", "wayland");
            }
        }
    }
}

#[tokio::main]
#[allow(unused_assignments)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    ensure_wayland_env();

    #[cfg(unix)]
    let _lock_file = {
        let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        let lock_path = format!("{}/scythe.lock", runtime_dir);
        match std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(file) => {
                use std::os::unix::io::AsRawFd;
                let res = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
                if res != 0 {
                    eprintln!("Another instance of scythe-daemon is already running. Exiting.");
                    return Ok(());
                }
                Some(file)
            }
            Err(_) => None,
        }
    };
    let initial_config = scythe::config::ScytheConfig::load();

    // Automatically register dynamic keybindings on Hyprland if active (no config edits needed)
    scythe::hyprland_binds::register_hyprland_binds(&initial_config);
    scythe::hyprland_binds::spawn_hyprland_reload_watcher();

    let (frame_tx, frame_rx) = bounded::<Frame>(5);
    let (cmd_tx, cmd_rx) = bounded::<Command>(32);
    let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);
    let (audio_tx, audio_rx) = bounded::<Vec<f32>>(500);

    let is_recording_state = Arc::new(AtomicBool::new(false));
    let record_start_state = Arc::new(AtomicU64::new(0));
    let replay_enabled_state = Arc::new(AtomicBool::new(initial_config.replay_enabled));
    let audio_muted_state = Arc::new(AtomicBool::new(false));
    let audio_levels = Arc::new(scythe::capture::audio::AudioLevels::new());

    #[cfg(unix)]
    let listener = {
        let socket_path = scythe::ipc::get_socket_path();
        let _ = std::fs::remove_file(&socket_path); 
        let l = std::os::unix::net::UnixListener::bind(&socket_path)?;
        println!("Daemon listening on IPC socket: {}", socket_path);
        l
    };

    #[cfg(windows)]
    let listener = {
        match std::net::TcpListener::bind("127.0.0.1:42069") {
            Ok(l) => {
                println!("Daemon listening on TCP IPC: 127.0.0.1:42069");
                l
            }
            Err(e) => {
                if scythe::ipc::query_status().is_ok() {
                    println!("Another instance of scythe-daemon is already running. Exiting cleanly.");
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                std::net::TcpListener::bind("127.0.0.1:42069")?
            }
        }
    };

    let cmd_tx_ipc = cmd_tx.clone();
    let is_recording_state_ipc = Arc::clone(&is_recording_state);
    let record_start_state_ipc = Arc::clone(&record_start_state);
    let replay_enabled_state_ipc = Arc::clone(&replay_enabled_state);
    let audio_muted_state_ipc = Arc::clone(&audio_muted_state);
    let audio_levels_ipc = Arc::clone(&audio_levels);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    let cmd_tx_ipc = cmd_tx_ipc.clone();
                    let is_recording_state_ipc = Arc::clone(&is_recording_state_ipc);
                    let record_start_state_ipc = Arc::clone(&record_start_state_ipc);
                    let replay_enabled_state_ipc = Arc::clone(&replay_enabled_state_ipc);
                    let audio_muted_state_ipc = Arc::clone(&audio_muted_state_ipc);
                    let audio_levels_ipc = Arc::clone(&audio_levels_ipc);

                    std::thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(1500)));
                        let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(1500)));
                        let mut len_buf = [0u8; 4];
                        if stream.read_exact(&mut len_buf).is_ok() {
                            let len = u32::from_le_bytes(len_buf) as usize;
                            let mut payload = vec![0u8; len];
                            if stream.read_exact(&mut payload).is_ok()
                                && let Ok(cmd) = serde_json::from_slice::<Command>(&payload) {
                                    match cmd {
                                        Command::GetStatus => {
                                            let rec = is_recording_state_ipc.load(Ordering::SeqCst);
                                            let start_ts = record_start_state_ipc.load(Ordering::SeqCst);
                                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                                            let duration = if rec && start_ts > 0 {
                                                now.saturating_sub(start_ts)
                                            } else {
                                                0
                                            };
                                            let cfg = scythe::config::ScytheConfig::load();
                                            let status = DaemonStatus {
                                                is_recording: rec,
                                                recording_duration_sec: duration,
                                                is_replay_active: replay_enabled_state_ipc.load(Ordering::SeqCst),
                                                audio_muted: audio_muted_state_ipc.load(Ordering::SeqCst),
                                                audio_mode: cfg.audio_mode,
                                                show_cursor: cfg.show_cursor,
                                                mic_volume: cfg.mic_volume,
                                                system_volume: cfg.system_volume,
                                                mic_level_peak: audio_levels_ipc.get_mic_peak(),
                                                system_level_peak: audio_levels_ipc.get_system_peak(),
                                            };
                                            if let Ok(resp) = serde_json::to_vec(&status) {
                                                let len_resp = (resp.len() as u32).to_le_bytes();
                                                let _ = stream.write_all(&len_resp);
                                                let _ = stream.write_all(&resp);
                                            }
                                        },
                                        Command::ToggleCursor => {
                                            let mut cfg = scythe::config::ScytheConfig::load();
                                            cfg.show_cursor = !cfg.show_cursor;
                                            let _ = cfg.save();
                                            #[cfg(target_os = "linux")]
                                            {
                                                let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
                                                if session_type.to_lowercase() == "wayland" {
                                                    println!("Cursor display toggled to: {}. Restarting Wayland session...", cfg.show_cursor);
                                                    let _ = cmd_tx_ipc.send(Command::StopRecording);
                                                    std::thread::sleep(std::time::Duration::from_millis(150));
                                                    std::process::exit(0);
                                                }
                                            }
                                            println!("Cursor display toggled to: {}", cfg.show_cursor);
                                        },
                                        Command::CycleAudioMode => {
                                            let mut cfg = scythe::config::ScytheConfig::load();
                                            cfg.audio_mode = match cfg.audio_mode.as_str() {
                                                "system" => "mic",
                                                "mic" => "both",
                                                "both" => "muted",
                                                _ => "system",
                                            }.to_string();
                                            let _ = cfg.save();
                                            println!("Audio mode cycled to: {}", cfg.audio_mode);
                                            let _ = cmd_tx_ipc.try_send(Command::ReloadConfig);
                                        },
                                        Command::StopDaemon => {
                                            println!("StopDaemon requested: Finalizing active recordings...");
                                            scythe::hyprland_binds::unregister_hyprland_binds(&scythe::config::ScytheConfig::load());
                                            let _ = cmd_tx_ipc.send(Command::StopRecording);
                                            std::thread::sleep(std::time::Duration::from_millis(350));
                                            std::process::exit(0);
                                        },
                                        other => {
                                            let _ = cmd_tx_ipc.try_send(other);
                                        }
                                    }
                                }
                        }
                    });
                }
                Err(err) => {
                    eprintln!("Socket error: {}", err);
                }
            }
        }
    });

    let cmd_tx_sig = cmd_tx.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            println!("\nReceived shutdown signal. Finalizing recordings...");
            let _ = cmd_tx_sig.send(Command::StopRecording);
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            scythe::hyprland_binds::unregister_hyprland_binds(&scythe::config::ScytheConfig::load());
            std::process::exit(0);
        }
    });

    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    println!("Detected session type: {}", session_type);

    let audio_capture_initial = scythe::capture::audio::AudioCapture::new_with_device_mode_volumes_and_levels(
        audio_tx.clone(),
        Some(&initial_config.audio_device),
        &initial_config.audio_mode,
        initial_config.mic_volume,
        initial_config.system_volume,
        Some(Arc::clone(&audio_levels)),
    ).ok();

    let mut source_opt: Option<Box<dyn FrameSource>> = None;

    if std::env::args().any(|a| a == "--mock") {
        println!("Initializing MOCK capture...");
        source_opt = Some(Box::new(scythe::capture::mock::MockCapture::new()));
    } else if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            println!("Initializing Windows DXGI capture...");
            if let Ok(c) = scythe::capture::windows::WindowsCapture::new() {
                source_opt = Some(Box::new(c));
            }
        }
    } else if session_type.to_lowercase() == "wayland" {
        #[cfg(target_os = "linux")]
        {
            for attempt in 1..=5 {
                println!("Initializing Wayland capture (cursor: {}, attempt {}/5)...", initial_config.show_cursor, attempt);
                // Ensure desktop portal is running
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "start", "xdg-desktop-portal-hyprland"])
                    .status();

                match scythe::capture::wayland::WaylandCapture::new_with_cursor(initial_config.show_cursor).await {
                    Ok(cap) => {
                        source_opt = Some(Box::new(cap));
                        break;
                    }
                    Err(e) => {
                        eprintln!("Wayland capture attempt {} failed: {}", attempt, e);
                        if attempt < 5 {
                            let _ = std::process::Command::new("systemctl")
                                .args(["--user", "restart", "xdg-desktop-portal"])
                                .status();
                            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                        }
                    }
                }
            }
        }
    } else {
        #[cfg(target_os = "linux")]
        {
            println!("Initializing X11 capture...");
            if let Ok(c) = scythe::capture::x11::X11Capture::new() {
                source_opt = Some(Box::new(c));
            }
        }
    }

    let mut source: Box<dyn FrameSource> = match source_opt {
        Some(s) => s,
        None => {
            eprintln!("Warning: Hardware capture unavailable; falling back to mock capture to preserve daemon IPC.");
            Box::new(scythe::capture::mock::MockCapture::new())
        }
    };

    let first_frame = match source.next_frame() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Initial frame capture error: {}. Using default frame.", e);
            Frame::Raw {
                data: vec![0u8; 1920 * 1080 * 4],
                width: 1920,
                height: 1080,
                stride: 1920 * 4,
            }
        }
    };
    let (width, height) = match &first_frame {
        Frame::Raw { width, height, .. } => (*width, *height),
        Frame::DmaBuf { width, height, .. } => (*width, *height),
        #[cfg(target_os = "windows")]
        Frame::D3D11Texture { width, height, .. } => (*width, *height),
    };

    let _ = frame_tx.send(first_frame);

    let capture_tx = frame_tx.clone();
    thread::spawn(move || {
        loop {
            match source.next_frame() {
                Ok(frame) => {
                    let _ = capture_tx.try_send(frame);
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
    let audio_tx_clone = audio_tx.clone();
    let audio_info = audio_capture_initial.as_ref().map(|(_, sr, ch)| (*sr, *ch));
    #[allow(unused_variables, unused_assignments)]
    let mut audio_capture = audio_capture_initial.map(|(c, _, _)| c);

    let recorder_handle = thread::spawn(move || {
        let mut config = scythe::config::ScytheConfig::load();
        replay_state_clone.store(config.replay_enabled, Ordering::SeqCst);

        let mut encoder = VideoEncoder::new_with_params(width, height, config.record_bitrate_kbps, config.fps, &config.video_codec)
            .expect("Failed to init encoder");
        let codec_ctx_ptr = encoder.codec_ctx() as usize;
        
        let mut audio_encoder = if let Some((sr, ch)) = audio_info {
            scythe::encoder::AudioEncoder::new(sr as i32, ch as i32).ok()
        } else {
            scythe::encoder::AudioEncoder::new(48000, 2).ok()
        };
        let audio_codec_ctx_ptr = audio_encoder.as_ref().map(|e| e.codec_ctx() as usize); 
        
        let video_time_base = unsafe { (*(codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext)).time_base };
        let audio_time_base = audio_codec_ctx_ptr.map(|p| unsafe { (*(p as *mut ffmpeg_next::ffi::AVCodecContext)).time_base });

        let mut ring = HeapRb::<Packet>::new((config.replay_duration_sec * 120).max(120) as usize);
        let mut normal_muxer: Option<Muxer> = None;
        let mut normal_recording = false;
        let mut normal_waiting_keyframe = false;
        let mut rec_base_video_pts: i64 = 0;
        let mut rec_base_audio_pts: i64 = -1;

        thread::spawn(move || {
            while let Ok(drain) = mux_rx.recv() {
                let filename = scythe::config::ScytheConfig::format_video_filename("Replay", "mp4");
                let full_path = scythe::config::ScytheConfig::load().resolve_save_path(&filename);
                println!("Saving replay to {}...", full_path);

                // Find the first video IDR keyframe (ignore audio packets which always have keyframe flag set)
                let start_video_idx = drain.iter().position(|p| p.stream_index() == 0 && p.is_keyframe());
                let first_v_idx = match start_video_idx {
                    Some(idx) => idx,
                    None => {
                        println!("No video keyframe found in buffer, skipping save.");
                        continue;
                    }
                };

                let first_video_pts = drain[first_v_idx].pts();
                let first_video_time_us = unsafe {
                    ffmpeg_next::ffi::av_rescale_q(
                        first_video_pts,
                        video_time_base,
                        ffmpeg_next::ffi::AVRational { num: 1, den: 1_000_000 },
                    )
                };

                // Find the audio packet in the ring buffer closest in real time to the first video keyframe
                let first_audio_pts = drain
                    .iter()
                    .filter(|p| p.stream_index() == 1)
                    .min_by_key(|p| {
                        let a_time_us = if let Some(a_tb) = audio_time_base {
                            unsafe {
                                ffmpeg_next::ffi::av_rescale_q(
                                    p.pts(),
                                    a_tb,
                                    ffmpeg_next::ffi::AVRational { num: 1, den: 1_000_000 },
                                )
                            }
                        } else {
                            0
                        };
                        (a_time_us - first_video_time_us).abs()
                    })
                    .map(|p| p.pts())
                    .unwrap_or(0);

                let mut prepared: Vec<(i64, Packet)> = Vec::with_capacity(drain.len());
                for (i, p) in drain.iter().enumerate() {
                    if p.stream_index() == 0 {
                        if i >= first_v_idx {
                            let rebased = p.rebased(first_video_pts);
                            let time_us = unsafe {
                                ffmpeg_next::ffi::av_rescale_q(
                                    rebased.pts(),
                                    video_time_base,
                                    ffmpeg_next::ffi::AVRational { num: 1, den: 1_000_000 },
                                )
                            };
                            prepared.push((time_us, rebased));
                        }
                    } else if p.stream_index() == 1
                        && p.pts() >= first_audio_pts {
                            let rebased = p.rebased(first_audio_pts);
                            let time_us = if let Some(a_tb) = audio_time_base {
                                unsafe {
                                    ffmpeg_next::ffi::av_rescale_q(
                                        rebased.pts(),
                                        a_tb,
                                        ffmpeg_next::ffi::AVRational { num: 1, den: 1_000_000 },
                                    )
                                }
                            } else {
                                0
                            };
                            prepared.push((time_us, rebased));
                    }
                }

                // Sort packets by presentation timestamp so muxer receives strictly chronological stream
                prepared.sort_by_key(|(t, _)| *t);

                let codec_ctx = codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext;
                let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);
                match unsafe { Muxer::new(&full_path, codec_ctx, audio_codec_ctx) } {
                    Ok(mut muxer) => {
                        for (_, p) in prepared {
                            let _ = muxer.write_packet(&p);
                        }
                        let _ = muxer.finalize();
                        println!("Replay saved to {}!", full_path);
                    }
                    Err(e) => {
                        eprintln!("Failed to create muxer for {}: {}", full_path, e);
                    }
                }
            }
        });

        let stream_start = std::time::Instant::now();
        let mut last_video_pts: i64 = -1;
        let mut ticker = crossbeam_channel::tick(std::time::Duration::from_nanos(1_000_000_000 / config.fps.max(1) as u64));
        let mut latest_frame: Option<Frame> = None;
        let mut has_new_frame = false;

        loop {
            crossbeam_channel::select! {
                recv(cmd_rx) -> cmd_res => {
                    if let Ok(cmd) = cmd_res {
                        match cmd {
                            Command::ReloadConfig => {
                                let new_config = scythe::config::ScytheConfig::load();
                                if new_config.show_cursor != config.show_cursor {
                                    println!("Cursor display changed ({} -> {}). Restarting daemon for new capture session...", config.show_cursor, new_config.show_cursor);
                                    if let Some(mut m) = normal_muxer.take() {
                                        let _ = m.finalize();
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(150));
                                    std::process::exit(0);
                                }
                                config = new_config;
                                replay_state_clone.store(config.replay_enabled, Ordering::SeqCst);
                                audio_muted_clone.store(config.audio_mode == "muted", Ordering::SeqCst);
                                println!("Daemon config reloaded! Audio mode: {}", config.audio_mode);
                                let new_capacity = (config.replay_duration_sec * 120).max(120) as usize;
                                if ring.capacity().get() != new_capacity {
                                    ring = HeapRb::<Packet>::new(new_capacity);
                                    println!("Replay buffer resized to {} packets.", new_capacity);
                                }
                                scythe::hyprland_binds::register_hyprland_binds(&config);
                                audio_capture = match scythe::capture::audio::AudioCapture::new_with_device_mode_volumes_and_levels(
                                    audio_tx_clone.clone(),
                                    Some(&config.audio_device),
                                    &config.audio_mode,
                                    config.mic_volume,
                                    config.system_volume,
                                    Some(Arc::clone(&audio_levels)),
                                ) {
                                    Ok((c, _, _)) => {
                                        println!("Audio capture reloaded for mode: {} (mic: {:.0}%, sys: {:.0}%)", config.audio_mode, config.mic_volume * 100.0, config.system_volume * 100.0);
                                        Some(c)
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to reload audio capture: {}", e);
                                        None
                                    }
                                };
                                ticker = crossbeam_channel::tick(std::time::Duration::from_nanos(1_000_000_000 / config.fps.max(1) as u64));
                            },
                            Command::SaveReplay => {
                                if config.replay_enabled {
                                    let drain = ring.iter().cloned().collect::<Vec<_>>();
                                    println!("SaveReplay triggered: {} packets in ring buffer", drain.len());
                                    let _ = mux_tx.try_send(drain);
                                }
                            },
                            Command::StartRecording => {
                                if !normal_recording {
                                    normal_recording = true;
                                    normal_waiting_keyframe = true;
                                    rec_state_clone.store(true, Ordering::SeqCst);
                                    rec_base_audio_pts = -1;
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
                                rec_base_video_pts = 0;
                                rec_base_audio_pts = -1;
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
                                    rec_base_video_pts = 0;
                                    rec_base_audio_pts = -1;
                                    rec_state_clone.store(false, Ordering::SeqCst);
                                    rec_start_clone.store(0, Ordering::SeqCst);
                                } else {
                                    normal_recording = true;
                                    normal_waiting_keyframe = true;
                                    rec_state_clone.store(true, Ordering::SeqCst);
                                    rec_base_audio_pts = -1;
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
                },
                recv(audio_rx) -> audio_res => {
                    if let Ok(mut audio_chunk) = audio_res {
                        if audio_muted_clone.load(Ordering::Relaxed) {
                            for sample in audio_chunk.iter_mut() {
                                *sample = 0.0;
                            }
                        }
                        if let Some(enc) = audio_encoder.as_mut()
                            && let Ok(audio_packets) = enc.encode_pcm(&audio_chunk) {
                                for mut pkt in audio_packets {
                                    pkt.set_stream_index(1);
                                    if normal_recording && !normal_waiting_keyframe
                                        && let Some(muxer) = normal_muxer.as_mut() {
                                            let rec_base_video_time_us = unsafe {
                                                ffmpeg_next::ffi::av_rescale_q(
                                                    rec_base_video_pts,
                                                    video_time_base,
                                                    ffmpeg_next::ffi::AVRational { num: 1, den: 1_000_000 },
                                                )
                                            };
                                            let a_time_us = if let Some(a_tb) = audio_time_base {
                                                unsafe {
                                                    ffmpeg_next::ffi::av_rescale_q(
                                                        pkt.pts(),
                                                        a_tb,
                                                        ffmpeg_next::ffi::AVRational { num: 1, den: 1_000_000 },
                                                    )
                                                }
                                            } else {
                                                0
                                            };
                                            if a_time_us >= rec_base_video_time_us {
                                                if rec_base_audio_pts < 0 {
                                                    rec_base_audio_pts = pkt.pts();
                                                }
                                                let rebased = pkt.rebased(rec_base_audio_pts);
                                                let _ = muxer.write_packet(&rebased);
                                            }
                                        }
                                    if config.replay_enabled {
                                        ring.push_overwrite(pkt);
                                    }
                                }
                            }
                    }
                },
                recv(frame_rx) -> frame_res => {
                    if let Ok(f) = frame_res {
                        latest_frame = Some(f);
                        has_new_frame = true;
                    }
                },
                recv(ticker) -> _ => {
                    let now = std::time::Instant::now();
                    let elapsed = now.duration_since(stream_start);
                    let raw_pts = (elapsed.as_secs_f64() * config.fps as f64).round() as i64;
                    let pts = if raw_pts > last_video_pts { raw_pts } else { last_video_pts + 1 };

                    let packets_res = if has_new_frame {
                        has_new_frame = false;
                        last_video_pts = pts;
                        if let Some(ref f) = latest_frame {
                            encoder.encode_frame(f, pts)
                        } else {
                            Ok(Vec::new())
                        }
                    } else {
                        // Heartbeat / keepalive: if no new frame for 500ms, emit cached frame to keep video track flowing smoothly
                        if pts - last_video_pts >= (config.fps as i64 / 2).max(1) {
                            last_video_pts = pts;
                            if let Some(ref f) = latest_frame {
                                encoder.encode_frame(f, pts)
                            } else {
                                encoder.encode_cached_frame(pts)
                            }
                        } else {
                            Ok(Vec::new())
                        }
                    };

                    if let Ok(packets) = packets_res {
                        for mut pkt in packets {
                            pkt.set_stream_index(0);
                            if normal_recording {
                                if normal_waiting_keyframe && pkt.is_keyframe() {
                                    normal_waiting_keyframe = false;
                                    rec_base_video_pts = pkt.pts();
                                    rec_base_audio_pts = -1;
                                    let codec_ctx = codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext;
                                    let filename = scythe::config::ScytheConfig::format_video_filename("Recording", "mp4");
                                    let full_path = config.resolve_save_path(&filename);
                                    let audio_codec_ctx = audio_codec_ctx_ptr.map(|p| p as *mut ffmpeg_next::ffi::AVCodecContext);
                                    normal_muxer = unsafe { Muxer::new(&full_path, codec_ctx, audio_codec_ctx).ok() };
                                    println!("Started normal recording to {}", full_path);
                                }
                                
                                if !normal_waiting_keyframe
                                    && let Some(muxer) = normal_muxer.as_mut() {
                                        let rebased = pkt.rebased(rec_base_video_pts);
                                        let _ = muxer.write_packet(&rebased);
                                }
                            }
                            if config.replay_enabled {
                                ring.push_overwrite(pkt);
                            }
                        }
                    }
                }
    }
}
    });

    let _ = recorder_handle.join();
    Ok(())
}
