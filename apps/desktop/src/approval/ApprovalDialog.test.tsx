// P5-T04 approval dialog. Acceptance: approving binds to the shown arguments, and an expired
// approval is refused. Also: every argument shown, plain-language risk, countdown, focus on Deny,
// and Enter can't approve a Sensitive action without the user moving focus to Approve.
import type { ApprovalPreview, JsonValue } from "@loki/contracts";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ChatView } from "../chat/ChatView";
import { installFakeBackend } from "../test/fakeBackend";
import { ApprovalDialog, showValue } from "./ApprovalDialog";

const ARGS: JsonValue = {
  app_id: "com.example.calculator",
  flags: ["--new-window", "--quiet"],
  label: "Calc\u202Etxt.exe",
};

async function startTurn(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByRole("textbox", { name: "Message" }), "open the calculator{Enter}");
  return screen.findByRole("alertdialog", { name: "Allow app.open?" });
}

function preview(overrides: Partial<ApprovalPreview> = {}): ApprovalPreview {
  return {
    id: "ap-1",
    request_id: "rq-1",
    tool: "messages.send",
    risk: "sensitive",
    arguments: { to: "Sam", text: "On my way" },
    expires_at: Date.now() + 120_000,
    ...overrides,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ApprovalDialog in the chat", () => {
  it("shows the tool, the risk in plain language, every argument and the countdown; focus on Deny", async () => {
    installFakeBackend({ approval: { tool: "app.open", risk: "local_change", arguments: ARGS } });
    const user = userEvent.setup();
    render(<ChatView />);
    const dialog = await startTurn(user);

    expect(dialog).toHaveAccessibleDescription(/changes something on this computer/);
    const args = within(dialog);
    expect(args.getByText("app_id")).toBeInTheDocument();
    expect(args.getByText('"com.example.calculator"')).toBeInTheDocument();
    expect(args.getByText("flags")).toBeInTheDocument();
    expect(args.getByText(/"--new-window",\s+"--quiet"/)).toBeInTheDocument();
    // The right-to-left override can't disguise the value: it is shown, not applied.
    expect(args.getByText('"Calc\\u{202e}txt.exe"')).toBeInTheDocument();
    expect(args.getByRole("timer")).toHaveTextContent(/Expires in (2:00|1:5\d)/);
    expect(args.getByRole("button", { name: "Deny" })).toHaveFocus();
  });

  it("Approve sends back exactly the shown arguments for that approval, and the tool runs", async () => {
    const backend = installFakeBackend({
      approval: { tool: "app.open", risk: "local_change", arguments: ARGS },
    });
    const user = userEvent.setup();
    render(<ChatView />);
    const dialog = await startTurn(user);
    await user.click(within(dialog).getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(backend.answers).toHaveLength(1));
    const [answer] = backend.answers;
    expect(answer).toMatchObject({ approved: true, accepted: true });
    expect(answer?.shownArguments).toEqual(ARGS);
    expect(await screen.findByText("app.open: succeeded")).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("Enter right after the dialog opens denies, and nothing runs", async () => {
    const backend = installFakeBackend({
      approval: { tool: "app.open", risk: "local_change", arguments: ARGS },
    });
    const user = userEvent.setup();
    render(<ChatView />);
    await startTurn(user);
    await user.keyboard("{Enter}");

    await waitFor(() => expect(backend.answers).toMatchObject([{ approved: false }]));
    expect(await screen.findByText(/app\.open: cancelled/)).toBeInTheDocument();
  });

  it("Escape denies", async () => {
    const backend = installFakeBackend({
      approval: { tool: "app.open", risk: "local_change", arguments: ARGS },
    });
    const user = userEvent.setup();
    render(<ChatView />);
    await startTurn(user);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(backend.answers).toMatchObject([{ approved: false }]));
  });

  it("an approval left until it expires is refused: nothing is sent, the tool never runs", async () => {
    const backend = installFakeBackend({
      approval: { tool: "app.open", risk: "local_change", arguments: ARGS, ttlMs: 800 },
    });
    const user = userEvent.setup();
    render(<ChatView />);
    await startTurn(user);
    // The runtime cancels the request at expiry, which also closes the dialog.
    expect(await screen.findByText(/app\.open: cancelled/, {}, { timeout: 3_000 })).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(backend.answers).toHaveLength(0);
  });
});

describe("ApprovalDialog keyboard and refusal rules", () => {
  it("an expired dialog disables both answers and says it won't run", async () => {
    const onAnswer = vi.fn(() => Promise.resolve(true));
    const user = userEvent.setup();
    render(
      <ApprovalDialog
        approval={preview({ risk: "local_change", expires_at: Date.now() + 300 })}
        onAnswer={onAnswer}
      />,
    );
    expect(await screen.findByText("Expired", {}, { timeout: 2_000 })).toBeInTheDocument();
    const approve = screen.getByRole("button", { name: "Approve" });
    expect(approve).toBeDisabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(/expired, so it won't run/);
    await user.click(approve);
    expect(onAnswer).not.toHaveBeenCalled();
  });

  it("Sensitive: Enter doesn't approve unless the user moved focus to Approve", async () => {
    const onAnswer = vi.fn(() => Promise.resolve(true));
    const user = userEvent.setup();
    const approval = preview();
    render(<ApprovalDialog approval={approval} onAnswer={onAnswer} />);
    expect(screen.getByRole("alertdialog")).toHaveAccessibleDescription(/Check every detail/);

    // Focus that landed on Approve without the user moving it doesn't count.
    const approve = screen.getByRole("button", { name: "Approve sensitive action" });
    approve.focus();
    await user.keyboard("{Enter}");
    expect(onAnswer).not.toHaveBeenCalled();

    // Tab wraps to Deny, Tab again reaches Approve: now Enter is an explicit choice.
    await user.tab();
    expect(screen.getByRole("button", { name: "Deny" })).toHaveFocus();
    await user.tab();
    expect(approve).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onAnswer).toHaveBeenCalledExactlyOnceWith(true, approval.arguments);
  });

  it("Tab and Shift+Tab stay inside the dialog", async () => {
    const user = userEvent.setup();
    render(
      <ApprovalDialog approval={preview({ risk: "local_change" })} onAnswer={() => Promise.resolve(true)} />,
    );
    const deny = screen.getByRole("button", { name: "Deny" });
    const approve = screen.getByRole("button", { name: "Approve" });
    expect(deny).toHaveFocus();
    await user.tab({ shift: true });
    expect(approve).toHaveFocus();
    await user.tab();
    expect(deny).toHaveFocus();
  });

  it("a held-down key (auto-repeat) does nothing", () => {
    render(<ApprovalDialog approval={preview()} onAnswer={() => Promise.resolve(true)} />);
    const deny = screen.getByRole("button", { name: "Deny" });
    const notCancelled = fireEvent.keyDown(deny, { key: "Enter", repeat: true });
    expect(notCancelled).toBe(false);
  });

  it("approving at the expiry instant sends nothing", async () => {
    const onAnswer = vi.fn(() => Promise.resolve(true));
    const user = userEvent.setup();
    const approval = preview({ risk: "local_change", expires_at: Date.now() + 60_000 });
    render(<ApprovalDialog approval={approval} onAnswer={onAnswer} />);
    vi.spyOn(Date, "now").mockReturnValue(approval.expires_at);
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(onAnswer).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(/expired, so it won't run/);
  });

  it("an answer the runtime refuses says that nothing ran", async () => {
    const user = userEvent.setup();
    render(
      <ApprovalDialog approval={preview({ risk: "network" })} onAnswer={() => Promise.resolve(false)} />,
    );
    expect(screen.getByRole("alertdialog")).toHaveAccessibleDescription(/sends a request to the internet/);
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByText(/Loki refused this answer .* nothing ran/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
  });

  it("shows a tool with no arguments as such", () => {
    render(<ApprovalDialog approval={preview({ arguments: {} })} onAnswer={() => Promise.resolve(true)} />);
    expect(screen.getByText("No arguments.")).toBeInTheDocument();
  });
});

describe("showValue", () => {
  it("shows values as JSON with nothing elided and hidden characters visible", () => {
    expect(showValue("a b ")).toBe('"a b "');
    expect(showValue("line\nbreak")).toBe('"line\\nbreak"');
    expect(showValue("zero\u200Bwidth\uFEFF")).toBe('"zero\\u{200b}width\\u{feff}"');
    expect(showValue({ n: 1, nested: [true, null] })).toBe(
      '{\n  "n": 1,\n  "nested": [\n    true,\n    null\n  ]\n}',
    );
    expect(showValue("x".repeat(5_000))).toHaveLength(5_002);
  });
});
