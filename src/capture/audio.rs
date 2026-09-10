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

#[derive(Debug, Default)]
pub struct ResampleState {
    pub phase: f64,
}

pub fn convert_to_stereo_48k(
    raw_data: &[f32],
    in_channels: u16,
    in_sample_rate: u32,
    gain: f32,
    state: &mut ResampleState,
) -> Vec<f32> {
    let ch = (in_channels as usize).max(1);
    let num_frames = raw_data.len() / ch;
    if num_frames == 0 {
        return Vec::new();
    }

    // Step 1: Up/downmix any input channel layout into stereo (2 channels)
    let mut stereo_in = Vec::with_capacity(num_frames * 2);
    match ch {
        1 => {
            for &sample in raw_data {
                let s = sample * gain;
                stereo_in.push(s);
                stereo_in.push(s);
            }
        }
        2 => {
            for pair in raw_data.chunks_exact(2) {
                stereo_in.push(pair[0] * gain);
                stereo_in.push(pair[1] * gain);
            }
        }
        3 => {
            for frame in raw_data.chunks_exact(3) {
                let center = frame[2] * std::f32::consts::FRAC_1_SQRT_2;
                stereo_in.push((frame[0] + center) * gain);
                stereo_in.push((frame[1] + center) * gain);
            }
        }
        _ => {
            // Surround downmix (4, 5.1, 7.1)
            for frame in raw_data.chunks_exact(ch) {
                let center = if ch > 2 { frame[2] * std::f32::consts::FRAC_1_SQRT_2 } else { 0.0 };
                let l_surround = if ch > 4 { frame[4] * std::f32::consts::FRAC_1_SQRT_2 } else { 0.0 };
                let r_surround = if ch > 5 { frame[5] * std::f32::consts::FRAC_1_SQRT_2 } else { 0.0 };
                let l = (frame[0] + center + l_surround) * gain;
                let r = (frame[1] + center + r_surround) * gain;
                stereo_in.push(l);
                stereo_in.push(r);
            }
        }
    }

    // Step 2: Sample rate conversion to 48,000 Hz
    if in_sample_rate == 48000 || in_sample_rate == 0 {
        for sample in stereo_in.iter_mut() {
            *sample = soft_limit(*sample);
        }
        return stereo_in;
    }

    let in_frames = stereo_in.len() / 2;
    let ratio = in_sample_rate as f64 / 48000.0;
    let est_out = ((in_frames as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(est_out * 2 + 8);

    while state.phase < in_frames as f64 {
        let idx = state.phase.floor() as usize;
        let frac = (state.phase - idx as f64) as f32;

        let (cur_l, cur_r) = (stereo_in[idx * 2], stereo_in[idx * 2 + 1]);
        let (next_l, next_r) = if idx + 1 < in_frames {
            (stereo_in[(idx + 1) * 2], stereo_in[(idx + 1) * 2 + 1])
        } else {
            (cur_l, cur_r)
        };

        let l = cur_l + (next_l - cur_l) * frac;
        let r = cur_r + (next_r - cur_r) * frac;

        out.push(soft_limit(l));
        out.push(soft_limit(r));

        state.phase += ratio;
    }

    state.phase -= in_frames as f64;
    if state.phase < 0.0 {
        state.phase = 0.0;
    }

    out
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
        let mut remainder = Vec::with_capacity(4096);
        let mut read_buf = [0u8; 4096];
        loop {
            match stdout.read(&mut read_buf) {
                Ok(0) => break,
                Ok(n) => {
                    remainder.extend_from_slice(&read_buf[..n]);
                    // 2 channels * 4 bytes/sample = 8 bytes per stereo frame
                    let full_bytes = remainder.len() - (remainder.len() % 8);
                    if full_bytes == 0 {
                        continue;
                    }

                    let floats_count = full_bytes / 4;
                    let mut floats = Vec::with_capacity(floats_count);
                    for chunk in remainder[..full_bytes].chunks_exact(4) {
                        let raw = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        floats.push(soft_limit(raw * gain));
                    }

                    remainder.drain(..full_bytes);

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
                        let mut sys_closed = false;
                        let mut mic_closed = false;

                        loop {
                            loop {
                                match sys_rx.try_recv() {
                                    Ok(chunk) => sys_q.extend(chunk),
                                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                        sys_closed = true;
                                        break;
                                    }
                                }
                            }
                            loop {
                                match mic_rx.try_recv() {
                                    Ok(chunk) => mic_q.extend(chunk),
                                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                        mic_closed = true;
                                        break;
                                    }
                                }
                            }

                            // 2 channels stereo alignment
                            let avail = ((sys_q.len().min(mic_q.len())) / 2) * 2;
                            if avail >= 480 {
                                let mut mixed = Vec::with_capacity(avail);
                                for (s, m) in sys_q.drain(..avail).zip(mic_q.drain(..avail)) {
                                    mixed.push(soft_limit(s + m));
                                }
                                if out_tx.try_send(mixed).is_err() && !out_tx.is_full() {
                                    break;
                                }
                            } else if sys_q.len() > 4800 {
                                let drain_len = (960.min(sys_q.len()) / 2) * 2;
                                let chunk: Vec<f32> = sys_q.drain(..drain_len).collect();
                                if out_tx.try_send(chunk).is_err() && !out_tx.is_full() {
                                    break;
                                }
                            } else if mic_q.len() > 4800 {
                                let drain_len = (960.min(mic_q.len()) / 2) * 2;
                                let chunk: Vec<f32> = mic_q.drain(..drain_len).collect();
                                if out_tx.try_send(chunk).is_err() && !out_tx.is_full() {
                                    break;
                                }
                            }

                            if sys_closed && mic_closed && sys_q.is_empty() && mic_q.is_empty() {
                                break;
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
            #[cfg(target_os = "windows")]
            {
                if let Some(out_dev) = host.default_output_device() {
                    return Some(out_dev);
                }
            }
            #[cfg(not(target_os = "windows"))]
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
            let config = dev.default_input_config().or_else(|_| dev.default_output_config())?;
            let in_sample_rate = config.sample_rate();
            let in_channels = config.channels();
            let format = config.sample_format();
            let err_fn = |err| eprintln!("Audio stream error: {}", err);
            let stream_config: cpal::StreamConfig = config.into();
            let lvl_f32 = Arc::clone(&levels_for_cpal);
            let lvl_i16 = Arc::clone(&levels_for_cpal);

            let mut resample_state_f32 = ResampleState::default();
            let mut resample_state_i16 = ResampleState::default();

            let stream = match format {
                cpal::SampleFormat::F32 => {
                    dev.build_input_stream(
                        stream_config,
                        move |data: &[f32], _: &_| {
                            let f32_data = convert_to_stereo_48k(data, in_channels, in_sample_rate, gain, &mut resample_state_f32);
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
                            let converted_f32: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                            let f32_data = convert_to_stereo_48k(&converted_f32, in_channels, in_sample_rate, gain, &mut resample_state_i16);
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
            Ok((stream, 48000, 2))
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
                    let mut sys_closed = false;
                    let mut mic_closed = false;

                    loop {
                        loop {
                            match sys_rx.try_recv() {
                                Ok(chunk) => sys_q.extend(chunk),
                                Err(crossbeam_channel::TryRecvError::Empty) => break,
                                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                    sys_closed = true;
                                    break;
                                }
                            }
                        }
                        loop {
                            match mic_rx.try_recv() {
                                Ok(chunk) => mic_q.extend(chunk),
                                Err(crossbeam_channel::TryRecvError::Empty) => break,
                                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                    mic_closed = true;
                                    break;
                                }
                            }
                        }

                        // 2 channels stereo alignment
                        let avail = ((sys_q.len().min(mic_q.len())) / 2) * 2;
                        if avail >= 480 {
                            let mut mixed = Vec::with_capacity(avail);
                            for (s, m) in sys_q.drain(..avail).zip(mic_q.drain(..avail)) {
                                mixed.push(soft_limit(s + m));
                            }
                            if out_tx.try_send(mixed).is_err() && !out_tx.is_full() {
                                break;
                            }
                        } else if sys_q.len() > 4800 {
                            let drain_len = (960.min(sys_q.len()) / 2) * 2;
                            let chunk: Vec<f32> = sys_q.drain(..drain_len).collect();
                            if out_tx.try_send(chunk).is_err() && !out_tx.is_full() {
                                break;
                            }
                        } else if mic_q.len() > 4800 {
                            let drain_len = (960.min(mic_q.len()) / 2) * 2;
                            let chunk: Vec<f32> = mic_q.drain(..drain_len).collect();
                            if out_tx.try_send(chunk).is_err() && !out_tx.is_full() {
                                break;
                            }
                        }

                        if sys_closed && mic_closed && sys_q.is_empty() && mic_q.is_empty() {
                            break;
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
            if let Ok(samples) = rx.try_recv()
                && !samples.is_empty() {
                    got_sys_samples = true;
                    println!("Received {} system audio samples", samples.len());
                    break;
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
            if let Ok(samples) = rx.try_recv()
                && !samples.is_empty() {
                    got_mic_samples = true;
                    println!("Received {} mic audio samples", samples.len());
                    break;
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
            if let Ok(samples) = rx.try_recv()
                && !samples.is_empty() {
                    got_both_samples = true;
                    println!("Received {} mixed audio samples", samples.len());
                    break;
                }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !got_both_samples {
            println!("Note: No mixed audio hardware active in current test environment");
        }
        drop(both_cap);
    }

    #[test]
    fn test_convert_mono_44100_to_stereo_48000() {
        let mut state = ResampleState::default();
        // 4410 mono frames at 44.1kHz = 0.1 seconds
        let mono_input: Vec<f32> = (0..4410).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let output = convert_to_stereo_48k(&mono_input, 1, 44100, 1.0, &mut state);

        // At 48000 Hz, 0.1s should yield ~4800 stereo frames = ~9600 floats
        let num_out_frames = output.len() / 2;
        assert!(
            (num_out_frames as i32 - 4800).abs() <= 5,
            "Expected ~4800 frames, got {}",
            num_out_frames
        );

        // Ensure every pair has left == right (mono centered)
        for pair in output.chunks_exact(2) {
            assert!((pair[0] - pair[1]).abs() < 1e-5, "Mono upmix must be centered: {} != {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn test_convert_stereo_48000_passthrough() {
        let mut state = ResampleState::default();
        let stereo_input = vec![0.25f32, -0.25f32, 0.5f32, -0.5f32];
        let output = convert_to_stereo_48k(&stereo_input, 2, 48000, 1.0, &mut state);
        assert_eq!(output.len(), 4);
        assert!((output[0] - 0.25).abs() < 1e-5);
        assert!((output[1] - (-0.25)).abs() < 1e-5);
    }

    #[test]
    fn test_convert_5_1_surround_downmix() {
        let mut state = ResampleState::default();
        // 6 channels: FL, FR, FC, LFE, SL, SR
        let surround_input = vec![0.2f32, 0.2f32, 0.4f32, 0.0f32, 0.1f32, 0.1f32];
        let output = convert_to_stereo_48k(&surround_input, 6, 48000, 1.0, &mut state);
        assert_eq!(output.len(), 2);
        // Both channels should be equal due to symmetrical inputs
        assert!((output[0] - output[1]).abs() < 1e-5);
        assert!(output[0] > 0.2, "Downmixing center/surround should contribute to stereo channel");
    }
}

