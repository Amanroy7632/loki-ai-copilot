import type { AllowedRootInfo, Capability, CapabilityStatus, PermissionRow } from "@loki/contracts";
import { useEffect, useState, type FormEvent } from "react";

import { addRoot, grantPermission, listPermissions, listRoots, removeRoot, revokePermission } from "../api";

/** What each capability lets tools do, in plain language. */
export const CAPABILITY_TEXT: Record<Capability, { label: string; description: string }> = {
  clock: { label: "Clock", description: "Read the current time and time zone." },
  system_info: {
    label: "System information",
    description: "Read OS, CPU, memory and disk details. Never the computer or user name.",
  },
  app_launch: {
    label: "Open apps",
    description: "List installed apps and open one. Opening an app always asks you first.",
  },
  file_read: { label: "Read files", description: "Search and read text files, only in the folders below." },
  network: { label: "Internet", description: "Search the web and fetch pages." },
  microphone: { label: "Microphone", description: "Listen while you use push-to-talk." },
  notifications: { label: "Notifications", description: "Show system notifications." },
  developer_tools: {
    label: "Developer tools",
    description: "Read the status of local developer tools such as Docker. Never changes them.",
  },
};

function statusText(status: CapabilityStatus): { label: string; reason: string | null } {
  switch (status.status) {
    case "supported":
      return { label: "Available", reason: null };
    case "permission_required":
      return { label: "Needs an OS permission", reason: status.reason };
    case "denied":
      return { label: "Blocked by the OS", reason: status.reason };
    case "unavailable":
      return { label: "Not available", reason: status.reason };
  }
}

const ANY = "*";

/**
 * Permissions and capabilities (P5-T05, FR-09). Shows what the platform reports for every
 * capability, with the reason when it isn't available, and the grants. All decisions are the
 * runtime's: this screen reloads from it after every change, so it never shows a stale grant.
 */
export function PermissionsView() {
  const [rows, setRows] = useState<PermissionRow[] | null>(null);
  const [roots, setRoots] = useState<AllowedRootInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => Promise.all([listPermissions(), listRoots()]);

  useEffect(() => {
    let active = true;
    load().then(
      ([permissions, folders]) => {
        if (!active) return;
        setRows(permissions);
        setRoots(folders);
      },
      (failure: unknown) => {
        if (active) setError(String(failure));
      },
    );
    return () => {
      active = false;
    };
  }, []);

  /** Runs one change, then reloads everything from the runtime. Resolves whether it worked. */
  const change = async (action: () => Promise<string | null>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    setNotice(null);
    let ok = false;
    try {
      setNotice(await action());
      ok = true;
    } catch (failure) {
      setError(String(failure));
    }
    try {
      const [permissions, folders] = await load();
      setRows(permissions);
      setRoots(folders);
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
    return ok;
  };

  if (rows === null) {
    return (
      <section className="permissions" aria-labelledby="permissions-title">
        <h2 id="permissions-title">Permissions</h2>
        {error ? <p role="alert">{error}</p> : <p>Loading…</p>}
      </section>
    );
  }

  const fileRead = rows.find((row) => row.capability === "file_read");
  const fileReadUsable = fileRead !== undefined && fileRead.status.status !== "unavailable";

  return (
    <section className="permissions" aria-labelledby="permissions-title" aria-busy={busy}>
      <h2 id="permissions-title">Permissions</h2>
      <p className="app-meta">
        Tools can only use what you allow here. Revoking takes effect for the next tool call.
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}

      <ul className="capabilities">
        {rows.map((row) => (
          <CapabilityItem
            key={row.capability}
            row={row}
            busy={busy}
            onGrant={() => change(() => grantPermission(row.capability, ANY).then(() => null))}
            onRevoke={() => change(() => revokePermission(row.capability, ANY).then(() => null))}
          />
        ))}
      </ul>

      <RootsSection
        roots={roots}
        grantedScopes={fileRead?.scopes ?? []}
        usable={fileReadUsable}
        busy={busy}
        onAdd={(id) =>
          change(async () => {
            const added = await addRoot(id);
            return added
              ? `Added “${added.id}”. Allow reading to let tools use it.`
              : "No folder was chosen.";
          })
        }
        onRemove={(id) => change(() => removeRoot(id).then(() => `Removed “${id}” and revoked its access.`))}
        onGrant={(id) => change(() => grantPermission("file_read", id).then(() => null))}
        onRevoke={(id) => change(() => revokePermission("file_read", id).then(() => null))}
      />
    </section>
  );
}

