import type {
  AuditEntry,
  AutostartStatus,
  CapabilityStatus,
  DesktopSettings,
  LocalModel,
  ProviderCheck,
  SettingsSnapshot,
} from "@loki/contracts";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  DataControlFailure,
  checkProvider,
  clearHistory,
  exportData,
  getAutostart,
  getSettings,
  listAudit,
  saveSettings,
  setAutostart,
} from "../api";

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
              {draft.provider === "ollama" ? (
                <ModelPicker
                  value={draft.model_id}
                  onPick={(model_id) => setDraft((current) => (current ? { ...current, model_id } : current))}
                />
              ) : (
                <p>
                  The mock provider gives canned, clearly labelled replies. It's for trying the app without a
                  model.
                </p>
              )}
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
          <AutostartToggle />

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

/**
 * Start at login (P5-T07). Off until the user turns it on; shows what the OS actually reports,
 * and the reason when it can't be used (e.g. development builds). Loki then starts hidden in
 * the menu bar / tray.
 */
function AutostartToggle() {
  const [status, setStatus] = useState<AutostartStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    getAutostart().then(
      (value) => {
        if (active) setStatus(value);
      },
      (failure: unknown) => {
        if (active) setError(String(failure));
      },
    );
    return () => {
      active = false;
    };
  }, []);

  const usable = status?.availability.status === "supported" && status.enabled !== null;
  const toggle = async (enabled: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await setAutostart(enabled));
    } catch (failure) {
      setError(String(failure));
      // Show what the OS reports now, not what was requested.
      setStatus(await getAutostart().catch(() => status));
    } finally {
      setBusy(false);
    }
  };

  const hint =
    status === null
      ? "Checking…"
      : status.availability.status === "supported"
        ? "Loki starts hidden in the menu bar when you log in."
        : reason(status.availability);

  return (
    <>
      <p>
        <label>
          <input
            type="checkbox"
            disabled={!usable || busy}
            checked={status?.enabled === true}
            aria-describedby="autostart-reason"
            onChange={(event) => void toggle(event.target.checked)}
          />{" "}
          Start at login
        </label>
      </p>
      <p id="autostart-reason">{hint}</p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </>
  );
}

function formatSize(bytes: number): string {
  if (bytes <= 0) return "";
  const gb = bytes / 1e9;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

function modelLabel(model: LocalModel): string {
  const parts = [
    model.display_name,
    formatSize(model.size_bytes),
    model.reviewed ? "reviewed" : "not reviewed by Loki",
  ];
  return `${model.id} — ${parts.filter(Boolean).join(" · ")}`;
}

/**
 * Checks the local Ollama and lists the models actually installed (cloud models are never
 * offered). If no model is chosen yet, or the chosen one isn't installed, it preselects the one
 * Loki suggests; nothing changes until the user saves. Falls back to typing a tag when Ollama
 * can't be reached.
 */
function ModelPicker({ value, onPick }: { value: string | null; onPick: (id: string | null) => void }) {
  const [check, setCheck] = useState<ProviderCheck | null>(null);
  const [checking, setChecking] = useState(true);
  const [picked, setPicked] = useState<string | null>(null);
  // The saved model that wasn't installed and was replaced by the suggestion.
  const [replaced, setReplaced] = useState<string | null>(null);
  const valueRef = useRef(value);
  useEffect(() => {
    valueRef.current = value;
  }, [value]);

  const onPickRef = useRef(onPick);
  useEffect(() => {
    onPickRef.current = onPick;
  }, [onPick]);

  // Starts a check; state is only set from its callbacks.
  const load = useCallback(() => {
    checkProvider().then(
      (result) => {
        setCheck(result);
        setChecking(false);
        const current = valueRef.current;
        const installed = result.models.some((model) => model.id === current);
        if (result.suggested && (current === null || !installed) && result.suggested !== current) {
          onPickRef.current(result.suggested);
          setPicked(result.suggested);
          setReplaced(current);
        }
      },
      (failure: unknown) => {
        setCheck({
          reachable: false,
          detail: String(failure),
          models: [],
          hidden_cloud_models: 0,
          suggested: null,
        });
        setChecking(false);
      },
    );
  }, []);

  // Check once when the picker appears (the provider was switched to Ollama).
  useEffect(load, [load]);

  const run = () => {
    setChecking(true);
    setPicked(null);
    load();
  };

  const models = check?.models ?? [];
  const listed = check?.reachable === true && models.length > 0;
  const missing = value !== null && listed && !models.some((model) => model.id === value);

  return (
    <div className="model-picker">
      <div className="provider-check" aria-live="polite">
        <span
          className={`provider-dot ${checking ? "checking" : check?.reachable ? "up" : "down"}`}
          aria-hidden="true"
        />
        <span>
          {checking
            ? "Checking Ollama…"
            : check?.reachable
              ? `Ollama is running · ${models.length} local model${models.length === 1 ? "" : "s"} installed`
              : "Ollama isn't reachable"}
        </span>
        <button type="button" className="ghost small" onClick={run} disabled={checking}>
          Check again
        </button>
      </div>
      {!checking && check?.detail && <p className="provider-detail">{check.detail}</p>}
      {!checking && check && check.hidden_cloud_models > 0 && (
        <p className="provider-detail">
          {check.hidden_cloud_models} cloud model{check.hidden_cloud_models === 1 ? " is" : "s are"} hidden:
          Loki only runs models on this computer.
        </p>
      )}
      {listed ? (
        <label>
          Local model
          <select value={value ?? ""} onChange={(event) => onPick(event.target.value || null)} required>
            {value === null && <option value="">Choose a model…</option>}
            {missing && <option value={value}>{value} — not installed</option>}
            {models.map((model) => (
              <option key={model.id} value={model.id}>
                {modelLabel(model)}
              </option>
            ))}
          </select>
        </label>
      ) : (
        <label>
          Local model tag
          <input
            value={value ?? ""}
            maxLength={200}
            required
            placeholder="bonsai-2:4b"
            onChange={(event) => onPick(event.target.value || null)}
          />
        </label>
      )}
      {picked && picked === value && (
        <p className="provider-detail picked">
          {replaced ? (
            <>
              <strong>{replaced}</strong> isn't installed, so <strong>{picked}</strong> was selected instead.
              Save settings to use it.
            </>
          ) : (
            <>
              Found <strong>{picked}</strong> installed and selected it. Save settings to use it.
            </>
          )}
        </p>
      )}
      <p>Only models already installed in Ollama are listed. Saving never downloads anything.</p>
    </div>
  );
}
