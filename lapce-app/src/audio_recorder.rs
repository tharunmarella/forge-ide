//! Simple cross-platform audio recorder using cpal + hound.
//!
//! Usage:
//!   let recorder = AudioRecorder::new();
//!   recorder.start()?;         // Opens mic, starts capturing
//!   // ... user speaks ...
//!   let wav = recorder.stop(); // Returns WAV bytes for Whisper
//!
//! The recorder captures mono audio, encodes to 16-bit WAV on stop.

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Shared sample buffer used by both the audio callback and the recorder.
struct SharedBuffer {
    samples: Vec<f32>,
    sample_rate: u32,
}

/// Thread-safe audio recorder.
#[derive(Clone)]
pub struct AudioRecorder {
    buffer: Arc<Mutex<SharedBuffer>>,
    stream: Arc<Mutex<Option<cpal::Stream>>>,
    recording: Arc<Mutex<bool>>,
}

impl std::fmt::Debug for AudioRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let recording = self.recording.lock().map(|r| *r).unwrap_or(false);
        f.debug_struct("AudioRecorder")
            .field("is_recording", &recording)
            .finish()
    }
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(SharedBuffer {
                samples: Vec::new(),
                sample_rate: 16000,
            })),
            stream: Arc::new(Mutex::new(None)),
            recording: Arc::new(Mutex::new(false)),
        }
    }

    /// Start recording from the default microphone.
    pub fn start(&self) -> Result<(), String> {
        {
            let rec = self.recording.lock().unwrap();
            if *rec {
                return Ok(()); // Already recording
            }
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No microphone found".to_string())?;

        let config = device
            .default_input_config()
            .map_err(|e| format!("No input config: {e}"))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        // Clear the buffer
        {
            let mut buf = self.buffer.lock().unwrap();
            buf.samples.clear();
            buf.sample_rate = sample_rate;
        }

        let buffer = self.buffer.clone();
        let err_fn = |err: cpal::StreamError| {
            tracing::error!("Audio stream error: {err}");
        };

        // Build input stream — mix to mono, store as f32
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut buf) = buffer.lock() {
                        if channels == 1 {
                            buf.samples.extend_from_slice(data);
                        } else {
                            for chunk in data.chunks(channels) {
                                let avg: f32 = chunk.iter().sum::<f32>() / channels as f32;
                                buf.samples.push(avg);
                            }
                        }
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let buffer = self.buffer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if let Ok(mut buf) = buffer.lock() {
                            if channels == 1 {
                                buf.samples.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                            } else {
                                for chunk in data.chunks(channels) {
                                    let avg: f32 = chunk.iter().map(|&s| s as f32 / i16::MAX as f32).sum::<f32>() / channels as f32;
                                    buf.samples.push(avg);
                                }
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            fmt => return Err(format!("Unsupported sample format: {fmt:?}")),
        }
        .map_err(|e| format!("Failed to build stream: {e}"))?;

        stream.play().map_err(|e| format!("Failed to start mic: {e}"))?;

        *self.stream.lock().unwrap() = Some(stream);
        *self.recording.lock().unwrap() = true;

        tracing::info!("Audio recording started ({}Hz, {} ch → mono)", sample_rate, channels);
        Ok(())
    }

    /// Stop recording and return audio as WAV bytes.
    pub fn stop(&self) -> Vec<u8> {
        // Stop the stream
        *self.stream.lock().unwrap() = None;
        *self.recording.lock().unwrap() = false;

        // Drain the buffer
        let (samples, sample_rate) = {
            let mut buf = self.buffer.lock().unwrap();
            let samples = std::mem::take(&mut buf.samples);
            (samples, buf.sample_rate)
        };

        if samples.is_empty() {
            tracing::warn!("Audio recording: no samples captured");
            return Vec::new();
        }

        tracing::info!("Audio recording stopped: {} samples ({:.1}s at {}Hz)",
            samples.len(), samples.len() as f32 / sample_rate as f32, sample_rate);

        encode_wav(&samples, sample_rate)
    }

    pub fn is_recording(&self) -> bool {
        *self.recording.lock().unwrap()
    }
}

/// Encode f32 mono samples to 16-bit WAV bytes in memory.
fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = match hound::WavWriter::new(&mut cursor, spec) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to create WAV writer: {e}");
                return Vec::new();
            }
        };
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let _ = writer.write_sample((clamped * i16::MAX as f32) as i16);
        }
        if let Err(e) = writer.finalize() {
            tracing::error!("Failed to finalize WAV: {e}");
            return Vec::new();
        }
    }

    cursor.into_inner()
}
