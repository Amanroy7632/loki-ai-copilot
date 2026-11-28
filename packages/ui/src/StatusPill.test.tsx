import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StatusPill } from "./StatusPill";

describe("StatusPill", () => {
  it("is a polite live region with a text label", () => {
    render(<StatusPill tone="good" label="Ready" />);
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveTextContent("Ready");
  });

  it("shows the detail when given", () => {
    render(<StatusPill tone="bad" label="Not ready" detail="Ollama is not running" />);
    expect(screen.getByText("Ollama is not running")).toBeInTheDocument();
  });

  it("does not rely on colour alone", () => {
    render(<StatusPill tone="bad" label="Not ready" />);
    expect(screen.getByRole("status")).toHaveTextContent("Not ready");
  });
});
