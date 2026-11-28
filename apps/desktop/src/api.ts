// Typed wrappers around Loki's Tauri commands. Types come only from @loki/contracts (generated).
import type { AppStatus, Conversation, ConversationId, Message, TurnEvent } from "@loki/contracts";
import { Channel, invoke } from "@tauri-apps/api/core";

export function appStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("app_status");
}

export function listConversations(): Promise<Conversation[]> {
  return invoke<Conversation[]>("list_conversations");
}

export function getMessages(conversationId: ConversationId): Promise<Message[]> {
  return invoke<Message[]>("get_messages", { conversationId });
}

/** A fresh, unguessable turn ID, used to stop the turn later. */
export function newTurnId(): string {
  return crypto.randomUUID();
}

/**
 * Sends a message and streams the turn: `onEvent` receives every TurnEvent in order, ending with
 * exactly one `completed`, `stopped` or `failed`. `conversationId: null` starts a new conversation.
 */
export async function sendMessage(
  turnId: string,
  conversationId: ConversationId | null,
  text: string,
  onEvent: (event: TurnEvent) => void,
): Promise<void> {
  const channel = new Channel<TurnEvent>();
  channel.onmessage = onEvent;
  await invoke("send_message", { turnId, conversationId, text, onEvent: channel });
}

/** Regenerates the last reply of a conversation, streamed like `sendMessage`. */
export async function retryReply(
  turnId: string,
  conversationId: ConversationId,
  onEvent: (event: TurnEvent) => void,
): Promise<void> {
  const channel = new Channel<TurnEvent>();
  channel.onmessage = onEvent;
  await invoke("retry_reply", { turnId, conversationId, onEvent: channel });
}

export function stopReply(turnId: string): Promise<boolean> {
  return invoke<boolean>("stop_reply", { turnId });
}

export function respondApproval(approvalId: string, approved: boolean): Promise<boolean> {
  return invoke<boolean>("respond_approval", { approvalId, approved });
}
