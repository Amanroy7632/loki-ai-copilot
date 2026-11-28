//! Synthetic audio generated in-test exercises the real local classifier; MockDetector covers
//! precise timing and error behavior independently of model quality.
//!
//! The audio is computed, never recorded: no microphone data, third-party voices or binary
//! fixture files (which `.gitignore` excludes as `*.wav` anyway). Samples are quantized to PCM16,
//! like real capture input.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::f64::consts::PI;

use loki_voice::vad::{
    EarshotVad, SpeechDetector, UtteranceDetector, UtteranceEnd, VAD_FRAME_SAMPLES, VadConfig,
    VadError,
};

const RATE: usize = 16_000;

/// Round-trips through PCM16, as a WAV/capture source would.
fn pcm16(samples: Vec<f64>) -> Vec<f32> {
    samples
        .into_iter()
        .map(|x| {
            let quantized = (x.clamp(-1.0, 1.0) * 32767.0).round_ties_even() as i16;
            f32::from(quantized) / 32768.0
        })
        .collect()
}

/// A voiced-vowel test signal, not intelligible speech: a harmonic glottal source shaped by
/// three Gaussian formant peaks per vowel (approximate /a/, /i/, /u/, /a/), with syllabic
/// envelopes, 250 ms of leading silence, short gaps, then 2 s of trailing silence.
fn vowels_then_silence() -> Vec<f32> {
    const FORMANTS: [[f64; 3]; 4] = [
        [700.0, 1100.0, 2450.0],
        [270.0, 2290.0, 3010.0],
        [300.0, 870.0, 2240.0],
        [700.0, 1100.0, 2450.0],
    ];
    const WIDTHS: [f64; 3] = [90.0, 120.0, 160.0];
    const SYLLABLE: usize = 6400;
    let mut signal = vec![0.0; RATE / 4];
    let mut phase = 0.0_f64;
    for formants in FORMANTS {
        let mut syllable = Vec::with_capacity(SYLLABLE);
        for i in 0..SYLLABLE {
            let t = i as f64 / RATE as f64;
            let fundamental = 125.0 + 15.0 * (2.0 * PI * 3.0 * t).sin();
            phase += 2.0 * PI * fundamental / RATE as f64;
            let mut sample = 0.0;
            for h in 1..=40 {
                let frequency = f64::from(h) * fundamental;
                let spectral: f64 = formants
                    .iter()
                    .zip(WIDTHS)
                    .map(|(f, width)| (-0.5 * ((frequency - f) / width).powi(2)).exp())
                    .sum();
                sample += (0.02 + spectral) * (f64::from(h) * phase).sin() / f64::from(h);
            }
            syllable.push(sample * (PI * i as f64 / SYLLABLE as f64).sin().sqrt());
        }
        let peak = syllable.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        signal.extend(syllable.iter().map(|x| x * 0.6 / peak));
        signal.extend([0.0; 800]);
    }
    signal.extend(vec![0.0; RATE * 2]);
    pcm16(signal)
}

/// 4 s of low-level white noise from a fixed integer LCG, so it is identical on every run and OS.
fn noise() -> Vec<f32> {
    let mut state: u32 = 42;
    let signal = (0..RATE * 4)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(state >> 8) / 16_777_215.0 * 2.0 - 1.0) * 0.08
        })
        .collect();
    pcm16(signal)
}

#[test]
fn synthetic_audio_is_deterministic_and_bounded() {
    assert_eq!(vowels_then_silence(), vowels_then_silence());
    assert_eq!(noise(), noise());
    assert!(noise().iter().all(|x| x.abs() <= 0.081));
    let vowels = vowels_then_silence();
    assert_eq!(vowels.len(), RATE / 4 + 4 * (6400 + 800) + 2 * RATE);
    assert!(vowels.iter().all(|x| x.abs() <= 0.61));
}

