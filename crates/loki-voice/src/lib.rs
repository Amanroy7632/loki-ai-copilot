//! # loki-voice
//!
//! Audio pipeline: capture, resampling, VAD, speech-to-text, TTS and the voice state machine.
//!
//! **Status:** P4-T01 implements the pure state machine; P4-T02 adds explicit CPAL capture and
//! 16 kHz mono conversion. Recognition, speech output and runtime integration remain later
//! tickets. Capture starts only through `CaptureSession::start`, never on initialization.
//!
//! ## Responsibilities
//!
//! - Implement the pure voice state machine (Idle, Listening, Recording, Transcribing, Thinking, Speaking, Cancelled, Muted).
//! - Capture mono PCM over a bounded channel and resample it to the STT model's input format.
//! - Run voice activity detection and the silence timeout.
//! - Run speech-to-text (whisper.cpp bindings) off the async runtime.
//! - Provide the TTS trait, with native implementations supplied by `loki-platform`.
//! - Optionally provide a wake-word engine behind a cargo feature, off by default.
//!
//! ## Must not
//!
//! - Keep raw audio on disk (it is disabled by default and needs an explicit setting).
//! - Listen while muted, or without a visible indicator.
//! - Treat speech as approval for sensitive actions.
//!
//! ## Allowed internal dependencies
//!
//! `loki-core`. Adding another edge needs an ADR (see `docs/ARCHITECTURE.md`).

mod state;

pub mod capture;
pub mod vad;

pub use state::{InvalidTransition, VoiceEvent, VoiceMachine, VoiceState};
