use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;

pub struct AudioOutput {
    _stream: Stream,
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

impl AudioOutput {
    /// Open the default audio output device and start a playback stream.
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no audio output device found".to_string())?;

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer: Arc<Mutex<VecDeque<f32>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(sample_rate as usize)));
        let buf_clone = buffer.clone();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buf = buf_clone.lock().unwrap();
                    // Hold last sample on underrun instead of outputting silence
                    // to avoid pops/clicks.
                    let mut last = 0.0f32;
                    for sample in data.iter_mut() {
                        if let Some(s) = buf.pop_front() {
                            last = s;
                        }
                        *sample = last;
                    }
                },
                |err| {
                    eprintln!("audio stream error: {}", err);
                },
                None,
            )
            .map_err(|e| format!("failed to build audio stream: {}", e))?;

        stream
            .play()
            .map_err(|e| format!("failed to start audio stream: {}", e))?;

        Ok(Self {
            _stream: stream,
            buffer,
        })
    }

    /// Push samples into the playback buffer.
    pub fn queue_samples(&self, samples: &[f32]) {
        let mut buf = self.buffer.lock().unwrap();
        buf.extend(samples);
    }

    /// Number of samples currently buffered (for pacing the emulation loop).
    pub fn buffered_samples(&self) -> usize {
        self.buffer.lock().unwrap().len()
    }
}
