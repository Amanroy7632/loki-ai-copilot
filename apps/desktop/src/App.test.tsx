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
