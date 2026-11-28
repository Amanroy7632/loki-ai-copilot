import type { Message } from "@loki/contracts";
import { useEffect, useRef, useState } from "react";

import { CheckIcon, CopyIcon, LogoMark, SparkleIcon, ToolIcon } from "../icons";
import type { ToolActivity } from "../turn";
import { Markdown } from "./Markdown";

export interface MessageListProps {
  messages: Message[];
  /** The user's message for a turn still in flight, before it is stored. */
  pendingUserText: string | null;
  /** Reply text streaming right now, or null when nothing streams. */
  streamingText: string | null;
  tools: ToolActivity[];
  /** Fills the composer with a suggested prompt (the user still sends it). */
  onSuggest?: ((text: string) => void) | undefined;
}

/** Starter prompts, matched to what Loki can actually do (tools still need permission). */
export const SUGGESTIONS: { title: string; prompt: string }[] = [
  { title: "What time is it in Tokyo?", prompt: "What time is it in Tokyo?" },
  { title: "Open the Calculator app", prompt: "Open the Calculator app." },
  { title: "Describe this computer", prompt: "What OS, CPU and memory does this computer have?" },
  { title: "Explain something simply", prompt: "Explain how a local AI model works, in three short points." },
];

function CopyReply({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="ghost small"
      aria-label="Copy reply"
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => setCopied(true));
      }}
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
      <span>{copied ? "Copied" : "Copy"}</span>
    </button>
  );
}

function ToolChips({ tools }: { tools: ToolActivity[] }) {
  if (tools.length === 0) return null;
  return (
    <ul className="tool-chips" aria-label="Tool activity">
      {tools.map((tool, index) => (
        <li key={index} className={`tool-chip tool-${tool.status}`}>
          <ToolIcon />
          <span>
            {tool.tool}: {tool.status.replace("_", " ")}
            {tool.detail ? ` (${tool.detail})` : ""}
          </span>
        </li>
      ))}
    </ul>
  );
}

function Welcome({ onSuggest }: { onSuggest?: ((text: string) => void) | undefined }) {
  return (
    <div className="welcome">
      <LogoMark className="welcome-logo" />
      <h2 className="welcome-title">How can I help?</h2>
      <p className="welcome-sub">
        Loki runs on this computer. Your conversations stay here, and tools only do what you allow.
      </p>
      {onSuggest && (
        <ul className="suggestions" aria-label="Suggestions">
          {SUGGESTIONS.map((suggestion) => (
            <li key={suggestion.title}>
              <button type="button" className="suggestion" onClick={() => onSuggest(suggestion.prompt)}>
                <SparkleIcon />
                <span>{suggestion.title}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * A `log` region: screen readers announce new messages politely. The streaming reply is marked
 * `aria-busy` so assistive tech waits for it to settle instead of reading every token.
 */
export function MessageList({
  messages,
  pendingUserText,
  streamingText,
  tools,
  onSuggest,
}: MessageListProps) {
  const end = useRef<HTMLLIElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView?.({ block: "end" });
  }, [messages, pendingUserText, streamingText, tools]);

  const visible = messages.filter((message) => message.role === "user" || message.role === "assistant");
  const empty = visible.length === 0 && pendingUserText === null && streamingText === null;
  return (
    <section className="messages" aria-label="Messages">
      {empty && <Welcome onSuggest={onSuggest} />}
      <ol role="log" aria-live="polite" aria-relevant="additions" className="message-log">
        {visible.map((message) =>
          message.role === "user" ? (
            <li key={message.id} className="message message-user">
              <span className="sr-only">You:</span>
              <p>{message.content}</p>
            </li>
          ) : (
            <li key={message.id} className="message message-assistant">
              <LogoMark className="avatar" />
              <div className="message-body">
                <span className="speaker">Loki</span>
                <Markdown source={message.content} />
                <div className="message-actions">
                  {message.stopped && <span className="marker">Stopped</span>}
                  <CopyReply text={message.content} />
                </div>
              </div>
            </li>
          ),
        )}
        {pendingUserText !== null && (
          <li className="message message-user">
            <span className="sr-only">You:</span>
            <p>{pendingUserText}</p>
          </li>
        )}
        {streamingText !== null && (
          <li className="message message-assistant" aria-busy="true">
            <LogoMark className="avatar avatar-live" />
            <div className="message-body">
              <span className="speaker">Loki</span>
              {streamingText === "" ? (
                <span className="thinking" role="status" aria-label="Thinking">
                  <span />
                  <span />
                  <span />
                </span>
              ) : (
                <>
                  <Markdown source={streamingText} />
                  <span className="caret" aria-hidden="true" />
                </>
              )}
              <ToolChips tools={tools} />
            </div>
          </li>
        )}
        {streamingText === null && tools.length > 0 && (
          <li className="message message-tools">
            <ToolChips tools={tools} />
          </li>
        )}
        <li ref={end} aria-hidden="true" className="end" />
      </ol>
    </section>
  );
}
