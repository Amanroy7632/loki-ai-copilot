import type {
  AllowedRootInfo,
  Capability,
  CapabilityStatus,
  MicTestResult,
  PermissionRow,
} from "@loki/contracts";
import { useEffect, useId, useState, type FormEvent, type ReactNode } from "react";

import {
  addRoot,
  disableCapability,
  enableCapability,
  grantPermission,
  listPermissions,
  listRoots,
  openPermissionSettings,
  removeRoot,
  revokePermission,
  testMicrophone,
} from "../api";
import {
  AppsIcon,
  BellIcon,
  ChipIcon,
  ClockIcon,
  ExternalIcon,
  FolderIcon,
  GlobeIcon,
  MicIcon,
  TerminalIcon,
  TrashIcon,
} from "../icons";

type Tone = "red" | "blue" | "green" | "orange" | "purple" | "gray" | "teal" | "pink";

/** What each capability lets Loki do, in plain language, with its settings-style tile. */
export const CAPABILITY_TEXT: Record<
  Capability,
  { label: string; description: string; Icon: () => ReactNode; tone: Tone }
> = {
  microphone: {
    label: "Microphone",
    description: "Hear you while you hold push-to-talk or test the microphone. Audio is never saved.",
    Icon: MicIcon,
    tone: "red",
  },
  file_read: {
    label: "Files and folders",
    description: "Search and read text files, only in the folders you add below.",
    Icon: FolderIcon,
    tone: "blue",
  },
  app_launch: {
    label: "Open apps",
    description: "List installed apps and open one. Loki asks you before every launch.",
    Icon: AppsIcon,
    tone: "purple",
  },
  clock: {
    label: "Clock",
    description: "Read the current time and time zone.",
    Icon: ClockIcon,
    tone: "orange",
  },
  system_info: {
    label: "System information",
    description: "Read OS, CPU, memory and disk details. Never the computer or user name.",
    Icon: ChipIcon,
    tone: "gray",
  },
  network: {
    label: "Internet",
    description: "Search the web and fetch pages.",
    Icon: GlobeIcon,
    tone: "teal",
  },
  notifications: {
    label: "Notifications",
    description: "Show system notifications.",
    Icon: BellIcon,
    tone: "pink",
  },
  developer_tools: {
    label: "Developer tools",
    description: "Read the status of local developer tools such as Docker. Never changes them.",
    Icon: TerminalIcon,
    tone: "green",
  },
};

const GROUPS: { title: string; capabilities: Capability[] }[] = [
  { title: "Privacy", capabilities: ["microphone", "file_read"] },
  { title: "Tools", capabilities: ["app_launch", "clock", "system_info"] },
  { title: "Coming later", capabilities: ["network", "notifications", "developer_tools"] },
];

const ANY = "*";

/** A settings-style on/off switch. The accessible name is the setting's title. */
function Switch(props: {
  checked: boolean;
  label: string;
  disabled?: boolean;
  describedBy?: string | undefined;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      aria-describedby={props.describedBy}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
    >
      <span className="switch-thumb" aria-hidden="true" />
    </button>
  );
}

/** One row: tile, title, description, OS status line, switch, and optional extra content. */
function SettingRow(props: {
  capability: Capability;
  status?: ReactNode;
  control?: ReactNode;
  children?: ReactNode;
  muted?: boolean;
}) {
  const text = CAPABILITY_TEXT[props.capability];
  const titleId = useId();
  return (
    <li className={`setting-row${props.muted ? " setting-row-muted" : ""}`} aria-labelledby={titleId}>
      <div className="setting-main">
        <span className={`tile tile-${text.tone}`} aria-hidden="true">
          <text.Icon />
        </span>
        <div className="setting-text">
          <h4 id={titleId}>{text.label}</h4>
          <p>{text.description}</p>
          {props.status}
        </div>
        {props.control}
      </div>
      {props.children}
    </li>
  );
}

function osLine(status: CapabilityStatus, id: string): ReactNode {
  switch (status.status) {
    case "supported":
      return null;
    case "permission_required":
      return (
        <p id={id} className="os-line os-ask">
          {status.reason}
        </p>
      );
    case "denied":
      return (
        <p id={id} className="os-line os-denied">
          {status.reason}
        </p>
      );
    case "unavailable":
      return (
        <p id={id} className="os-line">
          {status.reason}
        </p>
      );
  }
}

