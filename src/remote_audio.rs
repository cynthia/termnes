use std::collections::VecDeque;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Protocol magic. Bumped to TNS2 when the wire format changed from f32 to i16.
const MAGIC: &[u8; 4] = b"TNS2";

/// How many per-frame sample batches the sender may hold in-flight before
/// dropping on backpressure. At 60 fps, 16 slots ≈ 270 ms of audio.
/// Dropping is preferable to blocking because blocking would stall the
/// emulation thread when the network is slow.
const SENDER_QUEUE_FRAMES: usize = 16;

/// Convert a clamped [-1.0, 1.0] f32 sample to i16 PCM.
#[inline]
fn f32_to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32767.0) as i16
}

/// Streams audio samples over TCP to a remote listener. All network I/O
/// runs on a dedicated worker thread so the emulation thread never blocks
/// on socket writes. Samples are transmitted as little-endian i16 PCM.
pub struct RemoteAudioSender {
    tx: SyncSender<Vec<f32>>,
}

impl RemoteAudioSender {
    pub fn connect(addr: &str, sample_rate: u32) -> Result<Self, String> {
        let stream =
            TcpStream::connect(addr).map_err(|e| format!("failed to connect to {addr}: {e}"))?;
        // Disable Nagle so small per-frame writes go out immediately rather
        // than being held by the kernel waiting for more data to coalesce.
        let _ = stream.set_nodelay(true);
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
                    byte_buf.reserve(samples.len() * 2);
                    for &s in &samples {
                        byte_buf.extend_from_slice(&f32_to_i16(s).to_le_bytes());
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
    let _ = stream.set_nodelay(true);
    eprintln!("Connection from {addr}");

    // Read header
    let mut reader = BufReader::with_capacity(16384, stream);
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| format!("failed to read header: {e}"))?;
    if &magic != MAGIC {
        return Err(format!(
            "invalid protocol magic (expected {}, got {:?})",
            std::str::from_utf8(MAGIC).unwrap_or("?"),
            magic
        ));
    }
    let mut rate_bytes = [0u8; 4];
    reader
        .read_exact(&mut rate_bytes)
        .map_err(|e| format!("failed to read sample rate: {e}"))?;
    let sample_rate = u32::from_le_bytes(rate_bytes);
    eprintln!("Audio stream: {sample_rate} Hz mono (i16 PCM)");

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

    // Read i16 samples in chunks, convert to f32, and push to playback buffer.
    // Reading a chunk at a time (rather than one sample per syscall) minimizes
    // buffer-lock churn on the audio callback.
    const CHUNK_SAMPLES: usize = 256;
    let mut chunk_bytes = [0u8; CHUNK_SAMPLES * 2];
    let mut scratch: Vec<f32> = Vec::with_capacity(CHUNK_SAMPLES);
    loop {
        match reader.read_exact(&mut chunk_bytes) {
            Ok(()) => {
                scratch.clear();
                for pair in chunk_bytes.chunks_exact(2) {
                    let i = i16::from_le_bytes([pair[0], pair[1]]);
                    scratch.push(i as f32 / 32767.0);
                }
                buffer.lock().unwrap().extend(scratch.iter().copied());
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
