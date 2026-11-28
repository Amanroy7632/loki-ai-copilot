// Privacy & Permissions (redesigned): every capability is a switch; the microphone goes through
// the real macOS prompt (played by the fake backend) and can be tested; turning anything off
// revokes it. The runtime tests (`os_permissions.rs`, `permissions.rs`) cover the same rules
// against the real runtime: e.g. `mock_revoking_a_grant_immediately_denies_the_next_call`.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { installFakeBackend } from "../test/fakeBackend";
import { PermissionsView } from "./PermissionsView";

const row = (name: string) => screen.getByRole("listitem", { name });
const toggle = (name: string) => screen.getByRole("switch", { name });

describe("PermissionsView", () => {
  it("groups every capability like the OS settings app, with reasons for what isn't available", async () => {
    installFakeBackend();
    render(<PermissionsView />);
    await screen.findByRole("switch", { name: "Clock" });
    expect(screen.getByRole("heading", { name: "Privacy & Permissions" })).toBeInTheDocument();
    for (const group of ["Privacy", "Tools", "Coming later"]) {
      expect(screen.getByRole("list", { name: group })).toBeInTheDocument();
    }
    expect(toggle("Clock")).toHaveAttribute("aria-checked", "false");
    const internet = toggle("Internet");
    expect(internet).toBeDisabled();
    expect(internet).toHaveAccessibleDescription("Web tools arrive in P7.");
    expect(within(row("Microphone")).getByText(/macOS will ask you/)).toBeInTheDocument();
  });

  it("a switch turns a capability on and off, and the screen shows what the runtime stored", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("switch", { name: "Clock" }));
    await waitFor(() => expect(toggle("Clock")).toHaveAttribute("aria-checked", "true"));
    expect(backend.calls.find((c) => c.cmd === "enable_capability")?.args).toEqual({ capability: "clock" });
    expect(screen.getByRole("status")).toHaveTextContent("Clock is on.");

    await user.click(toggle("Clock"));
    await waitFor(() => expect(toggle("Clock")).toHaveAttribute("aria-checked", "false"));
    expect(backend.calls.find((c) => c.cmd === "disable_capability")?.args).toEqual({ capability: "clock" });
    expect(backend.permissions.find((r) => r.capability === "clock")?.scopes).toEqual([]);
  });

  it("turning the microphone on shows the macOS prompt; Allow turns it on and offers a test", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("switch", { name: "Microphone" }));
    await waitFor(() => expect(toggle("Microphone")).toHaveAttribute("aria-checked", "true"));
    expect(backend.prompts).toEqual(["microphone"]);
    expect(
      within(row("Microphone")).getByText(/macOS allows Loki to use the microphone/),
    ).toBeInTheDocument();

    await user.click(within(row("Microphone")).getByRole("button", { name: /Test microphone/ }));
    expect(
      await screen.findByText(/Your microphone works\. Nothing was recorded or saved\./),
    ).toBeInTheDocument();
    const meter = screen.getByRole("meter", { name: "Microphone level" });
    expect((meter as HTMLMeterElement).value).toBe(0);
  });

  it("if macOS says Don't Allow, the switch stays off and Open System Settings is offered", async () => {
    const backend = installFakeBackend({
      permissionAnswers: {
        microphone: { status: "denied", reason: "Microphone access is off for Loki in System Settings." },
      },
    });
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("switch", { name: "Microphone" }));
    expect(await screen.findByText(/macOS didn't allow microphone access/)).toBeInTheDocument();
    expect(toggle("Microphone")).toHaveAttribute("aria-checked", "false");
    expect(toggle("Microphone")).toBeDisabled();
    expect(
      within(row("Microphone")).queryByRole("button", { name: /Test microphone/ }),
    ).not.toBeInTheDocument();

    await user.click(within(row("Microphone")).getByRole("button", { name: "Open System Settings" }));
    await waitFor(() => expect(backend.openedSettings).toEqual(["microphone"]));
  });

  it("turning the microphone off removes the test and revokes Loki's grant", async () => {
    const backend = installFakeBackend({
      permissions: [{ capability: "microphone", status: { status: "supported" }, scopes: ["*"] }],
    });
    const user = userEvent.setup();
    render(<PermissionsView />);
    expect(await screen.findByRole("button", { name: /Test microphone/ })).toBeInTheDocument();
    await user.click(toggle("Microphone"));
    await waitFor(() => expect(toggle("Microphone")).toHaveAttribute("aria-checked", "false"));
    expect(screen.queryByRole("button", { name: /Test microphone/ })).not.toBeInTheDocument();
    expect(backend.permissions[0]?.scopes).toEqual([]);
  });

  it("adds a folder through the native picker, then turns reading on and off", async () => {
    const backend = installFakeBackend({ pickFolder: "/Users/me/Documents" });
    const user = userEvent.setup();
    render(<PermissionsView />);
    expect(await screen.findByText("No folders yet.")).toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "Short name" }), "docs");
    await user.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(await screen.findByText("/Users/me/Documents")).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "add_root")?.args).toEqual({ id: "docs" });

    await user.click(toggle("Read docs"));
    await waitFor(() => expect(toggle("Read docs")).toHaveAttribute("aria-checked", "true"));
    expect(within(row("Files and folders")).getByText("Reading allowed in 1 folder.")).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "grant_permission")?.args).toEqual({
      capability: "file_read",
      scope: "docs",
    });
    await user.click(toggle("Read docs"));
    await waitFor(() => expect(toggle("Read docs")).toHaveAttribute("aria-checked", "false"));
  });

  it("a cancelled picker adds nothing; an invalid name is refused and kept", async () => {
    const backend = installFakeBackend({ pickFolder: null });
    const user = userEvent.setup();
    render(<PermissionsView />);
    const name = await screen.findByRole("textbox", { name: "Short name" });
    await user.type(name, "docs");
    await user.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(await screen.findByText("No folder was chosen.")).toBeInTheDocument();
    expect(backend.roots).toEqual([]);

    await user.clear(name);
    await user.type(name, "My Docs");
    await user.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/folder names are 1-32/);
    expect(name).toHaveValue("My Docs");
  });

  it("removing a folder asks first (focus on Cancel) and revokes its access", async () => {
    const backend = installFakeBackend({
      roots: [{ id: "docs", path: "/Users/me/Documents" }],
      permissions: [{ capability: "file_read", status: { status: "supported" }, scopes: ["docs"] }],
    });
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("button", { name: "Remove folder docs" }));
    const confirm = screen.getByRole("group", { name: "Remove folder docs?" });
    expect(within(confirm).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(within(confirm).getByRole("button", { name: "Remove" }));
    expect(await screen.findByText("No folders yet.")).toBeInTheDocument();
    await waitFor(() => expect(backend.permissions[0]?.scopes).toEqual([]));
  });

  it("shows a runtime refusal instead of pretending the change worked", async () => {
    const backend = installFakeBackend({ roots: [{ id: "docs", path: "/Users/me/Documents" }] });
    const user = userEvent.setup();
    render(<PermissionsView />);
    const read = await screen.findByRole("switch", { name: "Read docs" });
    backend.roots = []; // removed elsewhere (e.g. the CLI) after the screen loaded
    await user.click(read);
    expect(await screen.findByRole("alert")).toHaveTextContent(/scope must be/);
    expect(screen.getByText("No folders yet.")).toBeInTheDocument();
  });

  it("a folder-wide file grant from the CLI is shown and can be limited", async () => {
    const backend = installFakeBackend({
      permissions: [{ capability: "file_read", status: { status: "supported" }, scopes: ["*"] }],
    });
    const user = userEvent.setup();
    render(<PermissionsView />);
    expect(await screen.findByText(/Allowed in every folder/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Limit to the folders below" }));
    await waitFor(() => expect(backend.permissions[0]?.scopes).toEqual([]));
  });

  it("links to Files and Folders in System Settings", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("button", { name: "Files and Folders in System Settings" }));
    await waitFor(() => expect(backend.openedSettings).toEqual(["file_read"]));
  });
});