/**
 * Privacy & Permissions (P5-T05, redesigned like the OS settings app). Each capability is a
 * switch. Two things must both allow it: Loki's switch here, and, for the microphone, macOS
 * itself. Turning the microphone on shows the macOS prompt the first time; if macOS says no, the
 * switch stays off and "Open System Settings" takes the user to the right page. Turning anything
 * off takes effect for the very next use. The runtime decides; this screen reloads after every
 * change, so it never shows a stale state.
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
        <h2 id="permissions-title">Privacy & Permissions</h2>
        {error ? <p role="alert">{error}</p> : <p>Loading…</p>}
      </section>
    );
  }

  const toggle = (capability: Capability, on: boolean) => {
    const label = CAPABILITY_TEXT[capability].label;
    return change(async () => {
      if (!on) {
        await disableCapability(capability);
        return `${label} is off.`;
      }
      const result = await enableCapability(capability);
      if (result.scopes.includes(ANY)) return `${label} is on.`;
      return result.status.status === "denied"
        ? `macOS didn't allow ${label.toLowerCase()} access, so it stays off.`
        : `${label} wasn't turned on.`;
    });
  };

  const openSettings = (capability: Capability) =>
    change(async () => {
      await openPermissionSettings(capability);
      return "System Settings is open. Come back here after changing it.";
    });

  return (
    <section className="permissions" aria-labelledby="permissions-title" aria-busy={busy}>
      <header className="page-head">
        <h2 id="permissions-title">Privacy & Permissions</h2>
        <p className="app-meta">
          Loki can only use what you turn on here. Turning something off takes effect for the next use.
        </p>
      </header>
      {error && (
        <p className="banner banner-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="banner" role="status">
          {notice}
        </p>
      )}

      {GROUPS.map((group) => (
        <div key={group.title} className="settings-section">
          <h3 className="group-title">{group.title}</h3>
          <ul className="settings-group" aria-label={group.title}>
            {group.capabilities.map((capability) => {
              const current = rows.find((r) => r.capability === capability);
              if (!current) return null;
              if (capability === "microphone") {
                return (
                  <MicrophoneRow
                    key={capability}
                    row={current}
                    busy={busy}
                    onToggle={(on) => void toggle("microphone", on)}
                    onOpenSettings={() => void openSettings("microphone")}
                  />
                );
              }
              if (capability === "file_read") {
                return (
                  <FilesRow
                    key={capability}
                    row={current}
                    roots={roots}
                    busy={busy}
                    onAdd={(id) =>
                      change(async () => {
                        const added = await addRoot(id);
                        return added
                          ? `Added “${added.id}”. Turn on reading to let tools use it.`
                          : "No folder was chosen.";
                      })
                    }
                    onRemove={(id) =>
                      change(() => removeRoot(id).then(() => `Removed “${id}” and revoked its access.`))
                    }
                    onFolderToggle={(id, on) =>
                      change(() =>
                        (on ? grantPermission("file_read", id) : revokePermission("file_read", id)).then(
                          () => null,
                        ),
                      )
                    }
                    onLimit={() => void toggle("file_read", false)}
                    onOpenSettings={() => void openSettings("file_read")}
                  />
                );
              }
              return (
                <PlainRow
                  key={capability}
                  row={current}
                  busy={busy}
                  onToggle={(on) => void toggle(capability, on)}
                />
              );
            })}
          </ul>
        </div>
      ))}
    </section>
  );
}

function PlainRow({
  row,
  busy,
  onToggle,
}: {
  row: PermissionRow;
  busy: boolean;
  onToggle: (on: boolean) => void;
}) {
  const reasonId = useId();
  const unavailable = row.status.status === "unavailable";
  return (
    <SettingRow
      capability={row.capability}
      muted={unavailable}
      status={osLine(row.status, reasonId)}
      control={
        <Switch
          label={CAPABILITY_TEXT[row.capability].label}
          checked={row.scopes.includes(ANY)}
          disabled={busy || unavailable}
          describedBy={unavailable ? reasonId : undefined}
          onChange={onToggle}
        />
      }
    />
  );
}

function MicrophoneRow(props: {
  row: PermissionRow;
  busy: boolean;
  onToggle: (on: boolean) => void;
  onOpenSettings: () => void;
}) {
  const { row } = props;
  const reasonId = useId();
  const on = row.scopes.includes(ANY);
  const status = row.status.status;
  const osAllowed = status === "supported";
  return (
    <SettingRow
      capability="microphone"
      muted={status === "unavailable"}
      status={
        <>
          {osAllowed && (
            <p className="os-line os-ok">
              <span className="dot" aria-hidden="true" /> macOS allows Loki to use the microphone.
            </p>
          )}
          {osLine(row.status, reasonId)}
        </>
      }
      control={
        <Switch
          label="Microphone"
          checked={on && osAllowed}
          disabled={props.busy || status === "unavailable" || (status === "denied" && !on)}
          describedBy={osAllowed ? undefined : reasonId}
          onChange={props.onToggle}
        />
      }
    >
      {status === "denied" && (
        <div className="setting-actions">
          <button type="button" className="small" onClick={props.onOpenSettings}>
            <ExternalIcon />
            <span>Open System Settings</span>
          </button>
        </div>
      )}
      {on && osAllowed && <MicTest />}
    </SettingRow>
  );
}

/** Listens for a few seconds with a live level meter, so the user can see the microphone works. */
function MicTest() {
  const [level, setLevel] = useState(0);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<MicTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setRunning(true);
    setResult(null);
    setError(null);
    try {
      setResult(await testMicrophone(setLevel));
    } catch (failure) {
      setError(String(failure));
    } finally {
      setLevel(0);
      setRunning(false);
    }
  };

  return (
    <div className="mic-test">
      <button type="button" className="small" onClick={() => void run()} disabled={running}>
        <MicIcon />
        <span>{running ? "Listening… say something" : "Test microphone"}</span>
      </button>
      <meter
        className="level-meter"
        aria-label="Microphone level"
        min={0}
        max={1}
        low={0.15}
        high={0.85}
        optimum={0.5}
        value={level}
      />
      <span className="mic-test-result" aria-live="polite">
        {result &&
          (result.stopped_early
            ? "The test was stopped."
            : result.heard_sound
              ? "Your microphone works. Nothing was recorded or saved."
              : "Loki heard only silence. Check the input device in System Settings › Sound.")}
        {error}
      </span>
    </div>
  );
}

