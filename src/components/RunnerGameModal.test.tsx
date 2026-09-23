import { fireEvent, render, screen } from "@testing-library/react";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import RunnerGameModal, { RUNNER_SRC } from "./RunnerGameModal";

const frameOf = () => screen.getByTitle("Dino game") as HTMLIFrameElement;

/** A `message` event as escape.js sends it, or a near-miss of one. */
const messageFrom = (source: Window, data: unknown) => new MessageEvent("message", { data, source });

test("the game opens in a sandboxed frame of the bundled page, with focus in it", () => {
  render(<RunnerGameModal onClose={() => {}} />);
  const frame = frameOf();
  expect(frame.getAttribute("src")).toBe("/vendor/runner/index.html");
  // No allow-same-origin alongside it - that would give the frame back the
  // same-origin access sandboxing exists to take away.
  expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
  expect(RUNNER_SRC).toBe("/vendor/runner/index.html");
  expect(document.activeElement).toBe(frame);
  const publicDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../public");
  expect(existsSync(join(publicDir, RUNNER_SRC))).toBe(true);
});

/// The sandboxed frame has no access to `parent` beyond postMessage, so
/// escape.js (bundled alongside the game) relays Escape as a message
/// instead of the modal reaching into the frame to listen directly.
test("a runner-escape message from the frame's own window closes it, once", () => {
  const onClose = vi.fn();
  render(<RunnerGameModal onClose={onClose} />);
  const win = frameOf().contentWindow!;
  window.dispatchEvent(messageFrom(win, { type: "runner-escape" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a message from any other source is ignored", () => {
  const onClose = vi.fn();
  render(<RunnerGameModal onClose={onClose} />);
  window.dispatchEvent(messageFrom(window, { type: "runner-escape" }));
  expect(onClose).not.toHaveBeenCalled();
});

test("a message of any other type from the frame is ignored", () => {
  const onClose = vi.fn();
  render(<RunnerGameModal onClose={onClose} />);
  const win = frameOf().contentWindow!;
  window.dispatchEvent(messageFrom(win, { type: "something-else" }));
  window.dispatchEvent(messageFrom(win, "runner-escape"));
  window.dispatchEvent(messageFrom(win, null));
  expect(onClose).not.toHaveBeenCalled();
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
