//! Preallocated lock-free handoff. The producer never waits for the consumer or allocates.

use super::{CaptureError, CaptureStats};
use crossbeam_queue::ArrayQueue;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

pub(super) struct Shared {
    pub(super) frames: ArrayQueue<f32>,
    active: AtomicBool,
    error: AtomicU8,
    captured: AtomicU64,
    dropped: AtomicU64,
}

impl Shared {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            frames: ArrayQueue::new(capacity),
            active: AtomicBool::new(true),
            error: AtomicU8::new(0),
            captured: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    pub(super) fn close(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub(super) fn stats(&self) -> CaptureStats {
        CaptureStats {
            captured_frames: self.captured.load(Ordering::Relaxed),
            dropped_frames: self.dropped.load(Ordering::Relaxed),
            queued_frames: self.frames.len(),
        }
    }

    pub(super) fn error(&self) -> Option<CaptureError> {
        match self.error.load(Ordering::Acquire) {
            0 => None,
            1 => Some(CaptureError::PermissionDenied),
            2 => Some(CaptureError::DeviceLost),
            3 => Some(CaptureError::InvalidSamples),
            _ => Some(CaptureError::Backend),
        }
    }
}

/// Callback endpoint supplied by `CaptureSession`. Clone for the backend's error callback.
/// Dropped frames are counted; the newest audio always replaces the oldest on overflow.
#[derive(Clone)]
pub struct CaptureSink {
    shared: Arc<Shared>,
    channels: usize,
}

impl CaptureSink {
    pub(super) fn new(shared: Arc<Shared>, channels: u16) -> Self {
        Self {
            shared,
            channels: usize::from(channels),
        }
    }

    /// Converts normalized samples to mono without allocation, locks, logging or file IO.
    /// Complete interleaved frames are required. Non-finite samples terminate capture.
    pub fn push_interleaved<T>(&self, samples: &[T])
    where
        T: cpal::Sample,
        f32: cpal::FromSample<T>,
    {
        if !self.shared.active.load(Ordering::Acquire) {
            return;
        }
        if !samples.len().is_multiple_of(self.channels) {
            self.fail(CaptureError::InvalidSamples);
            return;
        }
        for frame in samples.chunks_exact(self.channels) {
            if !self.shared.active.load(Ordering::Acquire) {
                return;
            }
            let mut sum = 0.0;
            for sample in frame {
                let value = sample.to_sample::<f32>();
                if !value.is_finite() {
                    self.fail(CaptureError::InvalidSamples);
                    return;
                }
                sum += value.clamp(-1.0, 1.0);
            }
            let mono = sum / self.channels as f32;
            if self.shared.frames.force_push(mono).is_some() {
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            }
            self.shared.captured.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Latches the first callback failure and rejects later samples. The consumer observes the
    /// typed error and releases the stream; this callback must never try to drop its own stream.
    pub fn fail(&self, error: CaptureError) {
        let code = match error {
            CaptureError::PermissionDenied => 1,
            CaptureError::DeviceLost => 2,
            CaptureError::InvalidSamples => 3,
            _ => 4,
        };
        let _ = self
            .shared
            .error
            .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
        self.shared.close();
    }
}
