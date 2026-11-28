// An in-memory stand-in for loki-runtime behind mocked Tauri IPC, for component tests. It streams
// turns through real Channel objects exactly as Tauri frames them ({ index, message }), and mimics
// the mock provider ("Mock reply: <text>", one word per token).
import type { AppStatus, Conversation, Message, TurnEvent } from "@loki/contracts";
import type { Channel } from "@tauri-apps/api/core";
import { mockIPC } from "@tauri-apps/api/mocks";

interface TauriInternals {
  runCallback(id: number, frame: { index: number; message: TurnEvent }): void;
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
}

export interface FakeBackend {
  calls: { cmd: string; args: Record<string, unknown> }[];
  conversations: Conversation[];
  messages: Map<string, Message[]>;
  /** Lets a held stream continue. */
  release(): void;
  failWith: string | null;
}

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

  const backend: FakeBackend = {
    calls: [],
    conversations: [...(options.conversations ?? [])],
    messages: new Map(Object.entries(options.messages ?? {})),
    release: () => releaseGate?.(),
    failWith: options.failWith ?? null,
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
        return STATUS;
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
