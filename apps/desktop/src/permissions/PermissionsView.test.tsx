// P5-T05 permissions screen: every capability with its status and reason, grant and revoke,
// unsupported capabilities shown as unavailable, and allowed folders through the native picker.
// The runtime test `mock_revoking_a_grant_immediately_denies_the_next_call` covers the acceptance
// criterion end to end; these check the screen calls the runtime and shows what it reports.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { installFakeBackend } from "../test/fakeBackend";
import { PermissionsView } from "./PermissionsView";

const item = (name: string) => screen.getByRole("listitem", { name });

describe("PermissionsView", () => {
  it("lists every capability with its status, and the reason when it isn't available", async () => {
    installFakeBackend();
    render(<PermissionsView />);
    const clock = await screen.findByRole("listitem", { name: "Clock" });
    expect(within(clock).getByText("Available")).toBeInTheDocument();
    expect(within(clock).getByText("Not allowed")).toBeInTheDocument();

    const internet = item("Internet");
    expect(within(internet).getByText("Not available")).toBeInTheDocument();
    expect(within(internet).getByText("Web tools arrive in P7.")).toBeInTheDocument();
    const allow = within(internet).getByRole("button", { name: "Allow Internet" });
    expect(allow).toBeDisabled();
    expect(allow).toHaveAccessibleDescription("Web tools arrive in P7.");

    const mic = item("Microphone");
    expect(within(mic).getByText("Needs an OS permission")).toBeInTheDocument();
    expect(within(mic).getByText(/System Settings/)).toBeInTheDocument();
    expect(screen.getAllByRole("listitem").length).toBeGreaterThanOrEqual(8);
  });

  it("Allow grants the capability, and Revoke takes it back at once", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("button", { name: "Allow Clock" }));
    expect(await within(item("Clock")).findByText("Allowed")).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "grant_permission")?.args).toEqual({
      capability: "clock",
      scope: "*",
    });

    await user.click(within(item("Clock")).getByRole("button", { name: "Revoke Clock" }));
    expect(await within(item("Clock")).findByText("Not allowed")).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "revoke_permission")?.args).toEqual({
      capability: "clock",
      scope: "*",
    });
    // The screen shows what the runtime stores, not what it assumed.
    expect(backend.permissions.find((r) => r.capability === "clock")?.scopes).toEqual([]);
  });

  it("shows a runtime refusal instead of pretending the grant worked", async () => {
    const backend = installFakeBackend({ roots: [{ id: "docs", path: "/Users/me/Documents" }] });
    const user = userEvent.setup();
    render(<PermissionsView />);
    const allow = await screen.findByRole("button", { name: "Allow reading docs" });
    backend.roots = []; // removed elsewhere (e.g. the CLI) after the screen loaded
    await user.click(allow);
    expect(await screen.findByRole("alert")).toHaveTextContent(/scope must be/);
    expect(screen.getByText("No folders yet.")).toBeInTheDocument();
    expect(within(item("Read files")).getByText("Not allowed")).toBeInTheDocument();
  });

  it("adds a folder through the native picker, then allows and revokes reading it", async () => {
    const backend = installFakeBackend({ pickFolder: "/Users/me/Documents" });
    const user = userEvent.setup();
    render(<PermissionsView />);
    expect(await screen.findByText("No folders yet.")).toBeInTheDocument();
    await user.type(screen.getByRole("textbox", { name: "Short name" }), "docs");
    await user.click(screen.getByRole("button", { name: "Choose folder…" }));

    expect(await screen.findByText("/Users/me/Documents")).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "add_root")?.args).toEqual({ id: "docs" });
    expect(screen.getByRole("status")).toHaveTextContent(/Added “docs”/);
    expect(screen.getByRole("textbox", { name: "Short name" })).toHaveValue("");

    await user.click(screen.getByRole("button", { name: "Allow reading docs" }));
    expect(await screen.findByText("Reading allowed")).toBeInTheDocument();
    expect(within(item("Read files")).getByText("Allowed in 1 folder(s)")).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "grant_permission")?.args).toEqual({
      capability: "file_read",
      scope: "docs",
    });

    await user.click(screen.getByRole("button", { name: "Revoke reading docs" }));
    expect(await screen.findByText("Reading not allowed")).toBeInTheDocument();
  });

  it("a cancelled picker adds nothing", async () => {
    const backend = installFakeBackend({ pickFolder: null });
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.type(await screen.findByRole("textbox", { name: "Short name" }), "docs");
    await user.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(await screen.findByText("No folder was chosen.")).toBeInTheDocument();
    expect(backend.roots).toEqual([]);
    expect(screen.getByText("No folders yet.")).toBeInTheDocument();
  });

  it("an invalid folder name is refused and kept for editing", async () => {
    installFakeBackend({ pickFolder: "/tmp/x" });
    const user = userEvent.setup();
    render(<PermissionsView />);
    const name = await screen.findByRole("textbox", { name: "Short name" });
    await user.type(name, "My Docs");
    await user.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/folder names are 1-32/);
    expect(name).toHaveValue("My Docs");
  });

  it("removing a folder asks first and revokes its access", async () => {
    const backend = installFakeBackend({
      roots: [{ id: "docs", path: "/Users/me/Documents" }],
      permissions: [{ capability: "file_read", status: { status: "supported" }, scopes: ["docs"] }],
    });
    const user = userEvent.setup();
    render(<PermissionsView />);
    await user.click(await screen.findByRole("button", { name: "Remove folder docs" }));
    const confirm = screen.getByRole("group", { name: "Remove folder docs?" });
    expect(within(confirm).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(backend.calls.some((c) => c.cmd === "remove_root")).toBe(false);

    await user.click(screen.getByRole("button", { name: "Remove folder docs" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(await screen.findByText("No folders yet.")).toBeInTheDocument();
    await waitFor(() => expect(backend.permissions[0]?.scopes).toEqual([]));
    expect(within(item("Read files")).getByText("Not allowed")).toBeInTheDocument();
  });

  it("a folder-wide file grant from the CLI is shown and can be revoked", async () => {
    const backend = installFakeBackend({
      permissions: [{ capability: "file_read", status: { status: "supported" }, scopes: ["*"] }],
    });
    const user = userEvent.setup();
    render(<PermissionsView />);
    const files = await screen.findByRole("listitem", { name: "Read files" });
    expect(within(files).getByText("Allowed in every folder")).toBeInTheDocument();
    await user.click(within(files).getByRole("button", { name: "Revoke Read files" }));
    await waitFor(() => expect(backend.permissions[0]?.scopes).toEqual([]));
  });
});
