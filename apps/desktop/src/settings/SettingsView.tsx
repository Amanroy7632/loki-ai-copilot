import type { AuditEntry, CapabilityStatus, DesktopSettings, SettingsSnapshot } from "@loki/contracts";
import { useEffect, useState } from "react";

import { DataControlFailure, clearHistory, exportData, getSettings, listAudit, saveSettings } from "../api";

function reason(status: CapabilityStatus): string {
  return status.status === "supported" ? "Available" : status.reason;
}

/** Settings and destructive actions delegate to the runtime; availability is reported by it. */
export function SettingsView({ onChanged }: { onChanged: () => void }) {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null);
  const [draft, setDraft] = useState<DesktopSettings | null>(null);
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<"save" | "clear" | null>(null);

  useEffect(() => {
    let active = true;
    getSettings().then(
      (value) => {
        if (active) {
          setSnapshot(value);
          setDraft(value.values);
        }
      },
      (failure: unknown) => {
        if (active) setError(String(failure));
      },
    );
    listAudit().then(
      (value) => {
        if (active) setAudit(value);
      },
      (failure: unknown) => {
        if (active) setError(String(failure));
      },
    );
    return () => {
      active = false;
    };
  }, []);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    setConfirm(null);
    try {
      await action();
    } catch (failure) {
      if (failure instanceof DataControlFailure && failure.stateChanged) onChanged();
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      if (!draft) return;
      try {
        const value = await saveSettings(draft);
        setSnapshot(value);
        setDraft(value.values);
        setNotice("Settings saved. Changes apply to the next reply.");
        onChanged();
      } catch (failure) {
        // Saving config may have succeeded even if retention cleanup failed. Show actual values.
        const value = await getSettings();
        setSnapshot(value);
        setDraft(value.values);
        throw failure;
      }
    });

  return (
    <section className="settings" aria-labelledby="settings-title">
      <h2 id="settings-title">Settings and data</h2>
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      {!snapshot || !draft ? (
        <p>Loading settings…</p>
      ) : (
        <>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (draft.history_days !== null && draft.history_days !== snapshot.values.history_days)
                setConfirm("save");
              else void save();
            }}
          >
            <fieldset disabled={busy || confirm !== null}>
              <legend>Model and privacy</legend>
              <label>
                Provider
                <select
                  value={draft.provider}
                  onChange={(event) =>
                    setDraft({ ...draft, provider: event.target.value === "mock" ? "mock" : "ollama" })
                  }
                >
                  <option value="ollama">Ollama (local model)</option>
                  <option value="mock">Mock provider (demo replies)</option>
                </select>
              </label>
              <label>
                Local model tag
                <input
                  value={draft.model_id ?? ""}
                  maxLength={200}
                  required={draft.provider === "ollama"}
                  placeholder="qwen3:4b"
                  onChange={(event) => setDraft({ ...draft, model_id: event.target.value || null })}
                />
              </label>
              <p>Choose a model already installed in Ollama. Saving does not download a model.</p>
              <label>
                Network mode
                <select
                  value={draft.network_mode}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      network_mode: event.target.value === "tools_allowed" ? "tools_allowed" : "local_only",
                    })
                  }
                >
                  <option value="local_only">Local only</option>
                  <option value="tools_allowed">Allow authorized internet tools</option>
                </select>
              </label>
              <p>Inference stays local. Internet tools need separate permissions.</p>
              <label>
                History retention in days (blank keeps forever)
                <input
                  type="number"
                  min={1}
                  max={36500}
                  step={1}
                  value={draft.history_days ?? ""}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      history_days: event.target.value === "" ? null : Number(event.target.value),
                    })
                  }
                />
              </label>
              <p>
                Deletes conversations based on their last activity, at startup and when settings are saved.
              </p>
              <button type="submit">Save settings</button>
            </fieldset>
          </form>
          <p>
            <label>
              <input type="checkbox" disabled checked={false} readOnly aria-describedby="autostart-reason" />{" "}
              Start at login
            </label>
          </p>
          <p id="autostart-reason">{reason(snapshot.autostart)}</p>
          <p>
            <label>
              <input type="checkbox" disabled checked={false} readOnly aria-describedby="wake-reason" /> Wake
              word
            </label>
          </p>
          <p id="wake-reason">{reason(snapshot.wake_word)}</p>
          <h3>Your data</h3>
          <p>Export saves unencrypted JSON to a new file you choose. Existing files are never overwritten.</p>
          <p>
            Clearing history removes conversations and messages. Audit records and permissions stay. Exports
            and backups outside the active database remain.
          </p>
          <div className="settings-actions">
            <button
              type="button"
              disabled={busy || confirm !== null}
              onClick={() =>
                void run(async () => {
                  setNotice((await exportData()) ? "History exported." : "Export cancelled.");
                })
              }
            >
              Export history and memory (JSON)…
            </button>
            <button
              key="clear-request"
              type="button"
              disabled={busy || confirm !== null}
              onClick={() => setConfirm("clear")}
            >
              Clear history…
            </button>
            <button type="button" disabled aria-describedby="memory-reason">
              Delete memory
            </button>
          </div>
          <p id="memory-reason">{reason(snapshot.memory)} Exports identify memory as unavailable.</p>
          {confirm && (
            <div
              className="data-confirm"
              role="group"
              aria-label="Confirm data deletion"
              onKeyDown={(event) => {
                if (event.key === "Escape") setConfirm(null);
              }}
            >
              <p>
                {confirm === "clear"
                  ? "Delete all conversations and messages?"
                  : "Save settings and delete expired conversations now?"}{" "}
                This cannot be undone.
              </p>
              <button key="cancel" type="button" autoFocus onClick={() => setConfirm(null)}>
                Cancel
              </button>
              <button
                key="confirm"
                type="button"
                onClick={() => {
                  if (confirm === "save") void save();
                  else
                    void run(async () => {
                      const count = await clearHistory();
                      onChanged();
                      setNotice(`Cleared ${count} conversation(s).`);
                    });
                }}
              >
                {confirm === "clear" ? "Delete all history" : "Save and delete expired history"}
              </button>
            </div>
          )}
        </>
      )}
      <h3>Audit history</h3>
      <p>Latest 100 tool runs. Summaries are redacted; conversation text is excluded.</p>
      <button type="button" disabled={busy} onClick={() => void run(async () => setAudit(await listAudit()))}>
        Refresh audit
      </button>
      {audit.length === 0 ? (
        <p>No tool runs to show.</p>
      ) : (
        <ul aria-label="Audit entries">
          {audit.map((entry) => (
            <li key={entry.id}>
              <time dateTime={new Date(entry.created_at).toISOString()}>
                {new Date(entry.created_at).toLocaleString()}
              </time>
              {" · "}
              {entry.tool}
              {" · "}
              {entry.status}
              {entry.summary && <p>{entry.summary}</p>}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
