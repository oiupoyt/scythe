pub mod capture;
pub mod encoder;
pub mod ring;
pub mod muxer;
pub mod hotkey;
pub mod debug;

use capture::{Frame, FrameSource};
use encoder::VaapiEncoder;
use ring::Packet;
use muxer::Muxer;
use ringbuf::HeapRb;
use ringbuf::traits::{RingBuffer, Consumer};
use crossbeam_channel::bounded;
use std::thread;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "x11".to_string());
    println!("Detected session type: {}", session_type);

    let mut source: Box<dyn FrameSource> = if session_type.to_lowercase() == "wayland" {
        println!("Initializing Wayland capture...");
        Box::new(capture::wayland::WaylandCapture::new().await?)
    } else {
        println!("Initializing X11 capture...");
        Box::new(capture::x11::X11Capture::new()?)
    };

    let first_frame = source.next_frame()?;
    let (width, height) = match &first_frame {
        Frame::Raw { width, height, .. } => (*width, *height),
        Frame::DmaBuf { width, height, .. } => (*width, *height),
    };

    // 1. Channels
    let (frame_tx, frame_rx) = bounded::<Frame>(5);
    let (trigger_tx, trigger_rx) = bounded::<()>(1);
    let (mux_tx, mux_rx) = bounded::<Vec<Packet>>(1);
    
    // Send first frame to encoder thread
    let _ = frame_tx.send(first_frame);

    // Capture thread
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

    // 3. Encoder Thread
    thread::spawn(move || {
        let mut encoder = VaapiEncoder::new(width, height).expect("Failed to init encoder");
        let codec_ctx_ptr = encoder.codec_ctx() as usize; // Send raw pointer safely
        
        let mut ring = HeapRb::<Packet>::new(3600); // 60 seconds at 60fps

        // Muxer Thread spawned inside here so it shares lifetime conceptually, 
        // though we use mux_rx.
        thread::spawn(move || {
            while let Ok(mut drain) = mux_rx.recv() {
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

        // Encoder loop
        while let Ok(frame) = frame_rx.recv() {
            if let Ok(packets) = encoder.encode_frame(&frame) {
                for pkt in packets {
                    ring.push_overwrite(pkt);
                }
            }
            
            // Check for trigger
            if trigger_rx.try_recv().is_ok() {
                // Clone all packets to send to muxer
                let drain = ring.iter().cloned().collect::<Vec<_>>();
                let _ = mux_tx.try_send(drain);
            }
        }
    });

    // 5. Hotkey listener
    println!("Starting hotkey listener...");
    if let Err(e) = hotkey::run_hotkey_listener(trigger_tx) {
        eprintln!("Failed to start hotkey listener: {}", e);
    }

    Ok(())
}
