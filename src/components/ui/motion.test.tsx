// The transitions adapted from transitions.dev (see "Motion" in index.css):
// what each shared control has to carry for its transition to play, and the
// reduced-motion guard every one of them must sit behind.

import { act, fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { StrictMode, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";

afterEach(() => {
  vi.useRealTimers();
});
import { Checkbox } from "./checkbox";
import { Modal } from "./modal";
import { Switch } from "./switch";

test("a switch bounces only once it has been used, not as the screen appears", () => {
  function Host() {
    const [on, setOn] = useState(false);
    return <Switch checked={on} onCheckedChange={setOn} ariaLabel="Group by title" />;
  }
  render(<Host />);
  const sw = screen.getByRole("switch", { name: "Group by title" });
  expect(sw).toHaveClass("t-toggle");
  expect(sw).not.toHaveClass("is-init");
  fireEvent.click(sw);
  expect(sw).toHaveClass("is-init");
  expect(sw).toHaveAttribute("aria-checked", "true");
});

test("the checkbox tick is always there to draw in; the mixed state shows a dash instead", () => {
  const TICK = "M5.252 12.7 10.2 18.63 18.748 5.37";
  const DASH = "M5.252 12h13.496";
  const mark = () =>
    screen
      .getByRole("checkbox", { name: "Pick" })
      .querySelector('[data-slot="checkbox-indicator"] path')
      ?.getAttribute("d");
  const { rerender } = render(<Checkbox checked={false} onCheckedChange={() => {}} ariaLabel="Pick" />);
  // Present while unchecked, so checking can DRAW it rather than pop it in.
  expect(mark()).toBe(TICK);
  rerender(<Checkbox checked onCheckedChange={() => {}} ariaLabel="Pick" />);
  expect(mark()).toBe(TICK);
  rerender(<Checkbox checked={false} indeterminate onCheckedChange={() => {}} ariaLabel="Pick" />);
  expect(mark()).toBe(DASH);
});

test("a modal's panel scales in over a fading backdrop", () => {
  render(
    <Modal onClose={() => {}}>
      <p>Body</p>
    </Modal>,
  );
  const dialog = screen.getByRole("dialog");
  expect(dialog).toHaveClass("t-modal");
  expect(dialog.parentElement).toHaveClass("t-backdrop");
});

test("every transition class is switched off under prefers-reduced-motion", () => {
  const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../../index.css"), "utf8");
  const motion = css.slice(css.indexOf("/* ── Motion"));
  expect(motion.length).toBeGreaterThan(0);
  // Every .t-* class that animates or transitions...
  const animated = new Set<string>();
  for (const block of motion.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (!/\b(animation|transition)\s*:/.test(block[2])) continue;
    for (const m of block[1].matchAll(/\.(t-[a-z-]+)/g)) animated.add(m[1]);
  }
  expect(animated.size).toBeGreaterThan(3);
  // ...is named inside the reduced-motion block.
  const guard = motion.slice(motion.lastIndexOf("@media (prefers-reduced-motion: reduce)"));
  const missing = [...animated].filter((c) => !guard.includes(`.${c}`));
  expect(missing).toEqual([]);
});

/// Screens unmount a dialog the moment it is dismissed, so the close
/// animation plays on a copy the Modal leaves behind - hidden, inert and
/// gone once the animation is.
test("a dismissed modal fades out on a copy that nothing can reach, then leaves", () => {
  vi.useFakeTimers();
  function Host() {
    const [open, setOpen] = useState(true);
    return (
      <>
        <button onClick={() => setOpen(false)}>Done</button>
        {open && (
          <Modal onClose={() => setOpen(false)}>
            <p>Are you sure?</p>
          </Modal>
        )}
      </>
    );
  }
  render(<Host />);
  expect(screen.getByRole("dialog")).toBeInTheDocument();

  fireEvent.click(screen.getByText("Done"));
  // The real dialog is gone at once...
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  // ...while its picture is still there, closing.
  const ghosts = [...document.querySelectorAll(".t-backdrop.is-closing")];
  const ghost = ghosts.find((g) => g.textContent?.includes("Are you sure?"));
  expect(ghost).toBeDefined();
  expect(ghost).toHaveAttribute("aria-hidden", "true");

  act(() => {
    vi.advanceTimersByTime(150);
  });
  expect(document.querySelector(".t-backdrop")).toBeNull();
});

/// StrictMode rehearses an unmount right after the first mount. That must
/// not leave a fading copy of a dialog that is, in fact, still open.
test("StrictMode's rehearsal unmount leaves no copy behind", () => {
  vi.useFakeTimers();
  render(
    <StrictMode>
      <Modal onClose={() => {}}>
        <p>Still here</p>
      </Modal>
    </StrictMode>,
  );
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  const copies = () => [...document.querySelectorAll(".is-closing")].filter((g) => g.textContent?.includes("Still here"));
  expect(copies()).toHaveLength(0);
  act(() => {
    vi.advanceTimersByTime(200);
  });
  expect(copies()).toHaveLength(0);
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});