#[test]
fn synthetic_vowels_then_silence_end_the_recording() {
    let audio = vowels_then_silence();
    let mut vad = UtteranceDetector::new(VadConfig::default()).unwrap();
    let mut consumed = 0;
    let mut end = None;
    for chunk in audio.chunks(317) {
        let update = vad.push(chunk).unwrap();
        consumed += update.consumed;
        if update.end.is_some() {
            end = update.end;
            break;
        }
    }
    assert_eq!(end, Some(UtteranceEnd::Silence));
    assert!(consumed > 16_000 && consumed < audio.len());
}

/// Runs the default detector over `audio` in odd-sized chunks; returns the end and where it fell.
fn run(audio: &[f32]) -> (bool, Option<UtteranceEnd>, usize) {
    let mut vad = UtteranceDetector::new(VadConfig::default()).unwrap();
    let (mut detected, mut consumed) = (false, 0);
    for chunk in audio.chunks(317) {
        let update = vad.push(chunk).unwrap();
        consumed += update.consumed;
        detected |= update.speech_detected;
        if update.end.is_some() {
            return (detected, update.end, consumed);
        }
    }
    (detected, None, consumed)
}

/// A continuous room-noise bed of `seconds`, as a real microphone always delivers (never exact
/// digital zeros), at peak amplitude `level`.
fn noise_bed(seconds: usize, level: f32) -> Vec<f32> {
    let source = noise();
    (0..seconds * RATE)
        .map(|i| source[i % source.len()] / 0.08 * level)
        .collect()
}

/// Noise alone never counts as speech, from stream start (the classifier's onset transient
/// reaches 352 ms, under the 400 ms confirmation) and at several room-noise levels.
#[test]
fn synthetic_noise_does_not_trigger_speech_or_silence_timeout() {
    for level in [0.005, 0.01, 0.02, 0.04, 0.08] {
        let (detected, end, _) = run(&noise_bed(6, level));
        assert!(!detected, "noise at {level} counted as speech");
        assert_eq!(end, None, "noise at {level}");
    }
}

/// Speech over a continuous noise bed still ends ~1.2 s after the speech stops.
#[test]
fn synthetic_vowels_over_room_noise_end_after_the_silence_timeout() {
    let vowels = vowels_then_silence();
    let speech = &vowels[RATE / 4..RATE / 4 + 4 * 7200];
    let speech_end = RATE + speech.len();
    for level in [0.005, 0.01, 0.02, 0.04] {
        let mut audio = noise_bed(6, level);
        for (i, sample) in speech.iter().enumerate() {
            audio[RATE + i] += sample;
        }
        let (detected, end, at) = run(&audio);
        assert!(detected, "speech at noise {level} not confirmed");
        assert_eq!(end, Some(UtteranceEnd::Silence), "noise {level}");
        let after_ms = (at - speech_end) * 1000 / RATE;
        assert!(
            (1000..=1600).contains(&after_ms),
            "noise {level}: ended {after_ms} ms after speech"
        );
    }
}

struct MockDetector;
impl SpeechDetector for MockDetector {
    fn reset(&mut self) {}
    fn score(&mut self, frame: &[f32; VAD_FRAME_SAMPLES]) -> Result<f32, VadError> {
        Ok(frame[0])
    }
}

fn mock_vad(config: VadConfig) -> UtteranceDetector<MockDetector> {
    UtteranceDetector::with_detector(MockDetector, config).unwrap()
}

#[test]
fn mock_silence_before_speech_and_short_spikes_do_not_start_recording() {
    let mut vad = mock_vad(VadConfig::default());
    assert!(!vad.push(&[0.0; 32_000]).unwrap().speech_detected);
    for _ in 0..10 {
        assert!(!vad.push(&[1.0; 256]).unwrap().speech_detected);
        assert!(!vad.push(&[0.0; 256]).unwrap().speech_detected);
    }
}

