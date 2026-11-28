import { forwardRef } from "react";

import { RetryIcon, SendIcon, StopIcon } from "../icons";

export interface ComposerProps {
  /** The draft text (owned by the chat view, so a suggestion can fill it). */
  value: string;
  onChange: (text: string) => void;
  streaming: boolean;
  canRetry: boolean;
  onSend: (text: string) => void;
  onStop: () => void;
  onRetry: () => void;
  /** Shown under the box, e.g. the model in use. */
  hint?: string | undefined;
}

const MAX_ROWS = 8;

/** Enter sends, Shift+Enter adds a line, Escape stops a running reply. Grows with its content. */
export const Composer = forwardRef<HTMLTextAreaElement, ComposerProps>(function Composer(props, ref) {
  const { value } = props;
  const send = () => {
    const trimmed = value.trim();
    if (trimmed === "" || props.streaming) return;
    props.onSend(trimmed);
    props.onChange("");
  };
  const rows = Math.min(MAX_ROWS, Math.max(1, value.split("\n").length));
  return (
    <form
      className="composer"
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      <div className="composer-box">
        <textarea
          ref={ref}
          aria-label="Message"
          placeholder="Message Loki…"
          rows={rows}
          value={value}
          onChange={(event) => props.onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
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
            <button
              type="button"
              className="round stop"
              onClick={props.onStop}
              aria-label="Stop"
              title="Stop (Esc)"
            >
              <StopIcon />
            </button>
          ) : (
            <>
              {props.canRetry && (
                <button
                  type="button"
                  className="round ghost"
                  onClick={props.onRetry}
                  aria-label="Retry"
                  title="Regenerate the last reply"
                >
                  <RetryIcon />
                </button>
              )}
              <button
                type="submit"
                className="round send"
                disabled={value.trim() === ""}
                aria-label="Send"
                title="Send (Enter)"
              >
                <SendIcon />
              </button>
            </>
          )}
        </div>
      </div>
      <p className="composer-hint">
        <span>
          <kbd>Enter</kbd> to send · <kbd>Shift</kbd>+<kbd>Enter</kbd> for a new line
        </span>
        {props.hint && <span className="composer-model">Using {props.hint}</span>}
      </p>
    </form>
  );
});
