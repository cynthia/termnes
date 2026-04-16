use std::collections::VecDeque;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const MAGIC: &[u8; 4] = b"TNAS";

/// How many per-frame sample batches the sender may hold in-flight before
/// dropping on backpressure. At 60 fps, 16 slots ≈ 270 ms of audio.
/// Dropping is preferable to blocking because blocking would stall the
/// emulation thread when the network is slow.
const SENDER_QUEUE_FRAMES: usize = 16;

/// Streams audio samples over TCP to a remote listener. All network I/O
/// runs on a dedicated worker thread so the emulation thread never blocks
/// on socket writes.
pub struct RemoteAudioSender {
    tx: SyncSender<Vec<f32>>,
}

impl RemoteAudioSender {
    pub fn connect(addr: &str, sample_rate: u32) -> Result<Self, String> {
        let stream =
            TcpStream::connect(addr).map_err(|e| format!("failed to connect to {addr}: {e}"))?;
        let mut writer = BufWriter::new(stream);
        writer
            .write_all(MAGIC)
            .and_then(|()| writer.write_all(&sample_rate.to_le_bytes()))
            .and_then(|()| writer.flush())
            .map_err(|e| format!("failed to send header: {e}"))?;

        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(SENDER_QUEUE_FRAMES);
        thread::Builder::new()
            .name("termnes-audio-tx".into())
            .spawn(move || {
                let mut byte_buf: Vec<u8> = Vec::with_capacity(4096);
                while let Ok(samples) = rx.recv() {
                    byte_buf.clear();
                    byte_buf.reserve(samples.len() * 4);
                    for s in &samples {
                        byte_buf.extend_from_slice(&s.to_le_bytes());
                    }
                    if writer.write_all(&byte_buf).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("failed to spawn audio sender thread: {e}"))?;

        Ok(Self { tx })
    }

    /// Hand a batch of samples to the sender thread. Non-blocking: if the
    /// worker is falling behind and the queue is full, the batch is dropped.
    pub fn queue_samples(&mut self, samples: &[f32]) {
        match self.tx.try_send(samples.to_vec()) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

/// Accept a single connection, receive audio samples, and play them via cpal.
pub fn run_listen(port: u16) -> Result<(), String> {
    let listener =
        TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("failed to bind port {port}: {e}"))?;
    eprintln!("Listening on port {port} for audio stream...");

    let (stream, addr) = listener.accept().map_err(|e| format!("accept failed: {e}"))?;
    eprintln!("Connection from {addr}");

    // Read header
    let mut reader = BufReader::with_capacity(16384, stream);
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| format!("failed to read header: {e}"))?;
    if &magic != MAGIC {
        return Err("invalid protocol magic (expected TNAS)".to_string());
    }
    let mut rate_bytes = [0u8; 4];
    reader
        .read_exact(&mut rate_bytes)
        .map_err(|e| format!("failed to read sample rate: {e}"))?;
    let sample_rate = u32::from_le_bytes(rate_bytes);
    eprintln!("Audio stream: {sample_rate} Hz mono");

    // Open local audio device
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

    let audio_stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut buf = buf_clone.lock().unwrap();
                let mut last = 0.0f32;
                for sample in data.iter_mut() {
                    if let Some(s) = buf.pop_front() {
                        last = s;
                    }
                    *sample = last;
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| format!("failed to build audio stream: {e}"))?;

    audio_stream
        .play()
        .map_err(|e| format!("failed to start audio stream: {e}"))?;

    eprintln!("Playing audio...");

    // Read samples and feed to playback buffer
    let mut sample_bytes = [0u8; 4];
    loop {
        match reader.read_exact(&mut sample_bytes) {
            Ok(()) => {
                buffer
                    .lock()
                    .unwrap()
                    .push_back(f32::from_le_bytes(sample_bytes));
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                eprintln!("Stream ended");
                break;
            }
            Err(e) => {
                eprintln!("Read error: {e}");
                break;
            }
        }
    }

    Ok(())
}
