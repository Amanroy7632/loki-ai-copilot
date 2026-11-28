// Replies are untrusted model output. The renderer must never turn them into HTML or links the
// webview could follow, and must survive partial (streaming) input.
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Markdown, parseBlocks } from "./Markdown";

describe("Markdown", () => {
  it("renders HTML in a reply as text, never as elements", () => {
    const { container } = render(
      <Markdown source={'<img src=x onerror="alert(1)"> <script>alert(2)</script> **<b>bold</b>**'} />,
    );
    expect(container.querySelector("img, script, b")).toBeNull();
    expect(container).toHaveTextContent(
      '<img src=x onerror="alert(1)"> <script>alert(2)</script> <b>bold</b>',
    );
  });

  it("shows links with their address but never makes them clickable", () => {
    const { container } = render(
      <Markdown source="See [the docs](https://example.com/x) and [js](javascript:void0)" />,
    );
    expect(container.querySelector("a")).toBeNull();
    expect(screen.getByText("(https://example.com/x)")).toBeInTheDocument();
    expect(screen.getByText("(javascript:void0)")).toBeInTheDocument();
  });

  it("renders paragraphs, emphasis, inline code, lists, headings and quotes", () => {
    const { container } = render(
      <Markdown
        source={[
          "## Plan",
          "First **bold** then *soft* and `code`.",
          "Second line.",
          "",
          "- one",
          "- two",
          "",
          "1. a",
          "2. b",
          "",
          "> quoted",
        ].join("\n")}
      />,
    );
    expect(screen.getByRole("heading", { name: "Plan" })).toBeInTheDocument();
    expect(container.querySelector("strong")).toHaveTextContent("bold");
    expect(container.querySelector("em")).toHaveTextContent("soft");
    expect(container.querySelector("p code")).toHaveTextContent("code");
    expect(container.querySelector("p br")).not.toBeNull();
    const lists = screen.getAllByRole("list");
    expect(lists.map((list) => list.tagName)).toEqual(["UL", "OL"]);
    expect(
      within(lists[0] as HTMLElement)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(["one", "two"]);
    expect(container.querySelector("blockquote")).toHaveTextContent("quoted");
  });

  it("renders code blocks verbatim with their language and a copy button", () => {
    const { container } = render(<Markdown source={"```rust\nfn main() { **not bold** }\n```"} />);
    expect(container.querySelector("pre code")).toHaveTextContent("fn main() { **not bold** }");
    expect(container.querySelector("strong")).toBeNull();
    expect(screen.getByText("rust")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy code" })).toBeInTheDocument();
  });

  it("shows an unclosed fence (a reply still streaming) as an open code block", () => {
    expect(parseBlocks("Here:\n```js\nconst a = 1;")).toEqual([
      { kind: "paragraph", text: "Here:" },
      { kind: "code", lang: "js", text: "const a = 1;" },
    ]);
  });

  it("keeps plain text exactly", () => {
    render(<Markdown source="Mock reply: hello" />);
    expect(screen.getByText("Mock reply: hello").tagName).toBe("P");
  });
});
