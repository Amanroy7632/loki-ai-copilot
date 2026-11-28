//! macOS microphone permission (TCC) through `AVAudioApplication` (macOS 14+).
//!
//! Loki never works around TCC: it reads the status macOS reports and, only when the user turns
//! the microphone on in Loki, asks macOS to show its own permission prompt. A "deny" (now or
//! earlier) can only be changed by the user in System Settings, which Loki can open for them.
//!
//! The app must declare `NSMicrophoneUsageDescription` in its Info.plist
//! (`apps/desktop/src-tauri/Info.plist`); macOS ends a process that touches the microphone without
//! it. In `tauri dev`, macOS attributes the request to the app that launched Loki (e.g. Terminal).
#![allow(unsafe_code)]

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_avf_audio::{AVAudioApplication, AVAudioApplicationRecordPermission};

/// What macOS says about microphone access for this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicPermission {
    /// The user hasn't been asked yet.
    NotAsked,
    /// Allowed.
    Granted,
    /// Refused (or blocked by policy); only System Settings can change it.
    Denied,
}

/// Reads the current microphone permission. Never prompts.
pub fn status() -> MicPermission {
    // SAFETY: `sharedInstance` returns a retained singleton and `recordPermission` is a plain
    // property read with no arguments; both are documented as safe to call from any thread.
    let permission = unsafe { AVAudioApplication::sharedInstance().recordPermission() };
    map(permission)
}

fn map(permission: AVAudioApplicationRecordPermission) -> MicPermission {
    if permission == AVAudioApplicationRecordPermission::Granted {
        MicPermission::Granted
    } else if permission == AVAudioApplicationRecordPermission::Denied {
        MicPermission::Denied
    } else {
        MicPermission::NotAsked
    }
}

/// Asks macOS for microphone access, showing the system prompt if the user was never asked, and
/// waits up to `wait` for the answer. Already-answered requests return at once. Blocking: call
/// it off the main thread (the prompt itself runs on macOS's own UI).
pub fn request(wait: Duration) -> MicPermission {
    if status() != MicPermission::NotAsked {
        return status();
    }
    let (sender, receiver) = mpsc::channel::<bool>();
    let block = RcBlock::new(move |granted: Bool| {
        let _ = sender.send(granted.as_bool());
    });
    // SAFETY: the class method takes a completion block that macOS calls exactly once on an
    // arbitrary queue. The block only sends on a channel (Send), and `RcBlock` keeps it alive for
    // the duration of the call; macOS copies it before returning.
    unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
    match receiver.recv_timeout(wait) {
        Ok(true) => MicPermission::Granted,
        Ok(false) => MicPermission::Denied,
        // No answer in time: report what macOS says now (usually still "not asked").
        Err(_) => status(),
    }
}
