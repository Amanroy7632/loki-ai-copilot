// Pure fold of a turn's TurnEvent stream into what the chat view shows. No IPC, no side effects.
import type { ApprovalPreview, Conversation, Message, ToolStatus, TurnEvent } from "@loki/contracts";

export type TurnPhase = "streaming" | "completed" | "stopped" | "failed";

export interface ToolActivity {
  tool: string;
  status: ToolStatus;
  detail: string | null;
}

export interface TurnState {
  phase: TurnPhase;
  /** Set by `started` for a new conversation, or passed in for an existing one. */
  conversation: Conversation | null;
  /** Reply text streamed so far. */
  text: string;
  /** The approval the user must answer now, if any. */
  pendingApproval: ApprovalPreview | null;
  tools: ToolActivity[];
  /** The stored reply once the turn ends (partial when stopped). */
  message: Message | null;
  error: string | null;
}

export function initialTurn(conversation: Conversation | null = null): TurnState {
  return {
    phase: "streaming",
    conversation,
    text: "",
    pendingApproval: null,
    tools: [],
    message: null,
    error: null,
  };
}

export function applyTurnEvent(state: TurnState, event: TurnEvent): TurnState {
  switch (event.type) {
    case "started":
      return { ...state, conversation: event.conversation };
    case "token":
      return { ...state, text: state.text + event.text };
    case "approval_requested":
      return { ...state, pendingApproval: event.approval };
    case "tool_finished":
      return {
        ...state,
        pendingApproval: state.pendingApproval?.tool === event.tool ? null : state.pendingApproval,
        tools: [...state.tools, { tool: event.tool, status: event.status, detail: event.detail }],
      };
    case "completed":
      return { ...state, phase: "completed", pendingApproval: null, message: event.message };
    case "stopped":
      return { ...state, phase: "stopped", pendingApproval: null, message: event.message };
    case "failed":
      return { ...state, phase: "failed", pendingApproval: null, error: event.error };
  }
}
