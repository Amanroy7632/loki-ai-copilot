import { forwardRef, useState } from "react";

export interface ComposerProps {
  streaming: boolean;
  canRetry: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
  onRetry: () => void;
}

/** Enter sends, Shift+Enter adds a line, Escape stops a running reply. */
export const Composer = forwardRef<HTMLTextAreaElement, ComposerProps>(function Composer(props, ref) {
  const [text, setText] = useState("");
  const send = () => {
    const trimmed = text.trim();
    if (trimmed === "" || props.streaming) return;
    props.onSend(trimmed);
    setText("");
  };
  return (
    <form
      className="composer"
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      <textarea
        ref={ref}
        aria-label="Message"
        placeholder="Message Loki…"
        rows={3}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            send();
          } else if (event.key === "Escape" && props.streaming) {
            event.preventDefault();
            props.onStop();
          }
        }}
      />
      <div className="composer-actions">
        {props.streaming ? (
          <button type="button" onClick={props.onStop}>
            Stop
          </button>
        ) : (
          <>
            {props.canRetry && (
              <button type="button" onClick={props.onRetry}>
                Retry
              </button>
            )}
            <button type="submit" disabled={text.trim() === ""}>
              Send
            </button>
          </>
        )}
      </div>
    </form>
  );
});
