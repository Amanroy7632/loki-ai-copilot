import type { AppStatus } from "@loki/contracts";
import { StatusPill } from "@loki/ui";
import { useEffect, useState } from "react";

import { appStatus } from "./api";
import { ChatView } from "./chat/ChatView";
import { ChatIcon, LogoMark, SettingsIcon, ShieldIcon } from "./icons";
import { PermissionsView } from "./permissions/PermissionsView";
import { SettingsView } from "./settings/SettingsView";
import { VoiceBar } from "./voice/VoiceBar";

type Section = "chat" | "permissions" | "settings";

type Load = { kind: "loading" } | { kind: "ready"; status: AppStatus } | { kind: "error"; message: string };

/** Fetches the runtime status once. All logic lives in loki-runtime; this only displays it. */
export function App() {
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [section, setSection] = useState<Section>("chat");
  const [chatVersion, setChatVersion] = useState(0);

  const settingsChanged = () => {
    // Successful changes required exclusive runtime access: no reply is running. Discard
    // cached conversation text after clear/retention and re-read the provider status.
    setChatVersion((value) => value + 1);
    void appStatus().then(
      (status) => setLoad({ kind: "ready", status }),
      (error: unknown) => setLoad({ kind: "error", message: String(error) }),
    );
  };

  useEffect(() => {
    let active = true;
    appStatus().then(
      (status) => {
        if (active) setLoad({ kind: "ready", status });
      },
      (error: unknown) => {
        if (active) setLoad({ kind: "error", message: String(error) });
      },
    );
    return () => {
      active = false;
    };
  }, []);

  const sections = [
    { name: "chat", label: "Chat", Icon: ChatIcon },
    { name: "permissions", label: "Permissions", Icon: ShieldIcon },
    { name: "settings", label: "Settings", Icon: SettingsIcon },
  ] as const;

  return (
    <div className="shell">
      <aside className="rail">
        <div className="brand">
          <LogoMark />
          <h1>Loki</h1>
        </div>
        {load.kind === "ready" && (
          <nav className="sections" aria-label="Sections">
            {sections.map(({ name, label, Icon }) => (
              <button
                key={name}
                type="button"
                className="rail-item"
                aria-current={section === name ? "page" : undefined}
                onClick={() => setSection(name)}
              >
                <Icon />
                <span>{label}</span>
              </button>
            ))}
          </nav>
        )}
        {load.kind === "ready" && (
          <p className="app-meta rail-foot">
            <span>Version {load.status.version}</span>
            <span>Tools {load.status.tools_enabled ? "on" : "off"}</span>
          </p>
        )}
      </aside>
      <main className="app">
        <header className="topbar">
          {load.kind === "loading" && <StatusPill tone="neutral" label="Starting…" />}
          {load.kind === "error" && (
            <StatusPill tone="bad" label="Runtime unavailable" detail={load.message} />
          )}
          {load.kind === "ready" && (
            <StatusPill
              tone={load.status.ready ? "good" : "bad"}
              label={providerLabel(load.status)}
              detail={load.status.detail ?? undefined}
            />
          )}
          {load.kind === "ready" && <VoiceBar />}
        </header>
        {load.kind === "ready" && (
          <>
            {/* Chat stays mounted so a running reply or a pending approval survives switching. */}
            <div className="section" hidden={section !== "chat"}>
              <ChatView key={chatVersion} modelLabel={providerLabel(load.status)} />
            </div>
            {section === "permissions" && (
              <div className="section section-page">
                <PermissionsView />
              </div>
            )}
            {section === "settings" && (
              <div className="section section-page">
                <SettingsView onChanged={settingsChanged} />
              </div>
            )}
          </>
        )}
      </main>
    </div>
  );
}

/** A mock provider is always named as one; it is never presented as a model. */
export function providerLabel(status: AppStatus): string {
  if (status.provider === "mock") return "Mock provider (no model)";
  return status.model ? `${status.provider} · ${status.model}` : status.provider;
}
