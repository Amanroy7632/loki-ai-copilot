import type { AppStatus } from "@loki/contracts";
import { mockIPC } from "@tauri-apps/api/mocks";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { App, providerLabel } from "./App";

const base: AppStatus = {
  version: "0.0.1",
  provider: "ollama",
  model: "bonsai-2:4b",
  ready: true,
  detail: null,
  tools_enabled: true,
};

describe("App", () => {
  it("shows the provider and model from the runtime", async () => {
    mockIPC((command) => (command === "app_status" ? base : []));
    render(<App />);
    expect(await screen.findByText("ollama · bonsai-2:4b")).toBeInTheDocument();
    expect(screen.getByText(/Tools on/)).toBeInTheDocument();
  });

  it("labels the mock provider honestly and shows the startup note", async () => {
    const mock: AppStatus = { ...base, provider: "mock", model: null, detail: "No model selected yet" };
    mockIPC((command) => (command === "app_status" ? mock : []));
    render(<App />);
    expect(await screen.findByText("Mock provider (no model)")).toBeInTheDocument();
    expect(screen.getByText("No model selected yet")).toBeInTheDocument();
  });

  it("reports a runtime error instead of pretending to work", async () => {
    mockIPC(() => {
      throw new Error("Ollama is not running");
    });
    render(<App />);
    expect(await screen.findByText("Runtime unavailable")).toBeInTheDocument();
  });
});

describe("providerLabel", () => {
  it("never shows a mock as a model", () => {
    expect(providerLabel({ ...base, provider: "mock", model: "qwen3:4b" })).toBe("Mock provider (no model)");
  });
});

describe("App sections", () => {
  it("switches to Permissions and back without unmounting the chat", async () => {
    const { default: userEvent } = await import("@testing-library/user-event");
    mockIPC((command) => (command === "app_status" ? base : []));
    const user = userEvent.setup();
    render(<App />);
    const nav = await screen.findByRole("navigation", { name: "Sections" });
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-current", "page");
    await user.click(screen.getByRole("button", { name: "Permissions" }));
    expect(await screen.findByRole("heading", { name: "Permissions", level: 2 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Permissions" })).toHaveAttribute("aria-current", "page");
    // The chat is hidden, not gone: a running reply or pending approval survives.
    expect(screen.getByRole("textbox", { name: "Message", hidden: true })).not.toBeVisible();
    await user.click(screen.getByRole("button", { name: "Chat" }));
    expect(screen.getByRole("textbox", { name: "Message" })).toBeVisible();
    expect(nav).toBeInTheDocument();
  });
});
