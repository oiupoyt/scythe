use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::thread;

pub struct AudioCapture {
    _streams: Vec<cpal::Stream>,
}

pub fn get_device_name(dev: &cpal::Device) -> String {
    dev.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| dev.to_string())
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for dev in devices {
            let name = get_device_name(&dev);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

impl AudioCapture {
    pub fn new(sender: Sender<Vec<f32>>) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device_and_mode(sender, None, "system")
    }

    pub fn new_with_device(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device_and_mode(sender, device_name, "system")
    }

    pub fn new_with_device_and_mode(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
        audio_mode: &str,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        let host = cpal::default_host();

        if audio_mode == "muted" {
            println!("Audio capture mode: MUTED (No audio recorded)");
            return Ok((Self { _streams: Vec::new() }, 48000, 2));
        }

        // Helper to find monitor device (system sound)
        let find_system_device = || -> Option<cpal::Device> {
            if let Ok(devs) = host.input_devices() {
                for d in devs {
                    let n = get_device_name(&d).to_lowercase();
                    if n.contains("monitor") {
                        return Some(d);
                    }
                }
            }
            host.default_input_device()
        };

        // Helper to find microphone device
        let find_mic_device = || -> Option<cpal::Device> {
            if let Some(target) = device_name
                && target != "default"
                && !target.trim().is_empty() {
                    if let Ok(devs) = host.input_devices() {
                        for d in devs {
                            if get_device_name(&d) == target {
                                return Some(d);
                            }
                        }
                    }
                }
            if let Ok(devs) = host.input_devices() {
                for d in devs {
                    let n = get_device_name(&d).to_lowercase();
                    if !n.contains("monitor") {
                        return Some(d);
                    }
                }
            }
            host.default_input_device()
        };

        // Helper to build a stream for a device
        let build_stream = |dev: &cpal::Device, tx: Sender<Vec<f32>>| -> Result<(cpal::Stream, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
            let config = dev.default_input_config()?;
            let sample_rate = config.sample_rate();
            let channels = config.channels();
            let format = config.sample_format();
            let err_fn = |err| eprintln!("Audio stream error: {}", err);
            let stream_config: cpal::StreamConfig = config.into();

            let stream = match format {
                cpal::SampleFormat::F32 => {
                    dev.build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &_| {
                            let _ = tx.try_send(data.to_vec());
                        },
                        err_fn,
                        None,
                    )?
                }
                cpal::SampleFormat::I16 => {
                    dev.build_input_stream(
                        stream_config,
                        move |data: &[i16], _: &_| {
                            let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                            let _ = tx.try_send(f32_data);
                        },
                        err_fn,
                        None,
                    )?
                }
                _ => return Err("Unsupported audio format".into()),
            };
            stream.play()?;
            Ok((stream, sample_rate, channels))
        };

        match audio_mode {
            "mic" => {
                let dev = find_mic_device().ok_or("No microphone device found")?;
                let name = get_device_name(&dev);
                let (stream, sr, ch) = build_stream(&dev, sender)?;
                println!("Audio capture mode: MICROPHONE ONLY [{}] ({} Hz, {} ch)", name, sr, ch);
                Ok((Self { _streams: vec![stream] }, sr, ch))
            }
            "both" => {
                let sys_dev = find_system_device().ok_or("No system audio monitor found")?;
                let mic_dev = find_mic_device().ok_or("No microphone found")?;

                let sys_name = get_device_name(&sys_dev);
                let mic_name = get_device_name(&mic_dev);

                let (sys_tx, sys_rx) = crossbeam_channel::bounded::<Vec<f32>>(100);
                let (mic_tx, mic_rx) = crossbeam_channel::bounded::<Vec<f32>>(100);

                let (sys_stream, sr, ch) = build_stream(&sys_dev, sys_tx)?;
                let (mic_stream, _, _) = build_stream(&mic_dev, mic_tx)?;

                let out_tx = sender;
                thread::spawn(move || {
                    let mut sys_q: VecDeque<f32> = VecDeque::with_capacity(16384);
                    let mut mic_q: VecDeque<f32> = VecDeque::with_capacity(16384);

                    loop {
                        while let Ok(chunk) = sys_rx.try_recv() {
                            sys_q.extend(chunk);
                        }
                        while let Ok(chunk) = mic_rx.try_recv() {
                            mic_q.extend(chunk);
                        }

                        let avail = sys_q.len().min(mic_q.len());
                        if avail >= 480 {
                            let mut mixed = Vec::with_capacity(avail);
                            for _ in 0..avail {
                                let s = sys_q.pop_front().unwrap_or(0.0);
                                let m = mic_q.pop_front().unwrap_or(0.0);
                                mixed.push((s + m * 0.9).clamp(-1.0, 1.0));
                            }
                            let _ = out_tx.try_send(mixed);
                        } else if sys_q.len() > 4800 {
                            let chunk: Vec<f32> = sys_q.drain(..960.min(sys_q.len())).collect();
                            let _ = out_tx.try_send(chunk);
                        } else if mic_q.len() > 4800 {
                            let chunk: Vec<f32> = mic_q.drain(..960.min(mic_q.len())).collect();
                            let _ = out_tx.try_send(chunk);
                        }

                        thread::sleep(std::time::Duration::from_millis(4));
                    }
                });

                println!("Audio capture mode: BOTH (System [{}] + Mic [{}]) ({} Hz, {} ch)", sys_name, mic_name, sr, ch);
                Ok((Self { _streams: vec![sys_stream, mic_stream] }, sr, ch))
            }
            _ => {
                // "system" is default
                let dev = find_system_device().ok_or("No audio device available")?;
                let name = get_device_name(&dev);
                let (stream, sr, ch) = build_stream(&dev, sender)?;
                println!("Audio capture mode: SYSTEM SOUNDS ONLY [{}] ({} Hz, {} ch)", name, sr, ch);
                Ok((Self { _streams: vec![stream] }, sr, ch))
            }
        }
    }
}
