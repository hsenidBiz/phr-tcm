import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useCallback, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import UiTour from "./UiTour";
import { tourDone } from "./tourState";
import type { TourStep, TourWhere } from "./tourScript";

afterEach(() => {
  localStorage.clear();
  document.querySelectorAll("[data-tour]").forEach((el) => el.remove());
});

function addAnchor(name: string) {
  const el = document.createElement("div");
  el.setAttribute("data-tour", name);
  document.body.appendChild(el);
}

const MANUAL_WHERE = { area: "cases", section: "manual" } as const;
const IMPORT_WHERE = { area: "cases", section: "import" } as const;
const BOARD_WHERE = { area: "work", workSection: "board" } as const;

const STEPS: TourStep[] = [
  { title: "Welcome", body: "A quick look around." },
  {
    where: IMPORT_WHERE,
    anchor: "import-drop",
    title: "Bring cases in from a file",
    body: "Drop in a file of ready-written cases.",
  },
  {
    where: BOARD_WHERE,
    anchor: "board-columns",
    title: "The other half of the app",
    body: "Your own items as cards.",
  },
];

/** Most of these walk stops that are NOT a move, so the app starts where
 * the stop lives; the tests that are about waiting say so. */
const AT_IMPORT: TourWhere = IMPORT_WHERE;

const waiting = () =>
  screen.getByRole("dialog", { name: "Interface tour" }).getAttribute("data-waiting") === "true";

const overlay = () => screen.getByRole("dialog", { name: "Interface tour" });

