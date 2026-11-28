import type { VoicePhase, VoiceStatus } from "@loki/contracts";
import { useCallback, useEffect, useRef, useState } from "react";

import { pressTalk, releaseTalk, setVoiceMuted, watchVoice } from "../api";

/** The push-to-talk shortcut: hold Ctrl+Shift+Space while the Loki window is focused. */
export const TALK_SHORTCUT = "Ctrl+Shift+Space";

const PHASE_TEXT: Record<VoicePhase, string> = {
  idle: "Voice idle",
  listening: "Listening for the wake word",
  recording: "Recording",
  transcribing: "Transcribing",
  thinking: "Thinking",
  speaking: "Speaking",
  stopping: "Stopping",
  muted: "Voice muted",
};

const isTalkShortcut = (event: KeyboardEvent) =>
  event.code === "Space" && event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;

/**
 * The listening indicator. Shown whenever the runtime says the microphone may be open
 * (`mic_open`, from the voice state machine), never hidden by a section or a setting.
 */
export function MicIndicator({ status }: { status: VoiceStatus }) {
  if (!status.mic_open) return null;
  return (
    <span className="mic-indicator" role="status" aria-live="assertive">
      <span aria-hidden="true">●</span> Microphone on · {PHASE_TEXT[status.phase]}
    </span>
  );
}

/**
 * Voice controls in the header (P5-T08): the listening indicator, the state, push-to-talk (button
 * and `TALK_SHORTCUT`, hold to talk) and mute. Everything follows the runtime's status stream, so
 * a mute from the tray shows here too. Push-to-talk is disabled, with the reason, while the
 * runtime reports it unavailable (no speech-to-text yet).
 */
export function VoiceBar() {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const talking = useRef(false);

  useEffect(() => {
    let active = true;
    watchVoice((next) => {
      if (active) setStatus(next);
    }).then(
      (current) => {
        if (active) setStatus(current);
      },
      (failure: unknown) => {
        if (active) setMessage(String(failure));
      },
    );
    return () => {
      active = false;
    };
  }, []);

  const start = useCallback(async () => {
    if (talking.current) return;
    talking.current = true;
    setMessage(null);
    try {
      setStatus(await pressTalk());
    } catch (failure) {
      talking.current = false;
      setMessage(String(failure));
    }
  }, []);

  const stop = useCallback(async () => {
    if (!talking.current) return;
    talking.current = false;
    try {
      setStatus(await releaseTalk());
    } catch (failure) {
      setMessage(String(failure));
    }
  }, []);

  // Hold-to-talk shortcut. Auto-repeat keydowns are ignored; releasing any part of it stops.
  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      if (!isTalkShortcut(event)) return;
      event.preventDefault();
      if (!event.repeat) void start();
    };
    const up = (event: KeyboardEvent) => {
      if (event.code === "Space" || event.key === "Control" || event.key === "Shift") void stop();
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    const blur = () => void stop();
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
  }, [start, stop]);

  if (status === null) {
    return message ? (
      <p className="error" role="alert">
        Voice unavailable: {message}
      </p>
    ) : null;
  }

  const ptt = status.push_to_talk;
  const pttReason = ptt.status === "supported" ? null : ptt.reason;
  const talkDisabled = pttReason !== null || status.muted;

  return (
    <div className="voice-bar" role="group" aria-label="Voice">
      <MicIndicator status={status} />
      <span className="voice-phase">{PHASE_TEXT[status.phase]}</span>
      <button
        type="button"
        disabled={talkDisabled}
        aria-describedby="voice-hint"
        aria-keyshortcuts="Control+Shift+Space"
        onPointerDown={() => void start()}
        onPointerUp={() => void stop()}
        onPointerLeave={() => void stop()}
      >
        Hold to talk
      </button>
      <button
        type="button"
        aria-pressed={status.muted}
        onClick={() => void setVoiceMuted(!status.muted).then(setStatus)}
      >
        {status.muted ? "Unmute" : "Mute"}
      </button>
      <span id="voice-hint" className="voice-hint">
        {pttReason ?? (status.muted ? "Unmute to talk." : `Hold the button or ${TALK_SHORTCUT}.`)}
      </span>
      {message && (
        <span className="voice-message" role="status">
          {message}
        </span>
      )}
    </div>
  );
}
