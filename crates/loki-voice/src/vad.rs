//! Local voice detection and bounded utterance timing over the capture pipeline's 16 kHz mono
//! samples. No audio is retained beyond one classifier frame; the runtime owns recording.

/// Earshot's frame size: 16 ms at 16 kHz.
pub const VAD_FRAME_SAMPLES: usize = 256;

/// Invalid settings/input or an attempt to reuse a finished utterance without reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VadError {
    /// Invalid or unbounded timing/threshold settings.
    #[error("invalid voice detection configuration")]
    InvalidConfig,
    /// Non-finite samples must never reach a classifier.
    #[error("voice detection received non-finite audio")]
    InvalidSamples,
    /// A classifier score was not finite or not in [0, 1].
    #[error("voice detector returned an invalid score")]
    InvalidScore,
    /// The utterance ended; create/reset the detector for a new explicit recording.
    #[error("voice detection has already ended for this utterance")]
    Finished,
}

/// Injectable classifier for timing tests. Input is one 16 kHz mono frame.
pub trait SpeechDetector {
    /// Clear recurrent state between unrelated recordings or after device changes.
    fn reset(&mut self);
    /// Probability of voice in [0, 1]. No network, IO, or user-authorization side effects.
    fn score(&mut self, frame: &[f32; VAD_FRAME_SAMPLES]) -> Result<f32, VadError>;
}

/// Earshot's embedded local predictor. No model download or native inference runtime needed.
#[derive(Default)]
pub struct EarshotVad {
    detector: earshot::Detector,
}

impl SpeechDetector for EarshotVad {
    fn reset(&mut self) {
        self.detector.reset();
    }

    fn score(&mut self, frame: &[f32; VAD_FRAME_SAMPLES]) -> Result<f32, VadError> {
        let mut normalized = [0.0; VAD_FRAME_SAMPLES];
        for (out, sample) in normalized.iter_mut().zip(frame) {
            if !sample.is_finite() {
                return Err(VadError::InvalidSamples);
            }
            // FFT resampling can slightly overshoot full-scale even when native PCM does not.
            *out = sample.clamp(-1.0, 1.0);
        }
        Ok(self.detector.predict_f32(&normalized))
    }
}

/// Timing is measured in received samples, never wall-clock scheduling delays. Decisions are
/// quantized to 16 ms frames, so a configured boundary can be exceeded by less than 16 ms.
#[derive(Debug, Clone, Copy)]
pub struct VadConfig {
    /// Voice probability threshold (0 < threshold < 1), default 0.5.
    pub threshold: f32,
    /// Consecutive speech needed to confirm an utterance, default 400 ms. Earshot scores the
    /// onset of broadband noise above 0.5 for up to 352 ms (measured, ADR-0013), so shorter
    /// windows let a fan or hiss count as speech. A very short reply ("yes") may then not confirm;
    /// push-to-talk release still ends the recording, and P4-T09 tunes this on real speech.
    pub min_speech_ms: u32,
    /// Continuous non-speech after confirmed speech ends recording; default 1200 ms.
    pub silence_ms: u32,
    /// Total recording cap including leading silence; default 30 seconds, maximum 5 minutes.
    pub max_utterance_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_ms: 400,
            silence_ms: 1200,
            max_utterance_ms: 30_000,
        }
    }
}

impl VadConfig {
    fn validate(self) -> Result<(), VadError> {
        if !self.threshold.is_finite()
            || self.threshold <= 0.0
            || self.threshold >= 1.0
            || !(16..=5000).contains(&self.min_speech_ms)
            || !(16..=10_000).contains(&self.silence_ms)
            || !(16..=300_000).contains(&self.max_utterance_ms)
            || self.min_speech_ms >= self.max_utterance_ms
            || self.silence_ms >= self.max_utterance_ms
        {
            return Err(VadError::InvalidConfig);
        }
        Ok(())
    }
}

/// Why the caller must stop recording. NoSpeech is not a transcript or an agent request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtteranceEnd {
    /// Speech was confirmed and then the silence timeout elapsed.
    Silence,
    /// Speech was confirmed but the total recording cap was reached.
    MaxDuration,
    /// Recording reached its cap without confirmed speech.
    NoSpeech,
}

