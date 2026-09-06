use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

#[derive(Debug, Default)]
pub struct AudioLevels {
    mic_peak: AtomicU32,
    system_peak: AtomicU32,
}

impl AudioLevels {
    pub fn new() -> Self {
        Self {
            mic_peak: AtomicU32::new(0),
            system_peak: AtomicU32::new(0),
        }
    }

    pub fn update_mic(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        self.update_val(&self.mic_peak, peak);
    }

    pub fn update_system(&self, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        self.update_val(&self.system_peak, peak);
    }

    fn update_val(&self, atomic: &AtomicU32, peak: f32) {
        let current = f32::from_bits(atomic.load(Ordering::Relaxed));
        let next = if peak >= current {
            peak.min(1.0)
        } else {
            (current * 0.82 + peak * 0.18).max(0.0)
        };
        atomic.store(next.to_bits(), Ordering::Relaxed);
    }

    pub fn get_mic_peak(&self) -> f32 {
        let current = f32::from_bits(self.mic_peak.load(Ordering::Relaxed));
        let decayed = (current * 0.90).max(0.0);
        self.mic_peak.store(decayed.to_bits(), Ordering::Relaxed);
        current
    }

    pub fn get_system_peak(&self) -> f32 {
        let current = f32::from_bits(self.system_peak.load(Ordering::Relaxed));
        let decayed = (current * 0.90).max(0.0);
        self.system_peak.store(decayed.to_bits(), Ordering::Relaxed);
        current
    }
}

