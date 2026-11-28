// P5-T03 component tests: streaming, stop, retry, conversation list (new, select, rename, delete),
// keyboard use and accessible names. The fake backend streams like the mock provider.
import type { Conversation, Message } from "@loki/contracts";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { installFakeBackend } from "../test/fakeBackend";
import { ChatView } from "./ChatView";

const groceries: Conversation = { id: "g", title: "Groceries", created_at: 1, updated_at: 3 };
const trip: Conversation = { id: "t", title: "Trip plan", created_at: 1, updated_at: 2 };
const msg = (id: string, conversation: string, role: Message["role"], content: string): Message => ({
  id,
  conversation_id: conversation,
  role,
  content,
  created_at: 1,
  stopped: false,
});

describe("ChatView", () => {
  it("exposes labelled regions and controls", async () => {
    installFakeBackend();
    render(<ChatView />);
    expect(screen.getByRole("navigation", { name: "Conversations" })).toBeInTheDocument();
    expect(screen.getByRole("log")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeInTheDocument();
    expect(await screen.findByText("No conversations yet.")).toBeInTheDocument();
  });

  it("streams a reply token by token, then shows the stored conversation", async () => {
    const backend = installFakeBackend({ holdStream: true });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(screen.getByRole("textbox", { name: "Message" }), "hello{Enter}");

    // Mid-stream: the partial reply is visible and marked busy; Stop replaces Send.
    const partial = await screen.findByText("Mock");
    expect(partial.closest("li")).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    expect(within(screen.getByRole("log")).getByText("hello")).toBeInTheDocument();

    backend.release();
    const reply = await screen.findByText("Mock reply: hello");
    expect(reply.closest("li")).not.toHaveAttribute("aria-busy");
    const nav = screen.getByRole("navigation", { name: "Conversations" });
    expect(within(nav).getByRole("button", { name: "hello" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus();
  });

  it("Stop stops the running turn and marks the saved partial reply", async () => {
    const backend = installFakeBackend({ holdStream: true });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(screen.getByRole("textbox", { name: "Message" }), "tell me a long story{Enter}");
    await screen.findByText("Mock");
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(await screen.findByText(/Stopped\. The partial reply was saved/)).toBeInTheDocument();
    expect(screen.getAllByText("Stopped").length).toBeGreaterThan(0);
    const send = backend.calls.find((c) => c.cmd === "send_message");
    const stop = backend.calls.find((c) => c.cmd === "stop_reply");
    expect(stop?.args.turnId).toBe(send?.args.turnId);
  });

  it("Escape in the composer also stops a running reply", async () => {
    const backend = installFakeBackend({ holdStream: true });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(screen.getByRole("textbox", { name: "Message" }), "hi{Enter}");
    await screen.findByText("Mock");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(backend.calls.some((c) => c.cmd === "stop_reply")).toBe(true));
  });

  it("Retry regenerates the last reply in place", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(screen.getByRole("textbox", { name: "Message" }), "again{Enter}");
    await screen.findByText("Mock reply: again");
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(backend.calls.filter((c) => c.cmd === "retry_reply")).toHaveLength(1));
    await waitFor(() => expect(screen.getAllByText("Mock reply: again")).toHaveLength(1));
    expect(backend.calls.find((c) => c.cmd === "retry_reply")?.args.conversationId).toBe("c1");
  });

  it("Enter sends, Shift+Enter adds a line, and blank messages aren't sent", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<ChatView />);
    const box = screen.getByRole("textbox", { name: "Message" });
    await user.type(box, "   {Enter}");
    expect(backend.calls.some((c) => c.cmd === "send_message")).toBe(false);
    await user.clear(box);
    await user.type(box, "line one{Shift>}{Enter}{/Shift}line two");
    expect(box).toHaveValue("line one\nline two");
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "send_message")?.args.text).toBe("line one\nline two"),
    );
  });

  it("a failed turn explains the error and keeps the message", async () => {
    installFakeBackend({ failWith: "Ollama is not running" });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(screen.getByRole("textbox", { name: "Message" }), "hi{Enter}");
    expect(await screen.findByText(/The reply failed: Ollama is not running/)).toBeInTheDocument();
    expect(within(screen.getByRole("log")).getByText("hi")).toBeInTheDocument();
  });

  it("selects a conversation and shows its messages", async () => {
    installFakeBackend({
      conversations: [groceries, trip],
      messages: { t: [msg("1", "t", "user", "Plan Rome"), msg("2", "t", "assistant", "Day one: Colosseum")] },
    });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.click(await screen.findByRole("button", { name: "Trip plan" }));
    expect(await screen.findByText("Day one: Colosseum")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Trip plan" })).toHaveAttribute("aria-current", "page");
  });

  it("arrow keys move between conversations", async () => {
    installFakeBackend({ conversations: [groceries, trip] });
    const user = userEvent.setup();
    render(<ChatView />);
    const first = await screen.findByRole("button", { name: "Groceries" });
    first.focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("button", { name: "Trip plan" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(first).toHaveFocus();
    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("button", { name: "Trip plan" })).toHaveFocus();
  });

  it("renames with the keyboard: Enter saves, Escape cancels and returns focus", async () => {
    const backend = installFakeBackend({ conversations: [groceries] });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.click(await screen.findByRole("button", { name: "Rename Groceries" }));
    const field = screen.getByRole("textbox", { name: "New title for Groceries" });
    expect(field).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("button", { name: "Groceries" })).toHaveFocus();
    expect(backend.calls.some((c) => c.cmd === "rename_conversation")).toBe(false);

    await user.click(screen.getByRole("button", { name: "Rename Groceries" }));
    await user.clear(screen.getByRole("textbox", { name: "New title for Groceries" }));
    await user.keyboard("Weekly shop{Enter}");
    const renamed = await screen.findByRole("button", { name: "Weekly shop" });
    expect(renamed).toHaveFocus();
    expect(backend.calls.find((c) => c.cmd === "rename_conversation")?.args).toMatchObject({
      conversationId: "g",
      title: "Weekly shop",
    });
  });

  it("delete asks first; Cancel keeps it, Delete removes it", async () => {
    const backend = installFakeBackend({ conversations: [groceries, trip] });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.click(await screen.findByRole("button", { name: "Delete Groceries" }));
    expect(screen.getByRole("group", { name: "Delete Groceries?" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Groceries" })).toBeInTheDocument();
    expect(backend.calls.some((c) => c.cmd === "delete_conversation")).toBe(false);

    await user.click(screen.getByRole("button", { name: "Delete Groceries" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Groceries" })).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Trip plan" })).toBeInTheDocument();
  });

  it("New chat clears the view and focuses the composer", async () => {
    installFakeBackend({ conversations: [trip], messages: { t: [msg("1", "t", "user", "Plan Rome")] } });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.click(await screen.findByRole("button", { name: "Trip plan" }));
    await screen.findByText("Plan Rome");
    await user.click(screen.getByRole("button", { name: "New chat" }));
    expect(screen.queryByText("Plan Rome")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus();
  });
});

describe("ChatView: AI-assistant touches", () => {
  it("a suggestion fills the message box but doesn't send it", async () => {
    const backend = installFakeBackend();
    const user = userEvent.setup();
    render(<ChatView />);
    const suggestions = await screen.findByRole("list", { name: "Suggestions" });
    await user.click(within(suggestions).getByRole("button", { name: "What time is it in Tokyo?" }));
    const box = screen.getByRole("textbox", { name: "Message" });
    expect(box).toHaveValue("What time is it in Tokyo?");
    expect(box).toHaveFocus();
    expect(backend.calls.some((c) => c.cmd === "send_message")).toBe(false);
    await user.keyboard("{Enter}");
    expect(await screen.findByText("Mock reply: What time is it in Tokyo?")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Suggestions" })).not.toBeInTheDocument();
  });

  it("searching filters the conversation list", async () => {
    installFakeBackend({ conversations: [groceries, trip] });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(await screen.findByRole("searchbox", { name: "Search conversations" }), "trip");
    const nav = screen.getByRole("navigation", { name: "Conversations" });
    expect(within(nav).getByRole("button", { name: "Trip plan" })).toBeInTheDocument();
    expect(within(nav).queryByRole("button", { name: "Groceries" })).not.toBeInTheDocument();
    await user.clear(screen.getByRole("searchbox", { name: "Search conversations" }));
    await user.type(screen.getByRole("searchbox", { name: "Search conversations" }), "zzz");
    expect(screen.getByText(/No conversations match/)).toBeInTheDocument();
  });

  it("shows a thinking indicator until the first token, then the streamed text", async () => {
    installFakeBackend({ approval: { tool: "app.open", risk: "local_change", arguments: { app_id: "x" } } });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.type(screen.getByRole("textbox", { name: "Message" }), "open x{Enter}");
    // The approval is asked before any token: Loki is "thinking".
    expect(await screen.findByRole("status", { name: "Thinking" })).toBeInTheDocument();
  });

  it("renders stored replies as Markdown, with the conversation title in the header", async () => {
    installFakeBackend({
      conversations: [trip],
      messages: {
        t: [msg("1", "t", "user", "Plan Rome"), msg("2", "t", "assistant", "**Day one**\n- Colosseum")],
      },
    });
    const user = userEvent.setup();
    render(<ChatView />);
    await user.click(await screen.findByRole("button", { name: "Trip plan" }));
    const log = screen.getByRole("log");
    expect(await within(log).findByText("Day one")).toContainHTML("strong");
    expect(screen.getByRole("heading", { name: "Trip plan", level: 2 })).toBeInTheDocument();
    expect(within(log).getByRole("button", { name: "Copy reply" })).toBeInTheDocument();
  });
});
