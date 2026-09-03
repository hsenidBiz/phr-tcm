import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import UiTour from "./UiTour";
import { tourDone } from "./tourState";
import type { TourStep } from "./tourScript";

afterEach(() => {
  localStorage.clear();
  document.querySelectorAll("[data-tour]").forEach((el) => el.remove());
});

function addAnchor(name: string) {
  const el = document.createElement("div");
  el.setAttribute("data-tour", name);
  document.body.appendChild(el);
}

const STEPS: TourStep[] = [
  { title: "Welcome", body: "A quick look around." },
  {
    where: { area: "cases", section: "import" },
    anchor: "import-drop",
    title: "Bring cases in from a file",
    body: "Drop in a file of ready-written cases.",
  },
  {
    where: { area: "work", workSection: "board" },
    anchor: "board-columns",
    title: "The other half of the app",
    body: "Your own items as cards.",
  },
];

test("each stop asks the app to go where it lives", async () => {
  addAnchor("import-drop");
  addAnchor("board-columns");
  const onNavigate = vi.fn();
  render(<UiTour steps={STEPS} onNavigate={onNavigate} onClose={vi.fn()} />);

  expect(screen.getByText("1 / 3")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(undefined);

  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(onNavigate).toHaveBeenLastCalledWith({ area: "cases", section: "import" });
  expect(await screen.findByText("Bring cases in from a file")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(onNavigate).toHaveBeenLastCalledWith({ area: "work", workSection: "board" });

  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(onNavigate).toHaveBeenLastCalledWith({ area: "cases", section: "import" });
});

test("a stop whose area never turns up still shows - it is not dropped", async () => {
  // No anchors added at all.
  render(<UiTour steps={STEPS} onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByText("Bring cases in from a file")).toBeInTheDocument();
  expect(screen.getByText("2 / 3")).toBeInTheDocument();
});

test("Skip tour ends it and remembers it was seen", () => {
  const onClose = vi.fn();
  render(<UiTour steps={STEPS} onNavigate={vi.fn()} onClose={onClose} />);
  fireEvent.click(screen.getByText("Skip tour"));
  expect(onClose).toHaveBeenCalled();
  expect(tourDone()).toBe(true);
});

test("Done on the last stop ends it the same way", () => {
  const onClose = vi.fn();
  render(<UiTour steps={STEPS} onNavigate={vi.fn()} onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalled();
  expect(tourDone()).toBe(true);
});

test("Escape does not end the tour - leaving is a deliberate click", () => {
  const onClose = vi.fn();
  render(<UiTour steps={STEPS} onNavigate={vi.fn()} onClose={onClose} />);
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
const MANUAL_WHERE = { area: "cases", section: "manual" } as const;
const IMPORT_WHERE = { area: "cases", section: "import" } as const;

const SINGLE_HOP_STEPS: TourStep[] = [
  { where: MANUAL_WHERE, anchor: "case-form", title: "Write a test case", body: "Fill in the steps." },
  { anchor: "queue", title: "Build up a batch", body: "Cases queue up here." },
  { where: IMPORT_WHERE, anchor: "import-drop", title: "Bring cases in", body: "Drop a file in." },
];

test("Back from a stop that declares a destination into one that does not carries forward the earlier declared one", async () => {
  addAnchor("case-form");
  addAnchor("queue");
  addAnchor("import-drop");
  const onNavigate = vi.fn();
  render(<UiTour steps={SINGLE_HOP_STEPS} onNavigate={onNavigate} onClose={vi.fn()} />);
  expect(onNavigate).toHaveBeenLastCalledWith(MANUAL_WHERE);

  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> queue (no where, inherits manual)
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> import-drop (declares import)
  expect(await screen.findByText("Bring cases in")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(IMPORT_WHERE);

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
  const onNavigate = vi.fn();
  render(<UiTour steps={MULTI_HOP_STEPS} onNavigate={onNavigate} onClose={vi.fn()} />);

  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> queue (no where)
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> case-list (no where)
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> import-drop (declares import)
  expect(await screen.findByText("Bring cases in")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(IMPORT_WHERE);

  onNavigate.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Back" })); // -> case-list (carries back to manual)
  fireEvent.click(screen.getByRole("button", { name: "Back" })); // -> queue (still manual)
  expect(await screen.findByText("Build up a batch")).toBeInTheDocument();
  expect(onNavigate).toHaveBeenLastCalledWith(MANUAL_WHERE);
});

test("moving between stops that share an effective destination does not re-navigate", async () => {
  addAnchor("case-form");
  addAnchor("queue");
  addAnchor("case-list");
  const onNavigate = vi.fn();
  render(<UiTour steps={MULTI_HOP_STEPS} onNavigate={onNavigate} onClose={vi.fn()} />);
  expect(onNavigate).toHaveBeenCalledTimes(1);
  expect(onNavigate).toHaveBeenLastCalledWith(MANUAL_WHERE);

  onNavigate.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // stop 1 -> stop 2, both inherit manual
  await screen.findByText("Build up a batch");
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // stop 2 -> stop 3, still manual
  await screen.findByText("Undeclared too");
  expect(onNavigate).not.toHaveBeenCalled();
});
