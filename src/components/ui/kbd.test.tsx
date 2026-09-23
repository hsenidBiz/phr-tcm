// Keyboard hints: one XiodUI key cap per key, read out as words - a screen
// reader reads glyphs like the shift arrow as nonsense.

import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Kbd } from "./kbd";

afterEach(() => {
  vi.restoreAllMocks();
});

test("one key cap per key, read out as words", () => {
  render(<Kbd keys="mod+shift+m" />);
  const hint = screen.getByRole("img", { name: "Control + Shift + M" });
  const caps = [...hint.querySelectorAll('[data-slot="kbd"]')];
  expect(caps.map((c) => c.textContent)).toEqual(["Ctrl", "⇧", "M"]);
  for (const c of caps) expect(c).toHaveAttribute("aria-hidden", "true");
});

test("mod is Command on a Mac", () => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  render(<Kbd keys="mod+k" />);
  expect(screen.getByRole("img", { name: "Command + K" })).toHaveTextContent("⌘K");
});
