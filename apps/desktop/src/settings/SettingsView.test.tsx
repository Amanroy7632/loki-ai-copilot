import type { ProviderCheck } from "@loki/contracts";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { App } from "../App";
import { installFakeBackend } from "../test/fakeBackend";
import { SettingsView } from "./SettingsView";

async function setup() {
  const backend = installFakeBackend();
  const changed = vi.fn();
  const user = userEvent.setup();
  render(<SettingsView onChanged={changed} />);
  await screen.findByRole("combobox", { name: "Provider" });
  return { backend, changed, user };
}

describe("SettingsView with mock IPC", () => {
  it("shows effective settings and reasons for unsupported controls", async () => {
    await setup();
    expect(screen.getByRole("combobox", { name: "Provider" })).toHaveValue("mock");
    expect(screen.getByRole("combobox", { name: "Network mode" })).toHaveValue("local_only");
    expect(screen.getByRole("checkbox", { name: "Wake word" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete memory" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete memory" })).toHaveAccessibleDescription(
      /not available/,
    );
  });

  it("saves typed settings and refreshes the app after success", async () => {
    const { backend, changed, user } = await setup();
    await user.selectOptions(screen.getByLabelText("Provider"), "ollama");
    await user.type(screen.getByLabelText("Local model tag"), "qwen3:4b");
    await user.selectOptions(screen.getByLabelText("Network mode"), "tools_allowed");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Settings saved");
    expect(backend.calls.find((call) => call.cmd === "save_settings")?.args).toEqual({
      settings: {
        provider: "ollama",
        model_id: "qwen3:4b",
        network_mode: "tools_allowed",
        history_days: null,
      },
    });
    expect(changed).toHaveBeenCalledOnce();
  });

  it("requires explicit consent before applying a shorter retention period", async () => {
    const { backend, user } = await setup();
    await user.type(screen.getByRole("spinbutton"), "30");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(backend.calls.some((call) => call.cmd === "save_settings")).toBe(false);
    await user.keyboard("{Enter}");
    expect(screen.queryByRole("group", { name: "Confirm data deletion" })).not.toBeInTheDocument();
    expect(backend.calls.some((call) => call.cmd === "save_settings")).toBe(false);
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await user.click(screen.getByRole("button", { name: "Save and delete expired history" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Settings saved");
    expect(backend.settings.values.history_days).toBe(30);
  });

  it("defaults deletion to Cancel and Escape cancels without invoking deletion", async () => {
    const { backend, user } = await setup();
    await user.click(screen.getByRole("button", { name: "Clear history…" }));
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Delete all history" })).not.toBeInTheDocument();
    expect(backend.calls.some((call) => call.cmd === "clear_history")).toBe(false);
  });

  it("shows busy/refusal errors without claiming success or discarding the chat", async () => {
    const { backend, changed, user } = await setup();
    backend.busy = true;
    await user.click(screen.getByRole("button", { name: "Clear history…" }));
    await user.click(screen.getByRole("button", { name: "Delete all history" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stop the reply");
    expect(changed).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stop the reply");
    expect(changed).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("distinguishes native export cancellation, success and failure", async () => {
    const { backend, user } = await setup();
    const button = screen.getByRole("button", { name: /Export history/ });
    backend.exportPicked = false;
    await user.click(button);
    expect(await screen.findByRole("status")).toHaveTextContent("Export cancelled");
    backend.exportPicked = true;
    await user.click(button);
    expect(await screen.findByRole("status")).toHaveTextContent("History exported");
    backend.dataError = "File already exists";
    await user.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent("File already exists");
    expect(
      backend.calls
        .filter((call) => call.cmd === "export_data")
        .every((call) => Object.keys(call.args).length === 0),
    ).toBe(true);
  });

  it("renders redacted audit summaries as text and can refresh", async () => {
    const { backend, user } = await setup();
    backend.audit = [
      {
        id: "run",
        tool: "files.read",
        status: "denied",
        summary: "<script>untrusted()</script>",
        created_at: 1,
      },
    ];
    await user.click(screen.getByRole("button", { name: "Refresh audit" }));
    expect(await screen.findByRole("list", { name: "Audit entries" })).toHaveTextContent(
      "<script>untrusted()</script>",
    );
    expect(document.querySelector("script")).toBeNull();
  });

  it.each([null, "History deleted but WAL cleanup failed"])(
    "clears the mounted chat cache after deletion (cleanup error: %s)",
    async (cleanupError) => {
      const backend = installFakeBackend({
        conversations: [{ id: "c", title: "Private title", created_at: 1, updated_at: 1 }],
        messages: {
          c: [
            {
              id: "m",
              conversation_id: "c",
              role: "user",
              content: "Private contents",
              created_at: 1,
              stopped: false,
            },
          ],
        },
      });
      backend.cleanupError = cleanupError;
      const user = userEvent.setup();
      render(<App />);
      await user.click(await screen.findByRole("button", { name: "Private title" }));
      expect(await screen.findByText("Private contents")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Settings" }));
      await user.click(await screen.findByRole("button", { name: "Clear history…" }));
      await user.click(screen.getByRole("button", { name: "Delete all history" }));
      if (cleanupError) expect(await screen.findByRole("alert")).toHaveTextContent(cleanupError);
      else expect(await screen.findByText("Cleared 1 conversation(s).")).toBeInTheDocument();
      await waitFor(() => expect(backend.messages.size).toBe(0));
      expect(screen.queryByText("Private contents")).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Chat" }));
      expect(screen.queryByText("Private title")).not.toBeInTheDocument();
    },
  );
});

describe("Start at login (P5-T07)", () => {
  it("is off until the user turns it on, and shows the state the OS reports", async () => {
    const { backend, user } = await setup();
    const box = await screen.findByRole("checkbox", { name: "Start at login" });
    await waitFor(() => expect(box).toBeEnabled());
    expect(box).not.toBeChecked();
    expect(backend.calls.some((c) => c.cmd === "set_autostart")).toBe(false);
    expect(box).toHaveAccessibleDescription(/starts hidden in the menu bar/);

    await user.click(box);
    await waitFor(() => expect(box).toBeChecked());
    expect(backend.calls.find((c) => c.cmd === "set_autostart")?.args).toEqual({ enabled: true });
    expect(backend.autostart.enabled).toBe(true);
    await user.click(box);
    await waitFor(() => expect(box).not.toBeChecked());
  });

  it("is disabled with the reason when it can't be used", async () => {
    const backend = installFakeBackend();
    backend.autostart = {
      enabled: null,
      availability: { status: "unavailable", reason: "Available in installed release builds only." },
    };
    render(<SettingsView onChanged={() => undefined} />);
    const box = await screen.findByRole("checkbox", { name: "Start at login" });
    await waitFor(() =>
      expect(box).toHaveAccessibleDescription("Available in installed release builds only."),
    );
    expect(box).toBeDisabled();
    expect(box).not.toBeChecked();
  });

  it("an OS refusal is shown and the box keeps the real state", async () => {
    const { backend, user } = await setup();
    backend.autostartError = "the OS did not apply the requested start-at-login setting";
    const box = await screen.findByRole("checkbox", { name: "Start at login" });
    await waitFor(() => expect(box).toBeEnabled());
    await user.click(box);
    expect(await screen.findByText(/did not apply/)).toBeInTheDocument();
    expect(box).not.toBeChecked();
  });
});

describe("Model picker: provider check and installed models", () => {
  const installed: ProviderCheck = {
    reachable: true,
    detail: null,
    models: [
      {
        id: "bonsai-2:4b",
        size_bytes: 1_600_000_000,
        reviewed: true,
        display_name: "Bonsai 2 4B",
        license: "Apache-2.0",
      },
      {
        id: "llama-custom:7b",
        size_bytes: 4_100_000_000,
        reviewed: false,
        display_name: null,
        license: null,
      },
    ],
    hidden_cloud_models: 1,
    suggested: "bonsai-2:4b",
  };

  async function renderWith(check: ProviderCheck | undefined, model: string | null) {
    const backend = installFakeBackend(check ? { providerCheck: check } : {});
    backend.settings.values = { ...backend.settings.values, provider: "ollama", model_id: model };
    const user = userEvent.setup();
    render(<SettingsView onChanged={() => undefined} />);
    await screen.findByRole("combobox", { name: "Provider" });
    return { backend, user };
  }

  it("lists the installed models, auto-selects the suggested one, and saves it only on Save", async () => {
    const { backend, user } = await renderWith(installed, null);
    const picker = await screen.findByRole("combobox", { name: "Local model" });
    expect(screen.getByText("Ollama is running · 2 local models installed")).toBeInTheDocument();
    expect(picker).toHaveValue("bonsai-2:4b");
    expect(
      screen.getByRole("option", { name: /bonsai-2:4b — Bonsai 2 4B · 1\.6 GB · reviewed/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /llama-custom:7b — 4\.1 GB · not reviewed by Loki/ }),
    ).toBeInTheDocument();
    expect(screen.getByText(/1 cloud model is hidden/)).toBeInTheDocument();
    expect(screen.getByText(/selected it\. Save settings to use it/)).toBeInTheDocument();
    expect(backend.calls.some((c) => c.cmd === "save_settings")).toBe(false);

    await user.selectOptions(picker, "llama-custom:7b");
    await user.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "save_settings")?.args).toMatchObject({
        settings: { provider: "ollama", model_id: "llama-custom:7b" },
      }),
    );
  });

  it("keeps the saved model when it is installed", async () => {
    await renderWith({ ...installed, suggested: "llama-custom:7b" }, "llama-custom:7b");
    expect(await screen.findByRole("combobox", { name: "Local model" })).toHaveValue("llama-custom:7b");
    expect(screen.queryByText(/selected it/)).not.toBeInTheDocument();
  });

  it("replaces a saved model that isn't installed with the suggested one, and says so", async () => {
    await renderWith(installed, "qwen3:8b");
    expect(await screen.findByRole("combobox", { name: "Local model" })).toHaveValue("bonsai-2:4b");
    expect(screen.getByText(/isn't installed, so/)).toHaveTextContent(
      "qwen3:8b isn't installed, so bonsai-2:4b was selected instead. Save settings to use it.",
    );
  });

  it("when Ollama isn't reachable, explains why, allows typing a tag, and can check again", async () => {
    const { backend, user } = await renderWith(undefined, null);
    expect(await screen.findByText("Ollama isn't reachable")).toBeInTheDocument();
    expect(screen.getByText(/Can't reach Ollama/)).toBeInTheDocument();
    expect(screen.getByLabelText("Local model tag")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(backend.calls.filter((c) => c.cmd === "check_provider")).toHaveLength(2));
  });

  it("the mock provider needs no model and runs no check", async () => {
    const backend = installFakeBackend({ providerCheck: installed });
    render(<SettingsView onChanged={() => undefined} />);
    expect(await screen.findByText(/canned, clearly labelled replies/)).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Local model" })).not.toBeInTheDocument();
    expect(backend.calls.some((c) => c.cmd === "check_provider")).toBe(false);
  });
});
