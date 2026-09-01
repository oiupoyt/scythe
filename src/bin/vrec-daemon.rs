use vrec::capture::{Frame, FrameSource};
use vrec::encoder::VaapiEncoder;
use vrec::ring::Packet;
use vrec::muxer::Muxer;
use vrec::ipc::Command;
use ringbuf::HeapRb;
use ringbuf::traits::{RingBuffer, Consumer};
use crossbeam_channel::bounded;
use std::thread;
use std::env;
use std::os::unix::net::UnixListener;
use std::io::Read;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    println!("Detected session type: {}", session_type);

    let mut source: Box<dyn FrameSource> = if session_type.to_lowercase() == "wayland" {
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
    let (trigger_tx, trigger_rx) = bounded::<()>(1);
    let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);
    
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
        
        let mut ring = HeapRb::<Packet>::new(3600);

        thread::spawn(move || {
            while let Ok(drain) = mux_rx.recv() {
                println!("Saving replay to output.mp4...");
                let mut start_idx = 0;
                for (i, p) in drain.iter().enumerate() {
                    if p.is_keyframe() {
                        start_idx = i;
                        break;
                    }
                }
                
                if start_idx < drain.len() {
                    let codec_ctx = codec_ctx_ptr as *mut ffmpeg_next::ffi::AVCodecContext;
                    let mut muxer = Muxer::new("output.mp4", codec_ctx).unwrap();
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
            if let Ok(packets) = encoder.encode_frame(&frame) {
                for pkt in packets {
                    ring.push_overwrite(pkt);
                }
            }
            
            if trigger_rx.try_recv().is_ok() {
                let drain = ring.iter().cloned().collect::<Vec<_>>();
                let _ = mux_tx.try_send(drain);
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
                    if stream.read_exact(&mut payload).is_ok() {
                        if let Ok(cmd) = serde_json::from_slice::<Command>(&payload) {
                            match cmd {
                                Command::SaveReplay => {
                                    let _ = trigger_tx.try_send(());
                                },
                                Command::StopDaemon => {
                                    break;
                                },
                                _ => {}
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