function FilesRow(props: {
  row: PermissionRow;
  roots: AllowedRootInfo[];
  busy: boolean;
  onAdd: (id: string) => Promise<boolean>;
  onRemove: (id: string) => Promise<boolean>;
  onFolderToggle: (id: string, on: boolean) => Promise<boolean>;
  onLimit: () => void;
  onOpenSettings: () => void;
}) {
  const { row, roots, busy } = props;
  const [name, setName] = useState("");
  const [confirming, setConfirming] = useState<string | null>(null);
  const everywhere = row.scopes.includes(ANY);
  const usable = row.status.status !== "unavailable";
  const allowedCount = roots.filter((root) => row.scopes.includes(root.id)).length;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const id = name.trim();
    if (id === "") return;
    void props.onAdd(id).then((ok) => {
      if (ok) setName("");
    });
  };

  return (
    <SettingRow
      capability="file_read"
      status={
        <p className="os-line">
          {everywhere
            ? "Allowed in every folder (set from the command line)."
            : allowedCount > 0
              ? `Reading allowed in ${allowedCount} folder${allowedCount === 1 ? "" : "s"}.`
              : "Not allowed in any folder yet."}
        </p>
      }
      control={<span className="count-badge">{everywhere ? "All" : allowedCount}</span>}
    >
      <div className="nested">
        {everywhere && (
          <div className="setting-actions">
            <button type="button" className="small" onClick={props.onLimit} disabled={busy}>
              Limit to the folders below
            </button>
          </div>
        )}
        {roots.length === 0 ? (
          <p className="empty">No folders yet.</p>
        ) : (
          <ul className="folder-list" aria-label="Allowed folders">
            {roots.map((root) => {
              const granted = row.scopes.includes(root.id);
              return (
                <li key={root.id} className="folder-row">
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
                    // Distinct keys: without them React reuses the focused "Remove folder" button
                    // as the confirm row's "Remove", so focus would land on the destructive action.
                    <div key="row" className="folder-main">
                      <FolderIcon />
                      <div className="folder-text">
                        <strong>{root.id}</strong>
                        <code className="root-path">{root.path}</code>
                      </div>
                      <button
                        type="button"
                        className="ghost icon-button"
                        disabled={busy}
                        aria-label={`Remove folder ${root.id}`}
                        onClick={() => setConfirming(root.id)}
                      >
                        <TrashIcon />
                      </button>
                      <Switch
                        label={`Read ${root.id}`}
                        checked={granted || everywhere}
                        disabled={busy || !usable || everywhere}
                        onChange={(on) => void props.onFolderToggle(root.id, on)}
                      />
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
            <FolderIcon />
            <span>Choose folder…</span>
          </button>
        </form>
        <div className="setting-actions">
          <span className="app-meta">
            macOS may ask the first time Loki reads Desktop, Documents or Downloads.
          </span>
          <button type="button" className="ghost small" onClick={props.onOpenSettings}>
            <ExternalIcon />
            <span>Files and Folders in System Settings</span>
          </button>
        </div>
      </div>
    </SettingRow>
  );
}
