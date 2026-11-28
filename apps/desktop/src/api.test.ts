// P5-T02 acceptance: with the mock provider's event stream, the UI side receives a streamed reply
// through a real Tauri Channel (IPC mocked, message framing as Tauri sends it).
import type { Conversation, Message, TurnEvent } from "@loki/contracts";
import type { Channel } from "@tauri-apps/api/core";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";

import { respondApproval, sendMessage, stopReply } from "./api";
import { applyTurnEvent, initialTurn, type TurnState } from "./turn";

const conversation: Conversation = { id: "c1", title: "hello", created_at: 1, updated_at: 1 };
const reply: Message = {
  id: "m2",
  conversation_id: "c1",
  role: "assistant",
  content: "Mock reply: hello",
  created_at: 2,
  stopped: false,
};

/** What the Rust MockProvider streams for "hello" (see loki-runtime tests/streaming.rs). */
const mockProviderTurn: TurnEvent[] = [
  { type: "started", conversation },
  { type: "token", text: "Mock " },
  { type: "token", text: "reply: " },
  { type: "token", text: "hello" },
  { type: "completed", message: reply },
];

/** The test-only hook `mockIPC` installs; delivering through it exercises the real Channel class. */
interface TauriInternals {
  runCallback(id: number, frame: { index: number; message: TurnEvent }): void;
}
function internals(): TauriInternals {
  return (window as unknown as { __TAURI_INTERNALS__: TauriInternals }).__TAURI_INTERNALS__;
}

/** Delivers events the way Tauri's Rust side does: `{ index, message }` per event. */
function emit(channel: Channel<TurnEvent>, events: TurnEvent[]) {
  events.forEach((message, index) => {
    internals().runCallback(channel.id, { index, message });
  });
}

describe("sendMessage", () => {
  it("receives a streamed reply from the mock provider", async () => {
    const calls: { cmd: string; args: Record<string, unknown> }[] = [];
    mockIPC((cmd, args) => {
      const payload = args as Record<string, unknown>;
      calls.push({ cmd, args: payload });
      if (cmd === "send_message") {
        emit(payload.onEvent as Channel<TurnEvent>, mockProviderTurn);
      }
      return null;
    });

    let state: TurnState = initialTurn();
    const seen: string[] = [];
    await sendMessage("turn-1", null, "hello", (event) => {
      seen.push(state.text);
      state = applyTurnEvent(state, event);
    });

    expect(state.phase).toBe("completed");
    expect(state.text).toBe("Mock reply: hello");
    expect(state.conversation?.id).toBe("c1");
    expect(state.message?.content).toBe("Mock reply: hello");
    expect(seen).toEqual(["", "", "Mock ", "Mock reply: ", "Mock reply: hello"]);
    expect(calls[0]?.cmd).toBe("send_message");
    expect(calls[0]?.args).toMatchObject({ turnId: "turn-1", conversationId: null, text: "hello" });
  });

  it("delivers out-of-order frames in order", async () => {
    mockIPC((cmd, args) => {
      if (cmd !== "send_message") return null;
      const channel = (args as { onEvent: Channel<TurnEvent> }).onEvent;
      const frames = mockProviderTurn.map((message, index) => ({ index, message }));
      for (const i of [0, 2, 1, 4, 3]) {
        const frame = frames[i];
        if (frame) internals().runCallback(channel.id, frame);
      }
      return null;
    });
    let state = initialTurn();
    await sendMessage("turn-2", null, "hello", (event) => {
      state = applyTurnEvent(state, event);
    });
    expect(state.text).toBe("Mock reply: hello");
    expect(state.phase).toBe("completed");
  });

  it("passes stop and approval answers through to the runtime", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return true;
    });
    expect(await stopReply("turn-3")).toBe(true);
    expect(await respondApproval("ap-1", false)).toBe(true);
    expect(calls).toEqual([
      ["stop_reply", { turnId: "turn-3" }],
      ["respond_approval", { approvalId: "ap-1", approved: false }],
    ]);
  });
});

describe("applyTurnEvent", () => {
  it("tracks approvals, tool results, stop and failure", () => {
    let state = initialTurn(conversation);
    state = applyTurnEvent(state, {
      type: "approval_requested",
      approval: {
        id: "ap",
        request_id: "rq",
        tool: "app.open",
        risk: "local_change",
        arguments: { app_id: "com.apple.calculator" },
        expires_at: 10,
      },
    });
    expect(state.pendingApproval?.tool).toBe("app.open");
    state = applyTurnEvent(state, {
      type: "tool_finished",
      tool: "app.open",
      status: "cancelled",
      detail: "no",
    });
    expect(state.pendingApproval).toBeNull();
    expect(state.tools).toEqual([{ tool: "app.open", status: "cancelled", detail: "no" }]);
    expect(applyTurnEvent(state, { type: "stopped", message: null }).phase).toBe("stopped");
    const failed = applyTurnEvent(state, { type: "failed", error: "Ollama is not running" });
    expect(failed.phase).toBe("failed");
    expect(failed.error).toBe("Ollama is not running");
  });
});