/// Field report: with the tour waiting, clicking the rail row it had just
/// asked for did nothing. The swallowing layer was correctly dropped while
/// waiting, but the overlay's own full-viewport container was still the
/// element every click landed on - a transparent covering element is still
/// a hit-test target, and only `pointer-events` changes that.
///
/// jsdom does no hit testing, so `fireEvent.click` reaches a covered
/// element happily and cannot reproduce this. What CAN be held here is the
/// structural rule the fix rests on: the container never takes pointer
/// events, and each child that must be clickable turns them back on.
/// Field report: on Update Test Cases the ring came out the wrong size and
/// walking Back then Next corrected it. The area is measured the moment it
/// exists, which on a fetching screen is while it is still empty - so the
/// ring was sized to an empty list and never re-measured. The element is
/// now watched for growth.
///
/// jsdom implements no layout, so a real size change cannot be produced
/// here. What is held instead is that the anchored element IS observed and
/// that the observer is disconnected on unmount - without which the ring
/// could only ever be correct by luck of timing.
test("the ringed area is watched, so a screen that fills in re-measures", () => {
  const observed: Element[] = [];
  let disconnected = 0;
  const real = globalThis.ResizeObserver;
  globalThis.ResizeObserver = class {
    observe(el: Element) {
      observed.push(el);
    }
    unobserve() {}
    disconnect() {
      disconnected += 1;
    }
  } as unknown as typeof ResizeObserver;

  try {
    addAnchor("case-form");
    const { unmount } = render(
      <UiTour
        steps={SINGLE_HOP_STEPS}
        at={MANUAL_WHERE}
        onNavigate={vi.fn()}
        onAwait={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(observed).toEqual([document.querySelector('[data-tour="case-form"]')]);
    unmount();
    expect(disconnected).toBeGreaterThan(0);
  } finally {
    globalThis.ResizeObserver = real;
  }
});

test("the overlay's container never swallows clicks meant for the app", () => {
  addAnchor("case-form");
  render(
    <UiTour
      steps={SINGLE_HOP_STEPS}
      at={MANUAL_WHERE}
      onNavigate={vi.fn()}
      onAwait={vi.fn()}
      onClose={vi.fn()}
    />,
  );
  expect(overlay().className).toContain("pointer-events-none");
  const card = screen.getByText(SINGLE_HOP_STEPS[0].title).closest("div.fixed");
  expect(card?.className).toContain("pointer-events-auto");
});

/** A stand-in for App: it holds where the app is, follows the tour when
 * Back walks it somewhere, and offers one button that does what clicking
 * the awaited rail row does - move the app to the awaited destination. */
function Walker({
  steps,
  start,
  onNavigate = () => {},
  onClose = () => {},
}: {
  steps: TourStep[];
  start: TourWhere;
  onNavigate?: (w: TourWhere | undefined) => void;
  onClose?: () => void;
}) {
  const [at, setAt] = useState<TourWhere>(start);
  const [awaited, setAwaited] = useState<TourWhere | null>(null);
  const nav = useCallback(
    (w: TourWhere | undefined) => {
      onNavigate(w);
      if (w) setAt(w);
    },
    [onNavigate],
  );
  const onAwait = useCallback((w: TourWhere | null) => setAwaited(w), []);
  return (
    <>
      <button onClick={() => awaited && setAt(awaited)}>walk there</button>
      <UiTour steps={steps} at={at} onNavigate={nav} onAwait={onAwait} onClose={onClose} />
    </>
  );
}

/** Click the control the tour is waiting for. */
const walk = () => fireEvent.click(screen.getByRole("button", { name: "walk there" }));
const next = () => fireEvent.click(screen.getByRole("button", { name: "Next" }));
/** Next when the tour offers it, otherwise the click it is waiting for. */
const advance = () => (waiting() ? walk() : next());

// --- the ask ------------------------------------------------------------

test("a stop somewhere else asks for the click instead of moving the app", async () => {
  addAnchor("nav-import");
  addAnchor("import-drop");
  const onNavigate = vi.fn();
  const onAwait = vi.fn();
  const { rerender } = render(
    <UiTour
      steps={STEPS}
      at={MANUAL_WHERE}
      onNavigate={onNavigate}
      onAwait={onAwait}
      onClose={vi.fn()}
    />,
  );

  // The opening card has nowhere to be, so it behaves normally.
  expect(waiting()).toBe(false);
  expect(onAwait).toHaveBeenLastCalledWith(null);

  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  // Stop 2 lives on Import File and the app is on Manual Entry: it asks,
  // by the name the rail actually shows, and does NOT navigate.
  expect(waiting()).toBe(true);
  expect(screen.getByText("Go to Import File")).toBeInTheDocument();
  expect(screen.getByText(/Click Import File in the menu on the left/)).toBeInTheDocument();
  expect(onNavigate).not.toHaveBeenCalled();
  expect(onAwait).toHaveBeenLastCalledWith(IMPORT_WHERE);
  // Next is gone - if it still advanced, the ask would be decorative.
  expect(screen.queryByRole("button", { name: "Next" })).not.toBeInTheDocument();

  // The user clicks it; the app arrives, and the stop shows its own words.
  rerender(
    <UiTour
      steps={STEPS}
      at={IMPORT_WHERE}
      onNavigate={onNavigate}
      onAwait={onAwait}
      onClose={vi.fn()}
    />,
  );
  expect(waiting()).toBe(false);
  expect(await screen.findByText("Bring cases in from a file")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Next" })).toBeInTheDocument();
  expect(onAwait).toHaveBeenLastCalledWith(null);
});

test("a stop the app is already on keeps its Next and says nothing about clicking", () => {
  addAnchor("import-drop");
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(waiting()).toBe(false);
  expect(screen.getByText("Bring cases in from a file")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Next" })).toBeInTheDocument();
});

test("crossing between the two halves of the app asks for the pill, not a rail row", () => {
  addAnchor("work");
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> import (already there)
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> the board
  expect(waiting()).toBe(true);
  expect(screen.getByText("Go to Work Manager")).toBeInTheDocument();
  expect(screen.getByText(/Click Work Manager at the top of the screen/)).toBeInTheDocument();
});

test("while waiting, the ring lands on the control being asked for", async () => {
  addAnchor("nav-import");
  const spy = vi.spyOn(Element.prototype, "scrollIntoView");
  render(<UiTour steps={STEPS} at={MANUAL_WHERE} onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await waitFor(() =>
    expect(spy.mock.instances).toContain(document.querySelector('[data-tour="nav-import"]')),
  );
  spy.mockRestore();
});

test("Back from a waiting stop goes back a stop and takes the app with it", async () => {
  addAnchor("case-form");
  addAnchor("nav-import");
  const steps: TourStep[] = [
    { where: MANUAL_WHERE, anchor: "case-form", title: "Write a test case", body: "Fill it in." },
    { where: IMPORT_WHERE, anchor: "import-drop", title: "Bring cases in", body: "Drop a file in." },
  ];
  const onNavigate = vi.fn();
  render(<UiTour steps={steps} at={MANUAL_WHERE} onNavigate={onNavigate} onClose={vi.fn()} />);

  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(waiting()).toBe(true);

  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(await screen.findByText("Write a test case")).toBeInTheDocument();
  expect(screen.getByText("1 / 2")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(MANUAL_WHERE);
});

test("Skip tour ends it and remembers it was seen", () => {
  const onClose = vi.fn();
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={onClose} />);
  fireEvent.click(screen.getByText("Skip tour"));
  expect(onClose).toHaveBeenCalled();
  expect(tourDone()).toBe(true);
});

test("Done on the last stop ends it the same way", () => {
  const onClose = vi.fn();
  render(<Walker steps={STEPS} start={MANUAL_WHERE} onClose={onClose} />);
  advance(); // Welcome -> the Import stop's ask
  advance(); // ...walked there
  advance(); // -> the board's ask
  advance(); // ...walked there
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalled();
  expect(tourDone()).toBe(true);
});

test("Escape does not end the tour - leaving is a deliberate click", () => {
  const onClose = vi.fn();
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={onClose} />);
  fireEvent.keyDown(document, { key: "Escape" });
  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
});

// A stop with no `where` of its own means "stay wherever the last stop
// left the app" - so Back into one has to navigate to the last declared
// destination, not to undefined (which would strand the app on whatever
// tab the later stop left it on). Mirrors the real bug: stop 5 (queue) has
// no `where` of its own and inherits Manual Entry from stop 1; stop 6
// (Import File) declares its own. Back from 6 into 5 must land back on
// Manual Entry, not fall through to undefined.
const SINGLE_HOP_STEPS: TourStep[] = [
  { where: MANUAL_WHERE, anchor: "case-form", title: "Write a test case", body: "Fill in the steps." },
  { anchor: "queue", title: "Build up a batch", body: "Cases queue up here." },
  { where: IMPORT_WHERE, anchor: "import-drop", title: "Bring cases in", body: "Drop a file in." },
];

test("Back from a stop that declares a destination into one that does not carries forward the earlier declared one", async () => {
  addAnchor("case-form");
  addAnchor("queue");
  addAnchor("import-drop");
  addAnchor("nav-import");
  const onNavigate = vi.fn();
  render(<Walker steps={SINGLE_HOP_STEPS} start={MANUAL_WHERE} onNavigate={onNavigate} />);
  next(); // -> queue (inherits manual, so no ask)
  advance(); // -> the Import stop's ask
  advance(); // ...and the user walks there
  expect(await screen.findByText("Bring cases in")).toBeInTheDocument();

  onNavigate.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Back" })); // -> queue: must land back on manual
  expect(await screen.findByText("Build up a batch")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(MANUAL_WHERE);
  expect(onNavigate).not.toHaveBeenCalledWith(undefined);
});

const MULTI_HOP_STEPS: TourStep[] = [
  { where: MANUAL_WHERE, anchor: "case-form", title: "Write a test case", body: "Fill in the steps." },
  { anchor: "queue", title: "Build up a batch", body: "Cases queue up here." },
  { anchor: "case-list", title: "Undeclared too", body: "Still on manual." },
  { where: IMPORT_WHERE, anchor: "import-drop", title: "Bring cases in", body: "Drop a file in." },
];

test("Back across several undeclared stops still lands on the right destination", async () => {
  addAnchor("case-form");
  addAnchor("queue");
  addAnchor("case-list");
  addAnchor("import-drop");
  addAnchor("nav-import");
  const onNavigate = vi.fn();
  render(<Walker steps={MULTI_HOP_STEPS} start={MANUAL_WHERE} onNavigate={onNavigate} />);

  next(); // -> queue (no where)
  next(); // -> case-list (no where)
  advance(); // -> the Import stop's ask
  advance(); // ...and the user walks there
  expect(await screen.findByText("Bring cases in")).toBeInTheDocument();

  onNavigate.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Back" })); // -> case-list (carries back to manual)
  fireEvent.click(screen.getByRole("button", { name: "Back" })); // -> queue (still manual)
  expect(await screen.findByText("Build up a batch")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(MANUAL_WHERE);
});

test("a stop whose area never turns up still shows - it is not dropped", async () => {
  // No anchors added at all.
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText("Bring cases in from a file")).toBeInTheDocument();
  expect(screen.getByText("2 / 3")).toBeInTheDocument();
});

test("scrollIntoView is called for an anchored stop and not for an unanchored one", async () => {
  addAnchor("import-drop");
  const spy = vi.spyOn(Element.prototype, "scrollIntoView");
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={vi.fn()} />);

  // Stop 0 has no anchor - nothing to scroll to.
  expect(spy).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> anchored stop
  await screen.findByText("Bring cases in from a file");

  expect(spy).toHaveBeenCalledWith({ block: "center" });
  const el = document.querySelector('[data-tour="import-drop"]');
  expect(spy.mock.instances).toContain(el);

  spy.mockRestore();
});

test("a scroll event re-measures the ring, even dispatched on a non-bubbling target", async () => {
  addAnchor("import-drop");
  render(<UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  await screen.findByText("Bring cases in from a file");

  const el = document.querySelector('[data-tour="import-drop"]') as HTMLElement;
  const moved = {
    top: 900,
    left: 40,
    right: 240,
    bottom: 940,
    width: 200,
    height: 40,
    x: 40,
    y: 900,
    toJSON: () => ({}),
  } as DOMRect;
  const rectSpy = vi.spyOn(el, "getBoundingClientRect").mockReturnValue(moved);

  // The app scrolls inside an inner container, not the window, so scroll
  // events do not bubble to window - only the capture phase sees them.
  // Dispatch on an unrelated, non-bubbling target to prove that's what is
  // actually relied on here, not accidental bubbling.
  const inner = document.createElement("div");
  document.body.appendChild(inner);
  inner.dispatchEvent(new Event("scroll", { bubbles: false }));
  inner.remove();

  // The listener runs outside React's own event system (it's a native
  // window listener, not a synthetic one), so the resulting state update
  // is not necessarily flushed by the time dispatchEvent returns.
  await waitFor(() => {
    const ring = document.querySelector(".border-accent") as HTMLElement;
    expect(ring.style.top).toBe("894px"); // moved.top - pad(6)
  });

  rectSpy.mockRestore();
});

test("scroll and resize listeners are torn down on step change and on unmount", async () => {
  addAnchor("import-drop");
  addAnchor("board-columns");
  const addSpy = vi.spyOn(window, "addEventListener");
  const removeSpy = vi.spyOn(window, "removeEventListener");
  const { unmount } = render(
    <UiTour steps={STEPS} at={AT_IMPORT} onNavigate={vi.fn()} onClose={vi.fn()} />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> anchored stop
  await screen.findByText("Bring cases in from a file");
  expect(addSpy).toHaveBeenCalledWith("scroll", expect.any(Function), { capture: true, passive: true });
  expect(addSpy).toHaveBeenCalledWith("resize", expect.any(Function));

  fireEvent.click(screen.getByRole("button", { name: "Next" })); // step change tears the old pair down
  expect(removeSpy).toHaveBeenCalledWith("scroll", expect.any(Function), { capture: true });
  expect(removeSpy).toHaveBeenCalledWith("resize", expect.any(Function));

  removeSpy.mockClear();
  unmount();
  expect(removeSpy).toHaveBeenCalledWith("scroll", expect.any(Function), { capture: true });
  expect(removeSpy).toHaveBeenCalledWith("resize", expect.any(Function));

  addSpy.mockRestore();
  removeSpy.mockRestore();
});

test("moving between stops that share an effective destination waits for nothing", async () => {
  addAnchor("case-form");
  addAnchor("queue");
  addAnchor("case-list");
  const onAwait = vi.fn();
  render(
    <UiTour
      steps={MULTI_HOP_STEPS}
      at={MANUAL_WHERE}
      onNavigate={vi.fn()}
      onAwait={onAwait}
      onClose={vi.fn()}
    />,
  );
  onAwait.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // stop 1 -> 2, both inherit manual
  await screen.findByText("Build up a batch");
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // stop 2 -> 3, still manual
  await screen.findByText("Undeclared too");
  // The app is on manual throughout, so nothing was ever awaited.
  for (const call of onAwait.mock.calls) expect(call[0]).toBeNull();
});