function CapabilityItem(props: {
  row: PermissionRow;
  busy: boolean;
  onGrant: () => void;
  onRevoke: () => void;
}) {
  const { row, busy } = props;
  const text = CAPABILITY_TEXT[row.capability];
  const status = statusText(row.status);
  const unavailable = row.status.status === "unavailable";
  const allowedEverywhere = row.scopes.includes(ANY);
  const headingId = `capability-${row.capability}`;
  const reasonId = `capability-${row.capability}-reason`;

  return (
    <li className={`capability capability-${row.status.status}`} aria-labelledby={headingId}>
      <div className="capability-head">
        <h3 id={headingId}>{text.label}</h3>
        <span className="capability-status">{status.label}</span>
      </div>
      <p>{text.description}</p>
      {status.reason && (
        <p id={reasonId} className="capability-reason">
          {status.reason}
        </p>
      )}
      <div className="capability-actions">
        {allowedEverywhere ? (
          <>
            <span className="granted">
              {row.capability === "file_read" ? "Allowed in every folder" : "Allowed"}
            </span>
            <button
              type="button"
              disabled={busy}
              onClick={props.onRevoke}
              aria-label={`Revoke ${text.label}`}
            >
              Revoke
            </button>
          </>
        ) : row.capability === "file_read" ? (
          <span>{row.scopes.length > 0 ? `Allowed in ${row.scopes.length} folder(s)` : "Not allowed"}</span>
        ) : (
          <>
            <span>Not allowed</span>
            <button
              type="button"
              disabled={busy || unavailable}
              aria-describedby={status.reason ? reasonId : undefined}
              onClick={props.onGrant}
              aria-label={`Allow ${text.label}`}
            >
              Allow
            </button>
          </>
        )}
      </div>
    </li>
  );
}

function RootsSection(props: {
  roots: AllowedRootInfo[];
  grantedScopes: string[];
  usable: boolean;
  busy: boolean;
  onAdd: (id: string) => Promise<boolean>;
  onRemove: (id: string) => Promise<boolean>;
  onGrant: (id: string) => Promise<boolean>;
  onRevoke: (id: string) => Promise<boolean>;
}) {
  const { roots, grantedScopes, busy } = props;
  const [name, setName] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const id = name.trim();
    if (id === "") return;
    void props.onAdd(id).then((ok) => {
      if (ok) setName("");
    });
  };

  return (
    <section className="roots" aria-labelledby="roots-title">
      <h3 id="roots-title">Allowed folders</h3>
      <p className="app-meta">
        File tools can only see these folders. The model only sees the short name, never the path.
      </p>
      {roots.length === 0 ? (
        <p className="empty">No folders yet.</p>
      ) : (
        <ul>
          {roots.map((root) => {
            const granted = grantedScopes.includes(root.id);
            return (
              <li key={root.id} className="root">
                <div>
                  <strong>{root.id}</strong> <code className="root-path">{root.path}</code>
                </div>
                {confirming === root.id ? (
                  <div
                    key="confirm"
                    className="delete-confirm"
                    role="group"
                    aria-label={`Remove folder ${root.id}?`}
                  >
                    <span>Remove “{root.id}”? Its access is revoked too. Your files aren't touched.</span>
                    <button type="button" autoFocus onClick={() => setConfirming(null)}>
                      Cancel
                    </button>
                    <button
                      type="button"
                      className="danger"
                      disabled={busy}
                      onClick={() => {
                        setConfirming(null);
                        void props.onRemove(root.id);
                      }}
                    >
                      Remove
                    </button>
                  </div>
                ) : (
                  // Distinct keys: without them React reuses the focused "Remove folder" button as
                  // the confirm row's "Remove", so focus would land on the destructive action.
                  <div key="actions" className="capability-actions">
                    <span className={granted ? "granted" : undefined}>
                      {granted ? "Reading allowed" : "Reading not allowed"}
                    </span>
                    {granted ? (
                      <button
                        type="button"
                        disabled={busy}
                        aria-label={`Revoke reading ${root.id}`}
                        onClick={() => void props.onRevoke(root.id)}
                      >
                        Revoke
                      </button>
                    ) : (
                      <button
                        type="button"
                        disabled={busy || !props.usable}
                        aria-label={`Allow reading ${root.id}`}
                        onClick={() => void props.onGrant(root.id)}
                      >
                        Allow reading
                      </button>
                    )}
                    <button
                      type="button"
                      disabled={busy}
                      aria-label={`Remove folder ${root.id}`}
                      onClick={() => setConfirming(root.id)}
                    >
                      Remove
                    </button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      <form className="add-root" onSubmit={submit}>
        <label>
          Short name
          <input
            value={name}
            maxLength={32}
            placeholder="docs"
            aria-describedby="root-name-hint"
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <span id="root-name-hint" className="app-meta">
          a–z, 0–9, _ or -
        </span>
        <button type="submit" disabled={busy || name.trim() === ""}>
          Choose folder…
        </button>
      </form>
    </section>
  );
}
