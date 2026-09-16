import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { leaveExitGhost } from "./exitGhost";

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

function mount(): HTMLDivElement {
  const el = document.createElement("div");
  el.className = "t-backdrop";
  el.id = "backdrop";
  el.innerHTML = `<div role="dialog" id="dlg" aria-modal="true" tabindex="-1"><div class="scroller"><p>Body</p></div><button aria-label="Remove regression">x</button></div>`;
  document.body.appendChild(el);
  return el;
}

test("leaves a closing copy behind that nothing can reach, and takes it away after the animation", () => {
  const el = mount();
  leaveExitGhost(el, 150);
  el.remove(); // what React does next

  const ghost = document.querySelector(".t-backdrop.is-closing") as HTMLElement | null;
  expect(ghost).not.toBeNull();
  expect(ghost).toHaveAttribute("aria-hidden", "true");
  expect(ghost).toHaveAttribute("inert");
  expect(ghost?.style.pointerEvents).toBe("none");
  // The copy is a picture of the dialog, never a second dialog: no ids
  // for anything to find, no roles for assistive tech to announce.
  expect(ghost?.querySelector("[id]")).toBeNull();
  expect(ghost?.querySelector("[role]")).toBeNull();
  expect(ghost?.querySelector("[aria-label], [aria-modal]")).toBeNull();
  expect(ghost).toHaveAttribute("data-exit-ghost");
  expect(ghost?.textContent).toContain("Body");

  vi.advanceTimersByTime(149);
  expect(document.querySelector(".is-closing")).not.toBeNull();
  vi.advanceTimersByTime(1);
  expect(document.querySelector(".is-closing")).toBeNull();
});

test("a scrolled dialog keeps its place in the copy", () => {
  const el = mount();
  const scroller = el.querySelector(".scroller") as HTMLElement;
  Object.defineProperty(scroller, "scrollTop", { value: 120, writable: true, configurable: true });
  leaveExitGhost(el, 150);
  const ghostScroller = document.querySelector(".is-closing .scroller") as HTMLElement;
  expect(ghostScroller.scrollTop).toBe(120);
});

test("an in-flow copy shrinks where the original stood", () => {
  const el = mount();
  const after = document.createElement("p");
  after.textContent = "After";
  document.body.appendChild(after);
  leaveExitGhost(el, 150, "after");
  el.remove();
  const ghost = document.querySelector(".is-closing");
  expect(ghost?.nextElementSibling).toBe(after);
});

test("an animate hook runs on the placed copy and sets how long it stays", () => {
  const el = mount();
  let seen: HTMLElement | null = null;
  leaveExitGhost(el, 150, "after", (ghost, original) => {
    expect(ghost.isConnected).toBe(true);
    expect(original).toBe(el);
    seen = ghost;
    return 400;
  });
  expect(seen).not.toBeNull();
  vi.advanceTimersByTime(399);
  expect(document.querySelector(".is-closing")).not.toBeNull();
  vi.advanceTimersByTime(1);
  expect(document.querySelector(".is-closing")).toBeNull();
});

test("an animate hook that returns 0 removes the copy at once", () => {
  const el = mount();
  leaveExitGhost(el, 150, "after", () => 0);
  expect(document.querySelector(".is-closing")).toBeNull();
});

test("cancelling removes the copy at once", () => {
  const el = mount();
  const cancel = leaveExitGhost(el, 150);
  expect(document.querySelector(".is-closing")).not.toBeNull();
  cancel();
  expect(document.querySelector(".is-closing")).toBeNull();
  vi.advanceTimersByTime(200); // and the timer has nothing left to do
});

test("under reduced motion nothing is left behind", () => {
  const real = window.matchMedia;
  window.matchMedia = ((q: string) =>
    ({ matches: q.includes("reduce"), media: q, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia;
  try {
    const el = mount();
    const cancel = leaveExitGhost(el, 150);
    expect(document.querySelector(".is-closing")).toBeNull();
    cancel();
  } finally {
    window.matchMedia = real;
  }
});
