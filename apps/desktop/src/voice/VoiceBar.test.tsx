// P5-T08 voice UI. Acceptance: the listening indicator is visible in every mic-open state, tested
// against the state machine: `fixtures/voice-states.json` is written by the Rust test
// `voice_indicator_fixture_matches_the_state_machine` from loki-voice's `VoiceState::ALL`.
import type { VoiceStatus } from "@loki/contracts";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import table from "../../../../fixtures/voice-states.json";
import { installFakeBackend } from "../test/fakeBackend";
import { MicIndicator, VoiceBar } from "./VoiceBar";

const STATES = table as unknown as { state: string; status: VoiceStatus }[];
const SUPPORTED: VoiceStatus["push_to_talk"] = { status: "supported" };

describe("listening indicator against the state machine", () => {
  it("covers every state machine state, including the mic-open ones", () => {
    expect(STATES.map((row) => row.state)).toEqual([
      "Idle",
      "Listening",
      "Recording",
      "Transcribing",
      "Thinking",
      "Speaking",
      "Cancelled",
      "Muted { cleanup_pending: false }",
      "Muted { cleanup_pending: true }",
    ]);
    expect(STATES.filter((row) => row.status.mic_open).map((row) => row.state)).toEqual([
      "Listening",
      "Recording",
    ]);
  });

  it.each(STATES)("$state: the indicator is shown exactly when the microphone may be open", ({ status }) => {
    const { unmount } = render(<MicIndicator status={status} />);
    if (status.mic_open) {
      expect(screen.getByRole("status")).toHaveTextContent(/Microphone on/);
    } else {
      expect(screen.queryByText(/Microphone on/)).not.toBeInTheDocument();
    }
    unmount();
  });

  it.each(STATES)(
    "$state: the voice bar shows the indicator whenever the runtime reports it",
    async ({ status }) => {
      const backend = installFakeBackend();
      backend.voice = status;
      render(<VoiceBar />);
      await screen.findByRole("group", { name: "Voice" });
      expect(screen.queryByText(/Microphone on/) !== null).toBe(status.mic_open);
    },
  );
});

describe("VoiceBar", () => {
  it("push-to-talk is disabled with the reason while speech-to-text is unavailable", async () => {
    const backend = installFakeBackend();
    render(<VoiceBar />);
    const talk = await screen.findByRole("button", { name: "Hold to talk" });
    expect(talk).toBeDisabled();
    expect(talk).toHaveAccessibleDescription(/needs local speech-to-text/);
    expect(screen.getByText("Voice idle")).toBeInTheDocument();

    // The shortcut explains why instead of doing nothing; the microphone stays off.
    fireEvent.keyDown(window, { code: "Space", key: " ", ctrlKey: true, shiftKey: true });
    expect(
      await screen.findByText(/needs local speech-to-text/, { selector: ".voice-message" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Microphone on/)).not.toBeInTheDocument();
    expect(backend.voice.mic_open).toBe(false);
  });

  it("holding the button or the shortcut records; releasing turns the indicator off", async () => {
    const backend = installFakeBackend();
    backend.voice = { ...backend.voice, push_to_talk: SUPPORTED };
    const user = userEvent.setup();
    render(<VoiceBar />);
    const talk = await screen.findByRole("button", { name: "Hold to talk" });
    expect(talk).toBeEnabled();

    await user.pointer({ keys: "[MouseLeft>]", target: talk });
    expect(await screen.findByText(/Microphone on · Recording/)).toBeInTheDocument();
    await user.pointer({ keys: "[/MouseLeft]", target: talk });
    await waitFor(() => expect(screen.queryByText(/Microphone on/)).not.toBeInTheDocument());

    backend.voice = { ...backend.voice, phase: "idle" };
    fireEvent.keyDown(window, { code: "Space", key: " ", ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(window, { code: "Space", key: " ", ctrlKey: true, shiftKey: true, repeat: true });
    expect(await screen.findByText(/Microphone on/)).toBeInTheDocument();
    fireEvent.keyUp(window, { code: "Space", key: " " });
    await waitFor(() => expect(screen.queryByText(/Microphone on/)).not.toBeInTheDocument());
    expect(backend.calls.filter((c) => c.cmd === "press_talk")).toHaveLength(2);
  });

  it("Mute turns voice off and blocks talking; Unmute never starts the microphone", async () => {
    const backend = installFakeBackend();
    backend.voice = { ...backend.voice, push_to_talk: SUPPORTED };
    const user = userEvent.setup();
    render(<VoiceBar />);
    await user.click(await screen.findByRole("button", { name: "Mute" }));
    const unmute = await screen.findByRole("button", { name: "Unmute" });
    expect(unmute).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Voice muted")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hold to talk" })).toBeDisabled();

    await user.click(unmute);
    expect(await screen.findByRole("button", { name: "Mute" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByText(/Microphone on/)).not.toBeInTheDocument();
  });

  it("follows changes made outside the window, such as Mute in the tray", async () => {
    const backend = installFakeBackend();
    render(<VoiceBar />);
    await screen.findByText("Voice idle");
    backend.setVoice({ ...backend.voice, phase: "muted", muted: true });
    expect(await screen.findByText("Voice muted")).toBeInTheDocument();
    backend.setVoice({ ...backend.voice, phase: "recording", muted: false, mic_open: true });
    expect(await screen.findByText(/Microphone on · Recording/)).toBeInTheDocument();
  });
});