pub struct AudioCapture {
    #[cfg(unix)]
    _processes: Vec<std::process::Child>,
    _streams: Vec<cpal::Stream>,
    pub levels: Arc<AudioLevels>,
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        #[cfg(unix)]
        for child in &mut self._processes {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn get_device_name(dev: &cpal::Device) -> String {
    dev.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| dev.to_string())
}

pub fn list_input_devices() -> Vec<String> {
    let mut names = Vec::new();
    #[cfg(unix)]
    {
        // Try pactl sources first for PulseAudio/PipeWire
        if let Ok(out) = std::process::Command::new("pactl").args(["list", "short", "sources"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    names.push(parts[1].to_string());
                }
            }
        }
    }
    if names.is_empty() {
        let host = cpal::default_host();
        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                let name = get_device_name(&dev);
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

pub fn list_application_audio() -> Vec<String> {
    #[allow(unused_mut)]
    let mut apps = Vec::new();
    #[cfg(unix)]
    {
        if let Ok(out) = std::process::Command::new("pactl").args(["list", "sink-inputs"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("application.name = ") {
                    let name = rest.trim_matches('"').trim();
                    if !name.is_empty() && !apps.iter().any(|a: &String| a.eq_ignore_ascii_case(name)) {
                        apps.push(name.to_string());
                    }
                } else if let Some(rest) = trimmed.strip_prefix("media.name = ") {
                    let name = rest.trim_matches('"').trim();
                    if !name.is_empty() && !apps.iter().any(|a: &String| a.eq_ignore_ascii_case(name)) {
                        apps.push(name.to_string());
                    }
                }
            }
        }
    }
    apps
}

#[inline]
pub fn soft_limit(x: f32) -> f32 {
    if x.abs() <= 0.75 {
        x
    } else if x > 0.0 {
        0.75 + 0.24 * ((x - 0.75) / 0.24).tanh()
    } else {
        -0.75 - 0.24 * ((-x - 0.75) / 0.24).tanh()
    }
}

#[cfg(unix)]
fn is_parec_available() -> bool {
    std::process::Command::new("parec")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn spawn_parec_stream(
    device: &str,
    sender: Sender<Vec<f32>>,
    gain: f32,
    levels: Option<Arc<AudioLevels>>,
    is_mic: bool,
) -> Result<std::process::Child, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Read;

    let mut child = std::process::Command::new("parec")
        .args([
            "-d", device,
            "--format=float32le",
            "--rate=48000",
            "--channels=2",
            "--raw",
            "--latency-msec=20",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let mut stdout = child.stdout.take().ok_or("Failed to open parec stdout")?;
    std::thread::spawn(move || {
        let mut remainder = Vec::with_capacity(4);
        let mut read_buf = [0u8; 4096];
        loop {
            match stdout.read(&mut read_buf) {
                Ok(0) => break,
                Ok(n) => {
                    let total_len = remainder.len() + n;
                    let full_bytes = total_len - (total_len % 4);
                    if full_bytes == 0 {
                        remainder.extend_from_slice(&read_buf[..n]);
                        continue;
                    }

                    let mut combined = Vec::with_capacity(total_len);
                    combined.extend_from_slice(&remainder);
                    combined.extend_from_slice(&read_buf[..n]);

                    let floats_count = full_bytes / 4;
                    let mut floats = Vec::with_capacity(floats_count);
                    for i in 0..floats_count {
                        let b = [
                            combined[i * 4],
                            combined[i * 4 + 1],
                            combined[i * 4 + 2],
                            combined[i * 4 + 3],
                        ];
                        let raw = f32::from_le_bytes(b);
                        floats.push(soft_limit(raw * gain));
                    }

                    remainder.clear();
                    remainder.extend_from_slice(&combined[full_bytes..]);

                    if let Some(ref l) = levels {
                        if is_mic {
                            l.update_mic(&floats);
                        } else {
                            l.update_system(&floats);
                        }
                    }

                    if sender.send(floats).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok(child)
}

impl AudioCapture {
    pub fn new(sender: Sender<Vec<f32>>) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device_mode_volumes_and_levels(sender, None, "system", 0.60, 1.0, None)
    }

    pub fn new_with_device(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device_mode_volumes_and_levels(sender, device_name, "system", 0.60, 1.0, None)
    }

    pub fn new_with_device_and_mode(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
        audio_mode: &str,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device_mode_volumes_and_levels(sender, device_name, audio_mode, 0.60, 1.0, None)
    }

    pub fn new_with_device_mode_and_volumes(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
        audio_mode: &str,
        mic_volume: f32,
        system_volume: f32,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device_mode_volumes_and_levels(sender, device_name, audio_mode, mic_volume, system_volume, None)
    }

    pub fn new_with_device_mode_volumes_and_levels(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
        audio_mode: &str,
        mic_volume: f32,
        system_volume: f32,
        levels: Option<Arc<AudioLevels>>,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        let levels = levels.unwrap_or_else(|| Arc::new(AudioLevels::new()));

        if audio_mode == "muted" {
            println!("Audio capture mode: MUTED (No audio recorded)");
            return Ok((Self {
                #[cfg(unix)]
                _processes: Vec::new(),
                _streams: Vec::new(),
                levels,
            }, 48000, 2));
        }

        #[cfg(unix)]
        if is_parec_available() {
            println!("Using native PulseAudio/PipeWire parec capture (48000 Hz, 2 channels)...");
            match audio_mode {
                "mic" => {
                    let mic_target = device_name
                        .filter(|d| *d != "default" && !d.trim().is_empty())
                        .unwrap_or("@DEFAULT_SOURCE@");
                    let child = spawn_parec_stream(mic_target, sender, mic_volume, Some(Arc::clone(&levels)), true)?;
                    println!("Audio capture mode: MICROPHONE ONLY [{}] (vol: {:.0}%, 48000 Hz, 2 ch)", mic_target, mic_volume * 100.0);
                    return Ok((Self {
                        _processes: vec![child],
                        _streams: Vec::new(),
                        levels,
                    }, 48000, 2));
                }
                "both" => {
                    let (sys_tx, sys_rx) = crossbeam_channel::bounded::<Vec<f32>>(100);
                    let (mic_tx, mic_rx) = crossbeam_channel::bounded::<Vec<f32>>(100);

                    let sys_child = spawn_parec_stream("@DEFAULT_MONITOR@", sys_tx, system_volume, Some(Arc::clone(&levels)), false)?;
                    let mic_target = device_name
                        .filter(|d| *d != "default" && !d.trim().is_empty())
                        .unwrap_or("@DEFAULT_SOURCE@");
                    let mic_child = spawn_parec_stream(mic_target, mic_tx, mic_volume, Some(Arc::clone(&levels)), true)?;

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
                                    mixed.push(soft_limit(s + m));
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

                    println!("Audio capture mode: BOTH (System [@DEFAULT_MONITOR@, {:.0}%] + Mic [{}, {:.0}%]) (48000 Hz, 2 ch)", system_volume * 100.0, mic_target, mic_volume * 100.0);
                    return Ok((Self {
                        _processes: vec![sys_child, mic_child],
                        _streams: Vec::new(),
                        levels,
                    }, 48000, 2));
                }
                _ => {
                    // System audio only
                    let child = spawn_parec_stream("@DEFAULT_MONITOR@", sender, system_volume, Some(Arc::clone(&levels)), false)?;
                    println!("Audio capture mode: SYSTEM AUDIO ONLY [@DEFAULT_MONITOR@] (vol: {:.0}%, 48000 Hz, 2 ch)", system_volume * 100.0);
                    return Ok((Self {
                        _processes: vec![child],
                        _streams: Vec::new(),
                        levels,
                    }, 48000, 2));
                }
            }
        }

        // Fallback: CPAL
        let host = cpal::default_host();

        let find_system_device = || -> Option<cpal::Device> {
            if let Ok(devs) = host.input_devices() {
                for d in devs {
                    let n = get_device_name(&d).to_lowercase();
                    if n.contains("pipewire") || n.contains("pulse") || n.contains("monitor") {
                        return Some(d);
                    }
                }
            }
            host.default_input_device()
        };

        let find_mic_device = || -> Option<cpal::Device> {
            if let Some(target) = device_name
                && target != "default"
                && !target.trim().is_empty()
                && let Ok(devs) = host.input_devices() {
                    for d in devs {
                        if get_device_name(&d) == target {
                            return Some(d);
                        }
                    }
                }
            if let Ok(devs) = host.input_devices() {
                for d in devs {
                    let n = get_device_name(&d).to_lowercase();
                    if !n.contains("discard") && !n.contains("null") && !n.contains("monitor") {
                        return Some(d);
                    }
                }
            }
            host.default_input_device()
        };

        let levels_for_cpal = Arc::clone(&levels);
        let build_stream = move |dev: &cpal::Device, tx: Sender<Vec<f32>>, gain: f32, is_mic: bool| -> Result<(cpal::Stream, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
            let config = dev.default_input_config()?;
            let sample_rate = config.sample_rate();
            let channels = config.channels();
            let format = config.sample_format();
            let err_fn = |err| eprintln!("Audio stream error: {}", err);
            let stream_config: cpal::StreamConfig = config.into();
            let lvl_f32 = Arc::clone(&levels_for_cpal);
            let lvl_i16 = Arc::clone(&levels_for_cpal);

            let stream = match format {
                cpal::SampleFormat::F32 => {
                    dev.build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &_| {
                            let f32_data: Vec<f32> = data.iter().map(|&s| soft_limit(s * gain)).collect();
                            if is_mic {
                                lvl_f32.update_mic(&f32_data);
                            } else {
                                lvl_f32.update_system(&f32_data);
                            }
                            let _ = tx.try_send(f32_data);
                        },
                        err_fn,
                        None,
                    )?
                }
                cpal::SampleFormat::I16 => {
                    dev.build_input_stream(
                        stream_config,
                        move |data: &[i16], _: &_| {
                            let f32_data: Vec<f32> = data.iter().map(|&s| soft_limit((s as f32 / i16::MAX as f32) * gain)).collect();
                            if is_mic {
                                lvl_i16.update_mic(&f32_data);
                            } else {
                                lvl_i16.update_system(&f32_data);
                            }
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
                let (stream, sr, ch) = build_stream(&dev, sender, mic_volume, true)?;
                println!("Audio capture mode: MICROPHONE ONLY [{}] ({} Hz, {} ch)", name, sr, ch);
                Ok((Self {
                    #[cfg(unix)]
                    _processes: Vec::new(),
                    _streams: vec![stream],
                    levels,
                }, sr, ch))
            }
            "both" => {
                let sys_dev = find_system_device().ok_or("No system audio monitor found")?;
                let mic_dev = find_mic_device().ok_or("No microphone found")?;

                let sys_name = get_device_name(&sys_dev);
                let mic_name = get_device_name(&mic_dev);

                let (sys_tx, sys_rx) = crossbeam_channel::bounded::<Vec<f32>>(100);
                let (mic_tx, mic_rx) = crossbeam_channel::bounded::<Vec<f32>>(100);

                let (sys_stream, sr, ch) = build_stream(&sys_dev, sys_tx, system_volume, false)?;
                let (mic_stream, _, _) = build_stream(&mic_dev, mic_tx, mic_volume, true)?;

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
                                mixed.push(soft_limit(s + m));
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
                Ok((Self {
                    #[cfg(unix)]
                    _processes: Vec::new(),
                    _streams: vec![sys_stream, mic_stream],
                    levels,
                }, sr, ch))
            }
            _ => {
                let dev = find_system_device().ok_or("No audio device available")?;
                let name = get_device_name(&dev);
                let (stream, sr, ch) = build_stream(&dev, sender, system_volume, false)?;
                println!("Audio capture mode: SYSTEM SOUNDS ONLY [{}] ({} Hz, {} ch)", name, sr, ch);
                Ok((Self {
                    #[cfg(unix)]
                    _processes: Vec::new(),
                    _streams: vec![stream],
                    levels,
                }, sr, ch))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_probe() {
        // 1. Test System audio
        let (tx, rx) = crossbeam_channel::unbounded();
        let sys_cap = AudioCapture::new_with_device_and_mode(tx, None, "system");
        assert!(sys_cap.is_ok(), "System audio capture failed: {:?}", sys_cap.err());
        let mut got_sys_samples = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(600) {
            if let Ok(samples) = rx.try_recv() {
                if !samples.is_empty() {
                    got_sys_samples = true;
                    println!("Received {} system audio samples", samples.len());
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(got_sys_samples, "Did not receive any system audio samples");
        drop(sys_cap);

        // 2. Test Mic audio
        let (tx, rx) = crossbeam_channel::unbounded();
        let mic_cap = AudioCapture::new_with_device_and_mode(tx, None, "mic");
        assert!(mic_cap.is_ok(), "Mic audio capture failed: {:?}", mic_cap.err());
        let mut got_mic_samples = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(600) {
            if let Ok(samples) = rx.try_recv() {
                if !samples.is_empty() {
                    got_mic_samples = true;
                    println!("Received {} mic audio samples", samples.len());
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !got_mic_samples {
            println!("Note: No mic hardware active or producing sound in current test environment");
        }
        drop(mic_cap);

        // 3. Test Both (Mixed) audio
        let (tx, rx) = crossbeam_channel::unbounded();
        let both_cap = AudioCapture::new_with_device_and_mode(tx, None, "both");
        assert!(both_cap.is_ok(), "Both audio capture failed: {:?}", both_cap.err());
        let mut got_both_samples = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(600) {
            if let Ok(samples) = rx.try_recv() {
                if !samples.is_empty() {
                    got_both_samples = true;
                    println!("Received {} mixed audio samples", samples.len());
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !got_both_samples {
            println!("Note: No mixed audio hardware active in current test environment");
        }
        drop(both_cap);
    }
}

