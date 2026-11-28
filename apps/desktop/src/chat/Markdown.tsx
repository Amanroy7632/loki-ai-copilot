// A small Markdown renderer for assistant replies. Model output is untrusted: it only ever becomes
// React text nodes and a fixed set of elements. There is no HTML passthrough (no
// dangerouslySetInnerHTML), and links are shown with their address but never made clickable, so
// a reply can't navigate the app's webview. Partial input (a streaming reply) renders safely: an
// unclosed code fence is shown as an open code block.
import { useState, type ReactNode } from "react";

import { CheckIcon, CopyIcon } from "../icons";

type Block =
  | { kind: "code"; lang: string; text: string }
  | { kind: "heading"; level: 1 | 2 | 3; text: string }
  | { kind: "list"; ordered: boolean; items: string[] }
  | { kind: "quote"; text: string }
  | { kind: "paragraph"; text: string };

const FENCE = /^\s*```\s*([\w+-]*)\s*$/;
const HEADING = /^(#{1,3})\s+(.*)$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
const QUOTE = /^\s*>\s?(.*)$/;

/** Splits Markdown into blocks. Exported for tests. */
export function parseBlocks(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length > 0) blocks.push({ kind: "paragraph", text: paragraph.join("\n") });
    paragraph = [];
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      const body: string[] = [];
      i++;
      while (i < lines.length && !FENCE.test(lines[i] ?? "")) body.push(lines[i++] ?? "");
      blocks.push({ kind: "code", lang: fence[1] ?? "", text: body.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      const level = (heading[1]?.length ?? 1) as 1 | 2 | 3;
      blocks.push({ kind: "heading", level, text: heading[2] ?? "" });
      continue;
    }
    const bullet = BULLET.exec(line);
    const numbered = bullet ? null : NUMBERED.exec(line);
    if (bullet || numbered) {
      flush();
      const ordered = numbered !== null;
      const items: string[] = [];
      while (i < lines.length) {
        const match = (ordered ? NUMBERED : BULLET).exec(lines[i] ?? "");
        if (!match) break;
        items.push(match[1] ?? "");
        i++;
      }
      i--;
      blocks.push({ kind: "list", ordered, items });
      continue;
    }
    const quote = QUOTE.exec(line);
    if (quote) {
      flush();
      const body: string[] = [];
      while (i < lines.length) {
        const match = QUOTE.exec(lines[i] ?? "");
        if (!match) break;
        body.push(match[1] ?? "");
        i++;
      }
      i--;
      blocks.push({ kind: "quote", text: body.join("\n") });
      continue;
    }
    if (line.trim() === "") flush();
    else paragraph.push(line);
  }
  flush();
  return blocks;
}

// `code`, **bold**, *italic* / _italic_, [text](url). Everything else is literal text.
const INLINE = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(\*[^*\n]+\*|_[^_\n]+_)|(\[[^\]\n]+\]\([^)\s]+\))/g;

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const match of text.matchAll(INLINE)) {
    const index = match.index;
    if (index > last) out.push(text.slice(last, index));
    const [token] = match;
    if (match[1]) out.push(<code key={key++}>{token.slice(1, -1)}</code>);
    else if (match[2]) out.push(<strong key={key++}>{inline(token.slice(2, -2))}</strong>);
    else if (match[3]) out.push(<em key={key++}>{inline(token.slice(1, -1))}</em>);
    else if (match[4]) {
      const split = token.indexOf("](");
      const label = token.slice(1, split);
      const url = token.slice(split + 2, -1);
      out.push(
        <span key={key++} className="md-link" title={url}>
          {inline(label)} <span className="md-url">({url})</span>
        </span>,
      );
    }
    last = index + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/** Paragraph text keeps the model's single line breaks. */
function lines(text: string): ReactNode[] {
  return text
    .split("\n")
    .flatMap((line, i) => (i === 0 ? inline(line) : [<br key={`br${i}`} />, ...inline(line)]));
}

function CodeBlock({ lang, text }: { lang: string; text: string }) {
  const [copied, setCopied] = useState<"idle" | "done" | "failed">("idle");
  const copy = () => {
    const clipboard = typeof navigator === "undefined" ? undefined : navigator.clipboard;
    if (!clipboard) {
      setCopied("failed");
      return;
    }
    clipboard.writeText(text).then(
      () => setCopied("done"),
      () => setCopied("failed"),
    );
  };
  return (
    <figure className="md-code">
      <figcaption>
        <span>{lang || "code"}</span>
        <button type="button" className="ghost small" onClick={copy} aria-label="Copy code">
          {copied === "done" ? <CheckIcon /> : <CopyIcon />}
          <span>{copied === "done" ? "Copied" : copied === "failed" ? "Copy failed" : "Copy"}</span>
        </button>
      </figcaption>
      <pre>
        <code>{text}</code>
      </pre>
    </figure>
  );
}

/** Renders a reply. */
export function Markdown({ source }: { source: string }) {
  return (
    <div className="md">
      {parseBlocks(source).map((block, i) => {
        switch (block.kind) {
          case "code":
            return <CodeBlock key={i} lang={block.lang} text={block.text} />;
          case "heading": {
            const Tag = block.level === 1 ? "h3" : block.level === 2 ? "h4" : "h5";
            return <Tag key={i}>{inline(block.text)}</Tag>;
          }
          case "list": {
            const Tag = block.ordered ? "ol" : "ul";
            return (
              <Tag key={i}>
                {block.items.map((item, j) => (
                  <li key={j}>{inline(item)}</li>
                ))}
              </Tag>
            );
          }
          case "quote":
            return <blockquote key={i}>{lines(block.text)}</blockquote>;
          case "paragraph":
            return <p key={i}>{lines(block.text)}</p>;
        }
      })}
    </div>
  );
}
