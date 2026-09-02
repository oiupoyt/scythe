use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;

pub struct AudioCapture {
    _stream: cpal::Stream,
}

impl AudioCapture {
    pub fn new(sender: Sender<Vec<f32>>) -> Result<(Self, u32, u16), Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("No audio input device available")?;
        
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        
        println!("Audio capture initialized: {} Hz, {} channels", sample_rate, channels);
        
        let err_fn = |err| eprintln!("Audio stream error: {}", err);
        let stream_config: cpal::StreamConfig = config.clone().into();
        
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    stream_config.clone(),
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
