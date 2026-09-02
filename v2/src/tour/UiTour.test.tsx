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
