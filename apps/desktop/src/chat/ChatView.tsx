import type { ApprovalPreview, Conversation, ConversationId, Message, TurnEvent } from "@loki/contracts";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import {
  deleteConversation,
  getMessages,
  listConversations,
  newTurnId,
  renameConversation,
  retryReply,
  sendMessage,
  stopReply,
  streamTurn,
} from "../api";
import { applyTurnEvent, initialTurn, type TurnState } from "../turn";
import { Composer } from "./Composer";
import { ConversationList } from "./ConversationList";
import { MessageList } from "./MessageList";

export interface ChatViewProps {
  /** Renders the approval UI for a pending approval (P5-T04). */
  renderApproval?: (approval: ApprovalPreview) => ReactNode;
}

/**
 * The chat screen (P5-T03). All behaviour lives in loki-runtime; this holds only view state and
 * reloads what the runtime stored after every turn, so the screen always matches the database.
 */
export function ChatView({ renderApproval }: ChatViewProps) {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selected, setSelected] = useState<ConversationId | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [turn, setTurn] = useState<TurnState | null>(null);
  const [pendingUserText, setPendingUserText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const turnId = useRef<string | null>(null);
  const composer = useRef<HTMLTextAreaElement>(null);

  const streaming = turn?.phase === "streaming";

  const refresh = useCallback(async (id: ConversationId | null) => {
    setConversations(await listConversations());
    setMessages(id ? await getMessages(id) : []);
  }, []);

  // Initial load. State is only set from the promise callbacks, and not after unmount.
  useEffect(() => {
    let active = true;
    listConversations().then(
      (list) => {
        if (active) setConversations(list);
      },
      (failure: unknown) => {
        if (active) setError(String(failure));
      },
    );
    return () => {
      active = false;
    };
  }, []);

  const runTurn = async (
    start: (turnId: string) => (onEvent: (event: TurnEvent) => void) => Promise<void>,
  ) => {
    const id = newTurnId();
    turnId.current = id;
    let state = initialTurn();
    setTurn(state);
    setError(null);
    let conversationId = selected;
    try {
      await streamTurn(start(id), (event) => {
        state = applyTurnEvent(state, event);
        setTurn(state);
        if (event.type === "started") {
          conversationId = event.conversation.id;
          setSelected(event.conversation.id);
          setConversations((list) => [event.conversation, ...list]);
        }
      });
    } catch (failure) {
      setError(String(failure));
      setTurn({ ...state, phase: "failed", error: String(failure) });
    } finally {
      turnId.current = null;
      setPendingUserText(null);
      await refresh(conversationId).catch((failure: unknown) => setError(String(failure)));
      composer.current?.focus();
    }
  };

  const onSend = (text: string) => {
    setPendingUserText(text);
    void runTurn((id) => (onEvent) => sendMessage(id, selected, text, onEvent));
  };

  const onRetry = () => {
    if (!selected) return;
    const id = selected;
    // The runtime replaces trailing replies; hide them now so the new one streams in their place.
    setMessages((list) => {
      const copy = [...list];
      while (copy.at(-1)?.role === "assistant") copy.pop();
      return copy;
    });
    void runTurn((turnIdArg) => (onEvent) => retryReply(turnIdArg, id, onEvent));
  };

  const onStop = () => {
    if (turnId.current) void stopReply(turnId.current);
  };

  const onSelect = (id: ConversationId) => {
    setSelected(id);
    setTurn(null);
    getMessages(id).then(setMessages, (failure: unknown) => setError(String(failure)));
  };

  const onNew = () => {
    setSelected(null);
    setMessages([]);
    setTurn(null);
    composer.current?.focus();
  };

  const onRename = async (id: ConversationId, title: string) => {
    const updated = await renameConversation(id, title);
    setConversations((list) => list.map((c) => (c.id === id ? updated : c)));
  };

  const onDelete = async (id: ConversationId) => {
    await deleteConversation(id);
    if (selected === id) onNew();
    setConversations((list) => list.filter((c) => c.id !== id));
  };

  const last = messages.at(-1);
  const canRetry = selected !== null && last !== undefined && !streaming;
  const notice =
    turn?.phase === "stopped"
      ? "Stopped. The partial reply was saved; Retry regenerates it."
      : turn?.phase === "failed"
        ? `The reply failed: ${turn.error ?? "unknown error"}. Your message was kept; fix the cause, then Retry.`
        : null;

  return (
    <div className="chat">
      <ConversationList
        conversations={conversations}
        selected={selected}
        disabled={streaming}
        onSelect={onSelect}
        onNew={onNew}
        onRename={onRename}
        onDelete={onDelete}
      />
      <div className="chat-main">
        <MessageList
          messages={messages}
          pendingUserText={pendingUserText}
          streamingText={streaming ? (turn?.text ?? "") : null}
          tools={turn?.tools ?? []}
        />
        {turn?.pendingApproval && renderApproval?.(turn.pendingApproval)}
        {notice && (
          <p className="notice" role="status">
            {notice}
          </p>
        )}
        {error && turn?.phase !== "failed" && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <Composer
          ref={composer}
          streaming={streaming}
          canRetry={canRetry}
          onSend={onSend}
          onStop={onStop}
          onRetry={onRetry}
        />
      </div>
    </div>
  );
}