/// Result of consuming a chunk. The runtime must stop capture on `end` and discard any
/// unconsumed suffix, which belongs after the recording boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VadUpdate {
    /// Samples consumed from this call; can be shorter than input when recording ends.
    pub consumed: usize,
    /// Whether enough consecutive speech has been observed in this utterance.
    pub speech_detected: bool,
    /// Recording boundary, if one was reached.
    pub end: Option<UtteranceEnd>,
}

/// One recording's classifier and timer. A fixed frame buffer tolerates arbitrary capture
/// chunk boundaries. Silence before speech never triggers `Silence`, and short spikes do not
/// qualify as speech. A terminal result is latched until explicit reset.
pub struct UtteranceDetector<D: SpeechDetector = EarshotVad> {
    detector: D,
    config: VadConfig,
    pending: [f32; VAD_FRAME_SAMPLES],
    pending_len: usize,
    elapsed_samples: u64,
    voiced_run: u64,
    silent_run: u64,
    speech_detected: bool,
    end: Option<UtteranceEnd>,
    error: Option<VadError>,
}

impl UtteranceDetector<EarshotVad> {
    /// Uses the local embedded VAD with validated timing settings.
    pub fn new(config: VadConfig) -> Result<Self, VadError> {
        Self::with_detector(EarshotVad::default(), config)
    }
}

impl<D: SpeechDetector> UtteranceDetector<D> {
    /// Injects a classifier; construction resets it to prevent cross-session recurrent state.
    pub fn with_detector(mut detector: D, config: VadConfig) -> Result<Self, VadError> {
        config.validate()?;
        detector.reset();
        Ok(Self {
            detector,
            config,
            pending: [0.0; VAD_FRAME_SAMPLES],
            pending_len: 0,
            elapsed_samples: 0,
            voiced_run: 0,
            silent_run: 0,
            speech_detected: false,
            end: None,
            error: None,
        })
    }

    /// Explicit new recording: discard partial input and reset the classifier and timers.
    pub fn reset(&mut self) {
        self.detector.reset();
        self.pending.fill(0.0);
        self.pending_len = 0;
        self.elapsed_samples = 0;
        self.voiced_run = 0;
        self.silent_run = 0;
        self.speech_detected = false;
        self.end = None;
        self.error = None;
    }

    /// Consume 16 kHz mono audio on a worker thread. Invalid input/classifier output fails
    /// closed and stays failed until reset. A partial final frame is not padded as speech.
    pub fn push(&mut self, samples: &[f32]) -> Result<VadUpdate, VadError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.end.is_some() {
            return Err(VadError::Finished);
        }
        let mut consumed = 0;
        for sample in samples {
            if !sample.is_finite() {
                self.error = Some(VadError::InvalidSamples);
                break;
            }
            self.pending[self.pending_len] = *sample;
            self.pending_len += 1;
            consumed += 1;
            if self.pending_len == VAD_FRAME_SAMPLES {
                self.pending_len = 0;
                match self.classify_frame() {
                    Ok(()) => {}
                    Err(error) => {
                        self.error = Some(error);
                        break;
                    }
                }
                if self.end.is_some() {
                    break;
                }
            }
        }
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(VadUpdate {
            consumed,
            speech_detected: self.speech_detected,
            end: self.end,
        })
    }

    fn classify_frame(&mut self) -> Result<(), VadError> {
        let score = self.detector.score(&self.pending)?;
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            return Err(VadError::InvalidScore);
        }
        self.elapsed_samples += VAD_FRAME_SAMPLES as u64;
        if score >= self.config.threshold {
            self.voiced_run += VAD_FRAME_SAMPLES as u64;
            self.silent_run = 0;
            if self.voiced_run >= u64::from(self.config.min_speech_ms) * 16 {
                self.speech_detected = true;
            }
        } else {
            self.voiced_run = 0;
            self.silent_run += VAD_FRAME_SAMPLES as u64;
        }
        if self.elapsed_samples >= u64::from(self.config.max_utterance_ms) * 16 {
            self.end = Some(if self.speech_detected {
                UtteranceEnd::MaxDuration
            } else {
                UtteranceEnd::NoSpeech
            });
        } else if self.speech_detected && self.silent_run >= u64::from(self.config.silence_ms) * 16
        {
            self.end = Some(UtteranceEnd::Silence);
        }
        Ok(())
    }
}
