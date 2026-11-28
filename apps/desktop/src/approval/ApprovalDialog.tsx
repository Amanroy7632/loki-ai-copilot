import type { ApprovalPreview, JsonValue, Risk } from "@loki/contracts";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

/** Plain-language risk explanations (FR-04). */
export const RISK_TEXT: Record<Risk, { label: string; explanation: string }> = {
  read_only: {
    label: "Reads information",
    explanation: "This reads information on this computer. It doesn't change anything.",
  },
  network: {
    label: "Uses the internet",
    explanation: "This sends a request to the internet.",
  },
  local_change: {
    label: "Changes something",
    explanation: "This changes something on this computer, such as opening an app or writing a file.",
  },
  sensitive: {
    label: "Sensitive",
    explanation:
      "This can send, delete, install or buy something, or act on another device. Check every detail before you approve.",
  },
};

// Characters that are invisible or reorder text (bidi overrides). They are shown as \u{…} so an
// argument can't look like something it isn't. Control characters are already escaped by JSON.
const HIDDEN = /[\u007f-\u009f\u00ad\u061c\u180e\u200b-\u200f\u2028-\u202e\u2060-\u2069\ufeff\ufff9-\ufffb]/g;

/** An argument value exactly as it will run: JSON, nothing elided, hidden characters visible. */
export function showValue(value: JsonValue | undefined): string {
  const json = JSON.stringify(value ?? null, null, 2);
  return json.replace(HIDDEN, (c) => `\\u{${c.charCodeAt(0).toString(16).padStart(4, "0")}}`);
}

/** The visible part of a key: the same escaping as values, without the quotes. */
function showKey(key: string): string {
  return showValue(key).slice(1, -1);
}

function isObject(value: JsonValue): value is { [key: string]: JsonValue } {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function countdown(ms: number): string {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** Re-renders a few times a second so the countdown and the expiry state stay current. */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(timer);
  }, []);
  return now;
}

type Phase = "open" | "sending" | "approved" | "denied" | "refused" | "expired";

export interface ApprovalDialogProps {
  approval: ApprovalPreview;
  /**
   * Sends the answer with the arguments this dialog displayed, so the runtime can refuse a "yes"
   * for anything else. Resolves `false` when the runtime refused it.
   */
  onAnswer: (approved: boolean, shownArguments: JsonValue) => Promise<boolean>;
}

/**
 * The approval dialog (P5-T04, FR-04). Shows the tool, a plain-language risk explanation,
 * **every** argument and the expiry countdown.
 *
 * Keyboard safety: focus starts on Deny, so Enter denies; Escape denies; Tab stays inside the
 * dialog; a held-down key (auto-repeat, e.g. from pressing Enter in the composer) does nothing.
 * For a Sensitive action, the keyboard can only approve after the user moved focus to Approve
 * themselves (Tab, or a click); focus that landed there any other way doesn't count.
 *
 * The runtime decides; this only shows and answers. An expired approval isn't sent at all, and
 * the runtime refuses it anyway.
 */
export function ApprovalDialog({ approval, onAnswer }: ApprovalDialogProps) {
  const now = useNow();
  const [phase, setPhase] = useState<Phase>("open");
  const [refusal, setRefusal] = useState<string | null>(null);
  const deny = useRef<HTMLButtonElement>(null);
  const userMovedFocus = useRef(false);
  const titleId = useId();
  const riskId = useId();

  const shown = approval.arguments;
  const risk = RISK_TEXT[approval.risk];
  const sensitive = approval.risk === "sensitive";
  const remaining = approval.expires_at - now;
  const expired = remaining <= 0;
  const state: Phase = phase === "open" && expired ? "expired" : phase;
  const answerable = state === "open";

  useEffect(() => {
    deny.current?.focus();
  }, []);

  const answer = async (approved: boolean) => {
    if (phase !== "open") return;
    if (approved && Date.now() >= approval.expires_at) {
      setPhase("expired");
      return;
    }
    setPhase("sending");
    try {
      const accepted = await onAnswer(approved, shown);
      setPhase(accepted ? (approved ? "approved" : "denied") : "refused");
    } catch (failure) {
      setRefusal(String(failure));
      setPhase("refused");
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.repeat) {
      event.preventDefault();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      void answer(false);
      return;
    }
    if (event.key === "Tab") {
      userMovedFocus.current = true;
      const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      const first = buttons[0];
      const last = buttons.at(-1);
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  };

  return (
    <div
      className={`approval approval-${approval.risk}`}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={riskId}
      onKeyDown={onKeyDown}
    >
      <h2 id={titleId}>
        Allow <code>{approval.tool}</code>?
      </h2>
      <p id={riskId} className="approval-risk">
        <strong>{risk.label}.</strong> {risk.explanation}
      </p>

      <h3>Arguments (exactly as they will run)</h3>
      {isObject(shown) ? (
        Object.keys(shown).length === 0 ? (
          <p className="empty">No arguments.</p>
        ) : (
          <dl className="approval-arguments">
            {Object.entries(shown).map(([key, value]) => (
              <div key={key}>
                <dt>{showKey(key)}</dt>
                <dd>
                  <pre>{showValue(value)}</pre>
                </dd>
              </div>
            ))}
          </dl>
        )
      ) : (
        <pre className="approval-arguments">{showValue(shown)}</pre>
      )}

      <p className="approval-expiry" role="timer">
        {expired ? "Expired" : `Expires in ${countdown(remaining)}`}
      </p>

      <p role="status" className="approval-outcome">
        {state === "sending" && "Sending your answer…"}
        {state === "approved" && "Approved. Running it now."}
        {state === "denied" && "Denied. It won't run."}
        {state === "expired" && "This request expired, so it won't run. Ask again if you still want it."}
        {state === "refused" &&
          `Loki refused this answer (the request expired or changed), so nothing ran.${refusal ? ` ${refusal}` : ""}`}
      </p>

      <div className="approval-actions">
        <button type="button" ref={deny} disabled={!answerable} onClick={() => void answer(false)}>
          Deny
        </button>
        <button
          type="button"
          className="approve"
          disabled={!answerable}
          onPointerDown={() => {
            userMovedFocus.current = true;
          }}
          onClick={(event) => {
            // detail 0 = activated by the keyboard (or a script), not a pointer.
            if (sensitive && event.detail === 0 && !userMovedFocus.current) return;
            void answer(true);
          }}
        >
          {sensitive ? "Approve sensitive action" : "Approve"}
        </button>
      </div>
    </div>
  );
}
