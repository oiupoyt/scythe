use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;

pub struct AudioCapture {
    _stream: cpal::Stream,
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
            names.push(get_device_name(&dev));
        }
    }
    names
}

impl AudioCapture {
    pub fn new(sender: Sender<Vec<f32>>) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_device(sender, None)
    }

    pub fn new_with_device(
        sender: Sender<Vec<f32>>,
        device_name: Option<&str>,
    ) -> Result<(Self, u32, u16), Box<dyn std::error::Error + Send + Sync>> {
        let host = cpal::default_host();
        let device = if let Some(target) = device_name
            && target != "default"
            && !target.trim().is_empty() {
                if let Ok(mut devs) = host.input_devices() {
                    devs.find(|d| get_device_name(d) == target)
                        .or_else(|| host.default_input_device())
                        .ok_or("Audio device not found")?
                } else {
                    host.default_input_device().ok_or("No audio input device available")?
                }
            } else {
                host.default_input_device().ok_or("No audio input device available")?
            };
        
        let dev_name = get_device_name(&device);
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        
        println!("Audio capture initialized on [{}]: {} Hz, {} channels", dev_name, sample_rate, channels);
        
        let err_fn = |err| eprintln!("Audio stream error: {}", err);
        let stream_config: cpal::StreamConfig = config.into();
        
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    stream_config,
                    move |data: &[f32], _: &_| {
                        let _ = sender.try_send(data.to_vec());
                    },
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                device.build_input_stream(
                    stream_config,
                    move |data: &[i16], _: &_| {
                        let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        let _ = sender.try_send(f32_data);
                    },
                    err_fn,
                    None,
                )?
            }
            _ => return Err("Unsupported audio format".into()),
        };
        
        stream.play()?;
        
        Ok((Self { _stream: stream }, sample_rate, channels))
    }
}
