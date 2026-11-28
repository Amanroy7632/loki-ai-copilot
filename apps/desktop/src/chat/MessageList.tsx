import type { Message } from "@loki/contracts";
import { useEffect, useRef } from "react";

import type { ToolActivity } from "../turn";

export interface MessageListProps {
  messages: Message[];
  /** The user's message for a turn still in flight, before it is stored. */
  pendingUserText: string | null;
  /** Reply text streaming right now, or null when nothing streams. */
  streamingText: string | null;
  tools: ToolActivity[];
}

const SPEAKER: Record<Message["role"], string> = {
  user: "You",
  assistant: "Loki",
  system: "System",
  tool: "Tool",
};

/**
 * A `log` region: screen readers announce new messages politely. The streaming reply is marked
 * `aria-busy` so assistive tech waits for it to settle instead of reading every token.
 */
export function MessageList({ messages, pendingUserText, streamingText, tools }: MessageListProps) {
  const end = useRef<HTMLLIElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView?.({ block: "end" });
  }, [messages, pendingUserText, streamingText]);

  const visible = messages.filter((message) => message.role === "user" || message.role === "assistant");
  return (
    <section className="messages" aria-label="Messages">
      {visible.length === 0 && pendingUserText === null && <p className="empty">Send a message to start.</p>}
      <ol role="log" aria-live="polite" aria-relevant="additions">
        {visible.map((message) => (
          <li key={message.id} className={`message message-${message.role}`}>
            <span className="speaker">{SPEAKER[message.role]}</span>
            <p>{message.content}</p>
            {message.stopped && <span className="marker">Stopped</span>}
          </li>
        ))}
        {pendingUserText !== null && (
          <li className="message message-user">
            <span className="speaker">You</span>
            <p>{pendingUserText}</p>
          </li>
        )}
        {streamingText !== null && (
          <li className="message message-assistant" aria-busy="true">
            <span className="speaker">Loki</span>
            <p>{streamingText === "" ? "…" : streamingText}</p>
            {tools.map((tool, index) => (
              <span key={index} className="tool-activity">
                {tool.tool}: {tool.status.replace("_", " ")}
                {tool.detail ? ` (${tool.detail})` : ""}
              </span>
            ))}
          </li>
        )}
        <li ref={end} aria-hidden="true" className="end" />
      </ol>
    </section>
  );
}
