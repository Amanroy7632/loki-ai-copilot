// An in-memory stand-in for loki-runtime behind mocked Tauri IPC, for component tests. It streams
// turns through real Channel objects exactly as Tauri frames them ({ index, message }), and mimics
// the mock provider ("Mock reply: <text>", one word per token). With `approval`, the next turn asks
// for one first and answers like the runtime broker: a "yes" counts only if it is unexpired and
// carries the arguments that were shown.
import type {
  AllowedRootInfo,
  AutostartStatus,
  AppStatus,
  AuditEntry,
  Conversation,
  DesktopSettings,
  JsonValue,
  Message,
  PermissionRow,
  Risk,
  SettingsSnapshot,
  TurnEvent,
  VoiceStatus,
} from "@loki/contracts";
import type { Channel } from "@tauri-apps/api/core";
import { mockIPC } from "@tauri-apps/api/mocks";

interface TauriInternals {
  runCallback(id: number, frame: { index: number; message: TurnEvent | VoiceStatus }): void;
}

function internals(): TauriInternals {
  return (window as unknown as { __TAURI_INTERNALS__: TauriInternals }).__TAURI_INTERNALS__;
}

export interface FakeBackendOptions {
  /** Pause each stream after its first token until `release()` (to observe streaming / stop). */
  holdStream?: boolean;
  /** Make the next turn fail with this error. */
  failWith?: string;
  conversations?: Conversation[];
  messages?: Record<string, Message[]>;
  /** The next turn requests this approval before replying. */
  approval?: { tool: string; risk: Risk; arguments: JsonValue; ttlMs?: number };
  /** Permission rows (default: `DEFAULT_PERMISSIONS`). */
  permissions?: PermissionRow[];
  roots?: AllowedRootInfo[];
  /** What the "native folder picker" returns; `null` = the user cancelled. */
  pickFolder?: string | null;
}

export interface FakeBackend {
  settings: SettingsSnapshot;
  /** What the OS reports for start at login. */
  autostart: AutostartStatus;
  /** The next autostart change fails with this message (e.g. the OS refused). */
  autostartError: string | null;
  /** The runtime's voice status. Change it with `setVoice` to simulate the tray or the runtime. */
  voice: VoiceStatus;
  setVoice(next: VoiceStatus): void;
  audit: AuditEntry[];
  busy: boolean;
  exportPicked: boolean;
  dataError: string | null;
  cleanupError: string | null;
  calls: { cmd: string; args: Record<string, unknown> }[];
  conversations: Conversation[];
  messages: Map<string, Message[]>;
  /** Lets a held stream continue. */
  release(): void;
  failWith: string | null;
  /** Each approval answer and whether the fake runtime accepted it. */
  answers: { approvalId: string; approved: boolean; shownArguments: JsonValue; accepted: boolean }[];
  permissions: PermissionRow[];
  roots: AllowedRootInfo[];
  pickFolder: string | null;
}

/** Like the macOS adapter today: some capabilities work, later-phase ones are unavailable. */
export const DEFAULT_PERMISSIONS: PermissionRow[] = [
  { capability: "clock", status: { status: "supported" }, scopes: [] },
  { capability: "system_info", status: { status: "supported" }, scopes: [] },
  { capability: "app_launch", status: { status: "supported" }, scopes: [] },
  { capability: "file_read", status: { status: "supported" }, scopes: [] },
  { capability: "network", status: { status: "unavailable", reason: "Web tools arrive in P7." }, scopes: [] },
  {
    capability: "microphone",
    status: {
      status: "permission_required",
      reason: "Allow Loki in System Settings › Privacy › Microphone.",
    },
    scopes: [],
  },
  { capability: "notifications", status: { status: "unavailable", reason: "Not built yet." }, scopes: [] },
  { capability: "developer_tools", status: { status: "unavailable", reason: "Not built yet." }, scopes: [] },
];

const ROOT_ID = /^[a-z0-9_-]{1,32}$/;

