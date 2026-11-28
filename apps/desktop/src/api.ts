// Typed wrappers around Loki's Tauri commands. Types come only from @loki/contracts (generated).
import type {
  AllowedRootInfo,
  AutostartStatus,
  AuditEntry,
  AppStatus,
  Capability,
  Conversation,
  ConversationId,
  DataControlError,
  DesktopSettings,
  Grant,
  JsonValue,
  Message,
  PermissionRow,
  ProviderCheck,
  SettingsSnapshot,
  TurnEvent,
  VoiceStatus,
} from "@loki/contracts";
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

/**
 * Answers an approval with the arguments the dialog showed. The runtime refuses (returns `false`)
 * a "yes" that expired or doesn't match the pending arguments, and any repeated answer.
 */
export function respondApproval(
  approvalId: string,
  approved: boolean,
  shownArguments: JsonValue,
): Promise<boolean> {
  return invoke<boolean>("respond_approval", { approvalId, approved, shownArguments });
}

export function renameConversation(conversationId: ConversationId, title: string): Promise<Conversation> {
  return invoke<Conversation>("rename_conversation", { conversationId, title });
}

export async function deleteConversation(conversationId: ConversationId): Promise<void> {
  await invoke("delete_conversation", { conversationId });
}

function isTerminal(event: TurnEvent): boolean {
  return event.type === "completed" || event.type === "stopped" || event.type === "failed";
}

/**
 * Runs a streaming command and resolves once **both** the command has returned and the turn's
 * terminal event has arrived. Tauri orders channel messages among themselves, but not relative
 * to the command's own response, so waiting for just one of them could drop the last events.
 * If the command fails without a terminal event (e.g. invalid input), the promise rejects.
 */
export function streamTurn(
  start: (onEvent: (event: TurnEvent) => void) => Promise<void>,
  onEvent: (event: TurnEvent) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let terminal = false;
    let returned = false;
    const finishIfDone = () => {
      if (terminal && returned) resolve();
    };
    start((event) => {
      onEvent(event);
      if (isTerminal(event)) {
        terminal = true;
        finishIfDone();
      }
    }).then(
      () => {
        returned = true;
        finishIfDone();
      },
      (error: unknown) => {
        if (terminal) {
          returned = true;
          finishIfDone();
        } else {
          reject(error instanceof Error ? error : new Error(String(error)));
        }
      },
    );
  });
}

// ------------------------------------------------------------------ permissions (P5-T05)

export function listPermissions(): Promise<PermissionRow[]> {
  return invoke<PermissionRow[]>("list_permissions");
}

export function listRoots(): Promise<AllowedRootInfo[]> {
  return invoke<AllowedRootInfo[]>("list_roots");
}

/** `scope` is `*`, or an allowed-root ID for `file_read`. */
export function grantPermission(capability: Capability, scope: string): Promise<Grant> {
  return invoke<Grant>("grant_permission", { capability, scope });
}

/** Takes effect for the very next tool call. Resolves to how many grants were revoked. */
export function revokePermission(capability: Capability, scope: string): Promise<number> {
  return invoke<number>("revoke_permission", { capability, scope });
}

/** Opens the native folder picker; `null` when the user cancels. The path never comes from the page. */
export function addRoot(id: string): Promise<AllowedRootInfo | null> {
  return invoke<AllowedRootInfo | null>("add_root", { id });
}

/** Removes the folder and revokes its grants. */
export function removeRoot(id: string): Promise<boolean> {
  return invoke<boolean>("remove_root", { id });
}

export function getSettings(): Promise<SettingsSnapshot> {
  return invoke<SettingsSnapshot>("get_settings");
}

export function saveSettings(settings: DesktopSettings): Promise<SettingsSnapshot> {
  return dataMutation<SettingsSnapshot>("save_settings", { settings });
}

/** Native save dialog; false means cancelled. No path or file content comes from the page. */
export function exportData(): Promise<boolean> {
  return invoke<boolean>("export_data");
}

export function clearHistory(): Promise<number> {
  return dataMutation<number>("clear_history");
}

export function listAudit(): Promise<AuditEntry[]> {
  return invoke<AuditEntry[]>("list_audit");
}

/** Cleanup may fail after a successful write/delete; the UI must refresh in that case. */
export class DataControlFailure extends Error {
  readonly stateChanged: boolean;
  constructor(failure: DataControlError) {
    super(failure.message);
    this.stateChanged = failure.state_changed;
  }
}

async function dataMutation<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (failure) {
    if (
      typeof failure === "object" &&
      failure !== null &&
      "message" in failure &&
      typeof failure.message === "string" &&
      "state_changed" in failure &&
      typeof failure.state_changed === "boolean"
    ) {
      throw new DataControlFailure({ message: failure.message, state_changed: failure.state_changed });
    }
    throw failure;
  }
}

// ------------------------------------------------------------------ start at login (P5-T07)

/** The OS's actual registration; `enabled` is null when it can't be read (never shown as off). */
export function getAutostart(): Promise<AutostartStatus> {
  return invoke<AutostartStatus>("get_autostart");
}

/** Turns start at login on or off; resolves to the state the OS reports afterwards. */
export function setAutostart(enabled: boolean): Promise<AutostartStatus> {
  return invoke<AutostartStatus>("set_autostart", { enabled });
}

// ------------------------------------------------------------------ voice (P5-T08)

/** Current voice state: phase, whether the microphone may be open, mute, push-to-talk availability. */
export function getVoiceStatus(): Promise<VoiceStatus> {
  return invoke<VoiceStatus>("get_voice_status");
}

/**
 * Follows every voice change (including tray mute) until the window closes. Resolves to the
 * current status; `onStatus` also receives it first.
 */
export function watchVoice(onStatus: (status: VoiceStatus) => void): Promise<VoiceStatus> {
  const channel = new Channel<VoiceStatus>();
  channel.onmessage = onStatus;
  return invoke<VoiceStatus>("watch_voice", { onStatus: channel });
}

/** Rejects with the reason while muted or while push-to-talk is unavailable. */
export function pressTalk(): Promise<VoiceStatus> {
  return invoke<VoiceStatus>("press_talk");
}

export function releaseTalk(): Promise<VoiceStatus> {
  return invoke<VoiceStatus>("release_talk");
}

/** Muting always succeeds and turns the microphone off at once. */
export function setVoiceMuted(muted: boolean): Promise<VoiceStatus> {
  return invoke<VoiceStatus>("set_voice_muted", { muted });
}

/** Is Ollama reachable, which local models are installed, and which one to preselect. */
export function checkProvider(): Promise<ProviderCheck> {
  return invoke<ProviderCheck>("check_provider");
}
