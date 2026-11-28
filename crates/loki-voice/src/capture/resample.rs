//! Worker-side continuous FFT resampling with startup-delay removal and tail flushing.

use super::{CaptureError, STT_SAMPLE_RATE};
use rubato::{Fft, FixedSync, Resampler, audioadapter_buffers::direct::InterleavedSlice};

pub(super) struct MonoResampler {
    inner: Option<Fft<f32>>,
    pending: Vec<f32>,
    chunk: usize,
    rate: u32,
    trim: usize,
    input_frames: u64,
    output_frames: u64,
}

impl MonoResampler {
    pub(super) fn new(rate: u32, chunk: usize) -> Result<Self, CaptureError> {
        let inner = if rate == STT_SAMPLE_RATE {
            None
        } else {
            Some(
                Fft::new(
                    rate as usize,
                    STT_SAMPLE_RATE as usize,
                    chunk,
                    1,
                    FixedSync::Input,
                )
                .map_err(|_| CaptureError::Resampling)?,
            )
        };
        let trim = inner.as_ref().map_or(0, Resampler::output_delay);
        Ok(Self {
            inner,
            pending: Vec::with_capacity(chunk),
            chunk,
            rate,
            trim,
            input_frames: 0,
            output_frames: 0,
        })
    }

    pub(super) fn push(&mut self, mut samples: &[f32]) -> Result<Vec<f32>, CaptureError> {
        if self.inner.is_none() {
            return Ok(samples.to_vec());
        }
        let mut output = Vec::new();
        while !samples.is_empty() {
            let count = (self.chunk - self.pending.len()).min(samples.len());
            self.pending.extend_from_slice(&samples[..count]);
            self.input_frames += count as u64;
            samples = &samples[count..];
            if self.pending.len() == self.chunk {
                self.process(&mut output)?;
            }
        }
        Ok(output)
    }

    fn target_frames(&self) -> u64 {
        (self.input_frames * u64::from(STT_SAMPLE_RATE)).div_ceil(u64::from(self.rate))
    }

    fn process(&mut self, output: &mut Vec<f32>) -> Result<(), CaptureError> {
        self.pending.resize(self.chunk, 0.0);
        let input = InterleavedSlice::new(&self.pending, 1, self.chunk)
            .map_err(|_| CaptureError::Resampling)?;
        let inner = self.inner.as_mut().ok_or(CaptureError::Resampling)?;
        let converted = inner
            .process(&input, None)
            .map_err(|_| CaptureError::Resampling)?
            .take_data();
        let skip = self.trim.min(converted.len());
        self.trim -= skip;
        let available = converted.len() - skip;
        let take = available.min((self.target_frames() - self.output_frames) as usize);
        output.extend_from_slice(&converted[skip..skip + take]);
        self.output_frames += take as u64;
        self.pending.clear();
        Ok(())
    }

    pub(super) fn finish(&mut self) -> Result<Vec<f32>, CaptureError> {
        let mut output = Vec::new();
        if self.inner.is_some() {
            while self.output_frames < self.target_frames() {
                self.process(&mut output)?;
            }
        }
        Ok(output)
    }
}
