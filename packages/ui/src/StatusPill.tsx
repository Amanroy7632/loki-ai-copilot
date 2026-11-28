/** How a status should read. Colour is never the only signal: the label always says it. */
export type StatusTone = "good" | "bad" | "neutral";

export interface StatusPillProps {
  tone: StatusTone;
  /** Short, plain-language state, e.g. "Ollama · bonsai-2:4b". */
  label: string;
  /** Longer explanation, e.g. why the provider isn't ready. */
  detail?: string | undefined;
}

const SYMBOL: Record<StatusTone, string> = { good: "●", bad: "▲", neutral: "○" };

/** A polite live region, so screen readers hear status changes without losing focus. */
export function StatusPill({ tone, label, detail }: StatusPillProps) {
  return (
    <div className={`loki-status loki-status-${tone}`} role="status" aria-live="polite">
      <span aria-hidden="true">{SYMBOL[tone]} </span>
      <span>{label}</span>
      {detail !== undefined && <span className="loki-status-detail">{detail}</span>}
    </div>
  );
}
