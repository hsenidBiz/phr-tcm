import { fireEvent, render, screen } from "@testing-library/react";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import RunnerGameModal, { RUNNER_SRC } from "./RunnerGameModal";

const frameOf = () => screen.getByTitle("Dino game") as HTMLIFrameElement;

test("the game opens in a frame of the bundled page, with focus in it", () => {
  render(<RunnerGameModal onClose={() => {}} />);
  const frame = frameOf();
  expect(frame.getAttribute("src")).toBe("/vendor/runner/index.html");
  expect(RUNNER_SRC).toBe("/vendor/runner/index.html");
  expect(document.activeElement).toBe(frame);
  const publicDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../public");
  expect(existsSync(join(publicDir, RUNNER_SRC))).toBe(true);
});

/// A key pressed inside a frame never reaches this window, so the Modal's
/// own Escape handler cannot hear it while the game has focus.
test("Escape pressed inside the game closes it, once", () => {
  const onClose = vi.fn();
  render(<RunnerGameModal onClose={onClose} />);
  const frame = frameOf();
  fireEvent.load(frame);
  fireEvent.load(frame); // a second load of the same document wires nothing twice
  const win = frame.contentWindow!;
  // Cast only: TypeScript's Window type omits global constructors like
  // KeyboardEvent as members, but the iframe's own realm has one at
  // runtime, and using it (not the outer window's) matches how the
  // component's own listener is attached.
  const FrameKeyboardEvent = (win as unknown as typeof globalThis).KeyboardEvent;
  win.dispatchEvent(new FrameKeyboardEvent("keydown", { key: "Escape" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("the Close button closes it", () => {
  const onClose = vi.fn();
  render(<RunnerGameModal onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

/// The Modal leaves a fading copy of itself behind on close; a copied frame
/// would load the game a second time for the length of the fade.
test("the copy left behind on close does not load the game again", () => {
  function Host() {
    const [open, setOpen] = useState(true);
    return open ? <RunnerGameModal onClose={() => setOpen(false)} /> : <p>closed</p>;
  }
  render(<Host />);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.getByText("closed")).toBeInTheDocument();
  const copies = document.querySelectorAll("[data-exit-ghost] iframe");
  expect(copies.length).toBeGreaterThan(0); // the fade copy exists, so the check below means something
  for (const copy of copies) {
    expect(copy.getAttribute("src")).toBe("about:blank");
  }
});
