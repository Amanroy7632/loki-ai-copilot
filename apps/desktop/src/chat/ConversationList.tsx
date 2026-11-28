import type { Conversation, ConversationId } from "@loki/contracts";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { PencilIcon, PlusIcon, SearchIcon, TrashIcon } from "../icons";

export interface ConversationListProps {
  conversations: Conversation[];
  selected: ConversationId | null;
  /** While a reply streams, switching or editing conversations is paused. */
  disabled: boolean;
  onSelect: (id: ConversationId) => void;
  onNew: () => void;
  onRename: (id: ConversationId, title: string) => Promise<void>;
  onDelete: (id: ConversationId) => Promise<void>;
}

type Mode =
  { kind: "idle" } | { kind: "renaming"; id: ConversationId } | { kind: "deleting"; id: ConversationId };

/**
 * Keyboard: Tab reaches the list; Up/Down move between conversations; Enter opens one.
 * Rename: Enter saves, Escape cancels, focus returns to the conversation.
 * Delete always asks first, with focus on Cancel.
 */
export function ConversationList(props: ConversationListProps) {
  const { selected, disabled } = props;
  const [mode, setMode] = useState<Mode>({ kind: "idle" });
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const conversations = needle
    ? props.conversations.filter((c) => c.title.toLowerCase().includes(needle))
    : props.conversations;
  const itemRefs = useRef(new Map<ConversationId, HTMLButtonElement>());
  const focusAfter = useRef<ConversationId | null>(null);

  useEffect(() => {
    if (mode.kind === "idle" && focusAfter.current) {
      itemRefs.current.get(focusAfter.current)?.focus();
      focusAfter.current = null;
    }
  }, [mode]);

  const moveFocus = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = conversations[(index + step + conversations.length) % conversations.length];
    if (next) itemRefs.current.get(next.id)?.focus();
  };

  const finish = (id: ConversationId) => {
    focusAfter.current = id;
    setMode({ kind: "idle" });
  };

  return (
    <nav className="conversations" aria-label="Conversations">
      <button type="button" className="new-chat" onClick={props.onNew} disabled={disabled}>
        <PlusIcon />
        <span>New chat</span>
      </button>
      {props.conversations.length > 0 && (
        <label className="search">
          <SearchIcon />
          <span className="sr-only">Search conversations</span>
          <input
            type="search"
            placeholder="Search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
      )}
      <h2 className="sidebar-label">Recent</h2>
      {props.conversations.length === 0 ? (
        <p className="empty">No conversations yet.</p>
      ) : conversations.length === 0 ? (
        <p className="empty">No conversations match “{query.trim()}”.</p>
      ) : (
        <ul>
          {conversations.map((conversation, index) => (
            <li key={conversation.id}>
              {mode.kind === "renaming" && mode.id === conversation.id ? (
                <RenameField
                  conversation={conversation}
                  onSave={async (title) => {
                    await props.onRename(conversation.id, title);
                    finish(conversation.id);
                  }}
                  onCancel={() => finish(conversation.id)}
                />
              ) : mode.kind === "deleting" && mode.id === conversation.id ? (
                <DeleteConfirm
                  conversation={conversation}
                  onConfirm={async () => {
                    setMode({ kind: "idle" });
                    await props.onDelete(conversation.id);
                  }}
                  onCancel={() => finish(conversation.id)}
                />
              ) : (
                <div className="conversation-row">
                  <button
                    type="button"
                    className="conversation"
                    aria-current={conversation.id === selected ? "page" : undefined}
                    disabled={disabled}
                    ref={(element) => {
                      if (element) itemRefs.current.set(conversation.id, element);
                      else itemRefs.current.delete(conversation.id);
                    }}
                    onClick={() => props.onSelect(conversation.id)}
                    onKeyDown={(event) => moveFocus(event, index)}
                  >
                    {conversation.title}
                  </button>
                  <button
                    type="button"
                    className="icon"
                    aria-label={`Rename ${conversation.title}`}
                    disabled={disabled}
                    onClick={() => setMode({ kind: "renaming", id: conversation.id })}
                  >
                    <PencilIcon />
                  </button>
                  <button
                    type="button"
                    className="icon"
                    aria-label={`Delete ${conversation.title}`}
                    disabled={disabled}
                    onClick={() => setMode({ kind: "deleting", id: conversation.id })}
                  >
                    <TrashIcon />
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </nav>
  );
}

function RenameField(props: {
  conversation: Conversation;
  onSave: (title: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [title, setTitle] = useState(props.conversation.title);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    if (title.trim() === "") {
      setError("A title can't be empty.");
      return;
    }
    try {
      await props.onSave(title);
    } catch (failure) {
      setError(String(failure));
    }
  };
  return (
    <div className="rename">
      <input
        aria-label={`New title for ${props.conversation.title}`}
        value={title}
        maxLength={200}
        autoFocus
        onChange={(event) => setTitle(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void save();
          } else if (event.key === "Escape") {
            event.preventDefault();
            props.onCancel();
          }
        }}
      />
      {error && (
        <span role="alert" className="field-error">
          {error}
        </span>
      )}
    </div>
  );
}

function DeleteConfirm(props: {
  conversation: Conversation;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
}) {
  return (
    <div className="delete-confirm" role="group" aria-label={`Delete ${props.conversation.title}?`}>
      <span>Delete “{props.conversation.title}” and its messages?</span>
      <button type="button" autoFocus onClick={props.onCancel}>
        Cancel
      </button>
      <button type="button" className="danger" onClick={() => void props.onConfirm()}>
        Delete
      </button>
    </div>
  );
}