#[test]
fn mock_default_silence_timeout_is_exactly_1200ms_after_confirmed_speech() {
    let mut vad = mock_vad(VadConfig::default());
    // 400 ms of speech confirms it (one frame short does not).
    assert!(!vad.push(&[1.0; 6144]).unwrap().speech_detected);
    assert!(vad.push(&[1.0; 256]).unwrap().speech_detected);
    assert_eq!(vad.push(&[0.0; 19_199]).unwrap().end, None);
    assert_eq!(vad.push(&[0.0]).unwrap().end, Some(UtteranceEnd::Silence));
    assert_eq!(vad.push(&[0.0]), Err(VadError::Finished));
}

#[test]
fn mock_new_speech_resets_the_silence_timer() {
    let mut vad = mock_vad(VadConfig::default());
    vad.push(&[1.0; 6400]).unwrap();
    assert_eq!(vad.push(&[0.0; 18_944]).unwrap().end, None);
    assert_eq!(vad.push(&[1.0; 256]).unwrap().end, None);
    assert_eq!(
        vad.push(&[0.0; 19_200]).unwrap().end,
        Some(UtteranceEnd::Silence)
    );
}

#[test]
fn mock_maximum_duration_stops_both_continuous_speech_and_no_speech() {
    let config = VadConfig {
        max_utterance_ms: 2000,
        ..VadConfig::default()
    };
    for (value, expected) in [
        (0.0, UtteranceEnd::NoSpeech),
        (1.0, UtteranceEnd::MaxDuration),
    ] {
        let mut vad = mock_vad(config);
        let update = vad.push(&vec![value; 40_000]).unwrap();
        assert_eq!(update.end, Some(expected));
        assert_eq!(update.consumed, 32_000);
    }
}

#[test]
fn mock_reset_discards_partial_frames_and_previous_speech() {
    let mut vad = mock_vad(VadConfig::default());
    vad.push(&[1.0; 1300]).unwrap();
    vad.reset();
    let update = vad.push(&[0.0; 19_200]).unwrap();
    assert!(!update.speech_detected);
    assert_eq!(update.end, None);
}

#[test]
fn mock_invalid_input_or_scores_latch_failure_until_reset() {
    for (value, error) in [
        (f32::NAN, VadError::InvalidSamples),
        (f32::INFINITY, VadError::InvalidSamples),
        (-0.1, VadError::InvalidScore),
        (1.1, VadError::InvalidScore),
    ] {
        let mut vad = mock_vad(VadConfig::default());
        assert_eq!(vad.push(&[value; 256]), Err(error));
        assert_eq!(vad.push(&[0.0; 256]), Err(error));
        vad.reset();
        assert!(vad.push(&[0.0; 256]).is_ok());
    }
}

#[test]
fn invalid_settings_are_rejected() {
    for config in [
        VadConfig {
            threshold: f32::NAN,
            ..VadConfig::default()
        },
        VadConfig {
            threshold: 0.0,
            ..VadConfig::default()
        },
        VadConfig {
            threshold: 1.0,
            ..VadConfig::default()
        },
        VadConfig {
            silence_ms: 0,
            ..VadConfig::default()
        },
        VadConfig {
            min_speech_ms: 0,
            ..VadConfig::default()
        },
        VadConfig {
            max_utterance_ms: 300_001,
            ..VadConfig::default()
        },
        VadConfig {
            max_utterance_ms: 1200,
            ..VadConfig::default()
        },
    ] {
        assert!(matches!(
            UtteranceDetector::new(config),
            Err(VadError::InvalidConfig)
        ));
    }
}

#[test]
fn earshot_clamps_resampling_overshoot_and_rejects_nan() {
    let mut detector = EarshotVad::default();
    let score = detector.score(&[1.1; 256]).unwrap();
    assert!(score.is_finite() && (0.0..=1.0).contains(&score));
    assert_eq!(
        detector.score(&[f32::NAN; 256]),
        Err(VadError::InvalidSamples)
    );
}
