//! Explicit, in-memory microphone capture. The callback only downmixes and enqueues; the
//! caller drains and resamples on a worker thread. Dropping a session releases its device.

mod native;
mod queue;
mod resample;

pub use native::CpalSource;
pub use queue::CaptureSink;

use std::sync::Arc;

use queue::Shared;
use resample::MonoResampler;

/// Recognition input rate, independent of the hardware's native rate.
pub const STT_SAMPLE_RATE: u32 = 16_000;

/// Typed capture failures; no device names or captured samples appear in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// There is no default microphone.
    #[error("no default input device is available")]
    NoInputDevice,
    /// The OS denied access. The caller must explain the OS permission flow, never bypass it.
    #[error("microphone access denied; allow it in the operating system's privacy settings")]
    PermissionDenied,
    /// A disconnected or invalidated device requires a new explicit session.
    #[error("audio input device was lost or its stream was invalidated")]
    DeviceLost,
    /// Native sample rate, channel count or sample representation is unsupported.
    #[error("unsupported audio input format")]
    UnsupportedFormat,
    /// Buffer limits are invalid.
    #[error("invalid capture buffer configuration")]
    InvalidConfig,
    /// The producer supplied malformed or non-finite audio samples.
    #[error("invalid input audio frames")]
    InvalidSamples,
    /// The backend reported a failure not covered by a more precise category.
    #[error("audio backend failed")]
    Backend,
    /// The sample-rate converter could not process its input.
    #[error("audio resampling failed")]
    Resampling,
}

/// Native interleaved PCM format. A frame contains one sample per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFormat {
    /// Frames per second before conversion to 16 kHz.
    pub sample_rate: u32,
    /// Channels averaged to mono before enqueueing.
    pub channels: u16,
}

impl InputFormat {
    fn validate(self) -> Result<(), CaptureError> {
        // Bound FFT sizes and buffer allocation; unusual hardware rates are reported honestly.
        if !(1..=32).contains(&self.channels)
            || ![
                8_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400,
                192_000,
            ]
            .contains(&self.sample_rate)
        {
            return Err(CaptureError::UnsupportedFormat);
        }
        Ok(())
    }
}

/// Bounded memory/latency settings. No audio is written to disk.
#[derive(Debug, Clone, Copy)]
pub struct CaptureConfig {
    /// Native frames retained as milliseconds of audio (1..=2000; default 250).
    pub buffer_ms: u32,
    /// Input frames per resampler chunk (1..=8192; default 1024).
    pub chunk_frames: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            buffer_ms: 250,
            chunk_frames: 1024,
        }
    }
}

impl CaptureConfig {
    fn capacity(self, format: InputFormat) -> Result<usize, CaptureError> {
        format.validate()?;
        if !(1..=2000).contains(&self.buffer_ms) || !(1..=8192).contains(&self.chunk_frames) {
            return Err(CaptureError::InvalidConfig);
        }
        Ok((format.sample_rate as usize * self.buffer_ms as usize).div_ceil(1000))
    }
}

/// Replaceable source for hardware-independent tests and future platform capture adapters.
pub trait AudioSource {
    /// Owning guard: dropping it must stop and release the stream. No background producer may
    /// outlive that guard. The sink also rejects callbacks after session closure.
    type Stream;

    /// The format must stay fixed for the session; route changes must be reported as errors.
    fn format(&self) -> InputFormat;

    /// Starts capture only when explicitly called. On failure, release any partial stream.
    fn start(self, sink: CaptureSink) -> Result<Self::Stream, CaptureError>;
}

/// Counters in native mono frames (not scalar samples across all channels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureStats {
    /// Valid frames accepted from the source, including subsequently overwritten frames.
    pub captured_frames: u64,
    /// Oldest frames overwritten because the consumer fell behind.
    pub dropped_frames: u64,
    /// Current occupancy; observational while capture is active.
    pub queued_frames: usize,
}

/// Remaining 16 kHz mono samples and final counters after the device has been released.
#[derive(Debug)]
pub struct CaptureTail {
    /// Tail only: concatenate with samples already returned by `read`.
    pub samples: Vec<f32>,
    /// Counters after the producer stopped.
    pub stats: CaptureStats,
}

/// An explicit recording session. Own it and call `read` on a blocking worker, not a UI/audio
/// callback or async executor thread. There is no automatic start, retry, or file retention.
pub struct CaptureSession<S: AudioSource> {
    stream: Option<S::Stream>,
    shared: Arc<Shared>,
    resampler: MonoResampler,
    format: InputFormat,
}

impl<S: AudioSource> CaptureSession<S> {
    /// Allocates all callback storage and the converter before opening the input stream.
    /// The caller must show a listening indicator before calling this function.
    pub fn start(source: S, config: CaptureConfig) -> Result<Self, CaptureError> {
        let format = source.format();
        let capacity = config.capacity(format)?;
        let resampler = MonoResampler::new(format.sample_rate, config.chunk_frames)?;
        let shared = Arc::new(Shared::new(capacity));
        let sink = CaptureSink::new(Arc::clone(&shared), format.channels);
        let stream = match source.start(sink) {
            Ok(stream) => stream,
            Err(error) => {
                shared.close();
                return Err(error);
            }
        };
        Ok(Self {
            stream: Some(stream),
            shared,
            resampler,
            format,
        })
    }

    /// Native format selected at session creation.
    pub fn format(&self) -> InputFormat {
        self.format
    }

    /// Current overflow counters, without inspecting or logging audio.
    pub fn stats(&self) -> CaptureStats {
        self.shared.stats()
    }

    /// Drains at most one buffer's capacity and returns available 16 kHz mono audio. Empty is
    /// normal before a full resampler chunk arrives. Backend errors stop the stream immediately.
    pub fn read(&mut self) -> Result<Vec<f32>, CaptureError> {
        if let Some(error) = self.shared.error() {
            self.release();
            return Err(error);
        }
        let mut native = Vec::with_capacity(self.shared.frames.len());
        for _ in 0..self.shared.frames.capacity() {
            let Some(sample) = self.shared.frames.pop() else {
                break;
            };
            native.push(sample);
        }
        match self.resampler.push(&native) {
            Ok(samples) => Ok(samples),
            Err(error) => {
                self.release();
                Err(error)
            }
        }
    }

    /// Stops/relinquishes the device, drains queued audio and flushes the resampler tail.
    /// To cancel or mute, drop the session instead; that discards all pending audio.
    pub fn finish(mut self) -> Result<CaptureTail, CaptureError> {
        self.release();
        let mut samples = self.read()?;
        samples.extend(self.resampler.finish()?);
        Ok(CaptureTail {
            samples,
            stats: self.stats(),
        })
    }

    fn release(&mut self) {
        self.shared.close();
        drop(self.stream.take());
    }
}

impl<S: AudioSource> Drop for CaptureSession<S> {
    fn drop(&mut self) {
        self.release();
        while self.shared.frames.pop().is_some() {}
    }
}