export const STATUS: AppStatus = {
  version: "0.0.1",
  provider: "mock",
  model: null,
  ready: true,
  detail: null,
  tools_enabled: false,
};

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

export function installFakeBackend(options: FakeBackendOptions = {}): FakeBackend {
  let counter = 0;
  const next = (prefix: string) => `${prefix}${++counter}`;
  let releaseGate: (() => void) | null = null;
  const stopped = new Set<string>();

  const voiceWatchers: { send: (status: VoiceStatus) => void }[] = [];
  const backend: FakeBackend = {
    autostart: { enabled: false, availability: { status: "supported" } },
    autostartError: null,
    voice: {
      phase: "idle",
      mic_open: false,
      muted: false,
      push_to_talk: { status: "unavailable", reason: "Push-to-talk needs local speech-to-text." },
    },
    setVoice: (next) => {
      backend.voice = next;
      for (const watcher of voiceWatchers) watcher.send(next);
    },
    settings: {
      values: { provider: "mock", model_id: null, network_mode: "local_only", history_days: null },
      memory: { status: "unavailable", reason: "Personal memory is not available yet." },
      wake_word: { status: "unavailable", reason: "Wake-word listening is not available in this build." },
    },
    audit: [],
    busy: false,
    exportPicked: true,
    dataError: null,
    cleanupError: null,
    calls: [],
    conversations: [...(options.conversations ?? [])],
    messages: new Map(Object.entries(options.messages ?? {})),
    release: () => releaseGate?.(),
    failWith: options.failWith ?? null,
    answers: [],
    permissions: structuredClone(options.permissions ?? DEFAULT_PERMISSIONS),
    roots: structuredClone(options.roots ?? []),
    pickFolder: options.pickFolder ?? null,
  };
  /** Revokes every grant with this scope, like the runtime's `revoke_scope`. */
  const revokeScope = (scope: string) => {
    for (const row of backend.permissions) row.scopes = row.scopes.filter((s) => s !== scope);
  };
  let approvalConfig = options.approval ?? null;
  const waiting = new Map<
    string,
    { resolve: (approved: boolean) => void; arguments: JsonValue; expiresAt: number }
  >();

  const requestApproval = (emit: (message: TurnEvent) => void) => {
    const config = approvalConfig;
    if (!config) return Promise.resolve(null);
    approvalConfig = null;
    const id = next("ap");
    const expiresAt = Date.now() + (config.ttlMs ?? 120_000);
    return new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => {
        waiting.delete(id);
        resolve(false);
      }, expiresAt - Date.now());
      waiting.set(id, {
        resolve: (approved) => {
          clearTimeout(timer);
          resolve(approved);
        },
        arguments: config.arguments,
        expiresAt,
      });
      emit({
        type: "approval_requested",
        approval: {
          id,
          request_id: next("rq"),
          tool: config.tool,
          risk: config.risk,
          arguments: config.arguments,
          expires_at: expiresAt,
        },
      });
    }).then((approved) => {
      emit({
        type: "tool_finished",
        tool: config.tool,
        status: approved ? "succeeded" : "cancelled",
        detail: approved ? null : "the user did not approve this action",
      });
      return approved;
    });
  };

  const append = (conversationId: string, message: Message) => {
    backend.messages.set(conversationId, [...(backend.messages.get(conversationId) ?? []), message]);
  };

  const stream = async (
    turnId: string,
    channel: Channel<TurnEvent>,
    conversationId: string,
    prompt: string,
    first: TurnEvent[],
  ) => {
    let index = 0;
    const emit = (message: TurnEvent) => internals().runCallback(channel.id, { index: index++, message });
    first.forEach(emit);
    if (backend.failWith) {
      const error = backend.failWith;
      backend.failWith = null;
      emit({ type: "failed", error });
      throw new Error(error);
    }
    await requestApproval(emit);
    const words = `Mock reply: ${prompt}`.split(" ");
    let text = "";
    for (const [i, word] of words.entries()) {
      const token = i < words.length - 1 ? `${word} ` : word;
      await tick();
      if (stopped.has(turnId)) break;
      text += token;
      emit({ type: "token", text: token });
      if (i === 0 && options.holdStream) {
        await new Promise<void>((resolve) => {
          releaseGate = resolve;
        });
      }
    }
    const reply: Message = {
      id: next("m"),
      conversation_id: conversationId,
      role: "assistant",
      content: text,
      created_at: counter,
      stopped: stopped.has(turnId),
    };
    append(conversationId, reply);
    emit(stopped.has(turnId) ? { type: "stopped", message: reply } : { type: "completed", message: reply });
  };

  mockIPC(async (cmd, rawArgs) => {
    const args = (rawArgs ?? {}) as Record<string, unknown>;
    backend.calls.push({ cmd, args });
    switch (cmd) {
      case "app_status":
        return {
          ...STATUS,
          provider: backend.settings.values.provider,
          model: backend.settings.values.model_id,
        };
      case "get_voice_status":
        return structuredClone(backend.voice);
      case "watch_voice": {
        const channel = args.onStatus as Channel<VoiceStatus>;
        let index = 0;
        const watcher = {
          send: (status: VoiceStatus) =>
            internals().runCallback(channel.id, { index: index++, message: status }),
        };
        voiceWatchers.push(watcher);
        watcher.send(backend.voice);
        return structuredClone(backend.voice);
      }
      case "press_talk":
        if (backend.voice.muted) throw new Error("Voice is muted. Unmute to talk.");
        if (backend.voice.push_to_talk.status !== "supported")
          throw new Error(backend.voice.push_to_talk.reason);
        backend.setVoice({ ...backend.voice, phase: "recording", mic_open: true });
        return structuredClone(backend.voice);
      case "release_talk":
        if (backend.voice.phase === "recording") {
          backend.setVoice({ ...backend.voice, phase: "transcribing", mic_open: false });
        }
        return structuredClone(backend.voice);
      case "set_voice_muted": {
        const muted = args.muted as boolean;
        if (muted !== backend.voice.muted) {
          backend.setVoice({ ...backend.voice, muted, mic_open: false, phase: muted ? "muted" : "idle" });
        }
        return structuredClone(backend.voice);
      }
      case "get_autostart":
        return structuredClone(backend.autostart);
      case "set_autostart": {
        if (backend.autostart.availability.status !== "supported") {
          throw new Error("Start at login is not available.");
        }
        if (backend.autostartError) throw new Error(backend.autostartError);
        backend.autostart = { ...backend.autostart, enabled: args.enabled as boolean };
        return structuredClone(backend.autostart);
      }
      case "get_settings":
        return structuredClone(backend.settings);
      case "save_settings": {
        if (backend.busy) throw new Error("Loki is busy. Stop the reply and try again.");
        if (backend.dataError) throw new Error(backend.dataError);
        backend.settings.values = structuredClone(args.settings as DesktopSettings);
        if (backend.cleanupError) throw { message: backend.cleanupError, state_changed: true };
        return structuredClone(backend.settings);
      }
      case "clear_history": {
        if (backend.busy) throw new Error("Loki is busy. Stop the reply and try again.");
        if (backend.dataError) throw new Error(backend.dataError);
        const count = backend.conversations.length;
        backend.conversations = [];
        backend.messages.clear();
        if (backend.cleanupError) throw { message: backend.cleanupError, state_changed: true };
        return count;
      }
      case "export_data":
        if (backend.dataError) throw new Error(backend.dataError);
        return backend.exportPicked;
      case "list_audit":
        return structuredClone(backend.audit);
      case "list_conversations":
        return backend.conversations;
      case "get_messages":
        return backend.messages.get(args.conversationId as string) ?? [];
      case "send_message": {
        const text = args.text as string;
        let id = args.conversationId as string | null;
        const first: TurnEvent[] = [];
        if (id === null) {
          const conversation: Conversation = {
            id: next("c"),
            title: text.slice(0, 60),
            created_at: counter,
            updated_at: counter,
          };
          backend.conversations = [conversation, ...backend.conversations];
          id = conversation.id;
          first.push({ type: "started", conversation });
        }
        append(id, {
          id: next("m"),
          conversation_id: id,
          role: "user",
          content: text,
          created_at: counter,
          stopped: false,
        });
        await stream(args.turnId as string, args.onEvent as Channel<TurnEvent>, id, text, first);
        return null;
      }
      case "retry_reply": {
        const id = args.conversationId as string;
        const list = [...(backend.messages.get(id) ?? [])];
        while (list.at(-1)?.role === "assistant") list.pop();
        backend.messages.set(id, list);
        const prompt = list.at(-1)?.content ?? "";
        await stream(args.turnId as string, args.onEvent as Channel<TurnEvent>, id, prompt, []);
        return null;
      }
      case "stop_reply":
        stopped.add(args.turnId as string);
        releaseGate?.();
        return true;
      case "respond_approval": {
        const approvalId = args.approvalId as string;
        const approved = args.approved as boolean;
        const shownArguments = args.shownArguments as JsonValue;
        const entry = waiting.get(approvalId);
        waiting.delete(approvalId);
        const accepted =
          entry !== undefined &&
          (!approved ||
            (Date.now() < entry.expiresAt &&
              JSON.stringify(shownArguments) === JSON.stringify(entry.arguments)));
        entry?.resolve(approved && accepted);
        backend.answers.push({ approvalId, approved, shownArguments, accepted });
        return accepted;
      }
      case "list_permissions":
        return structuredClone(backend.permissions);
      case "list_roots":
        return structuredClone(backend.roots);
      case "grant_permission": {
        const capability = args.capability as string;
        const scope = args.scope as string;
        const row = backend.permissions.find((r) => r.capability === capability);
        if (!row) throw new Error(`unknown capability '${capability}'`);
        if (row.status.status === "unavailable") {
          throw new Error(`${capability} is unavailable here: ${row.status.reason}`);
        }
        if (scope !== "*" && !backend.roots.some((r) => r.id === scope)) {
          throw new Error(`scope must be '*' or an allowed root ID, not '${scope}'`);
        }
        if (!row.scopes.includes(scope)) row.scopes = [...row.scopes, scope].sort();
        return { capability, scope };
      }
      case "revoke_permission": {
        const row = backend.permissions.find((r) => r.capability === args.capability);
        const before = row?.scopes.length ?? 0;
        if (row) row.scopes = row.scopes.filter((s) => s !== args.scope);
        return before - (row?.scopes.length ?? 0);
      }
      case "add_root": {
        const id = args.id as string;
        if (!ROOT_ID.test(id)) throw new Error("folder names are 1-32 of a-z, 0-9, _ or -");
        const path = backend.pickFolder;
        if (path === null) return null;
        const existing = backend.roots.find((r) => r.id === id);
        if (existing && existing.path !== path) revokeScope(id);
        const root = { id, path };
        backend.roots = [...backend.roots.filter((r) => r.id !== id), root];
        return root;
      }
      case "remove_root": {
        const id = args.id as string;
        if (!backend.roots.some((r) => r.id === id)) return false;
        revokeScope(id);
        backend.roots = backend.roots.filter((r) => r.id !== id);
        return true;
      }
      case "rename_conversation": {
        const id = args.conversationId as string;
        backend.conversations = backend.conversations.map((c) =>
          c.id === id ? { ...c, title: (args.title as string).trim() } : c,
        );
        return backend.conversations.find((c) => c.id === id);
      }
      case "delete_conversation":
        backend.conversations = backend.conversations.filter((c) => c.id !== args.conversationId);
        backend.messages.delete(args.conversationId as string);
        return null;
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return backend;
}
