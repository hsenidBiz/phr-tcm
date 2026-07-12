import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import UiTour, { tourDone } from "./UiTour";

afterEach(() => {
  localStorage.clear();
  document.querySelectorAll("[data-tour]").forEach((el) => el.remove());
});

function addAnchor(name: string) {
  const el = document.createElement("div");
  el.setAttribute("data-tour", name);
  document.body.appendChild(el);
}

test("steps through available anchors; Done marks the tour finished", () => {
  addAnchor("org");
  addAnchor("nav-import");
  const onClose = vi.fn();
  render(<UiTour onClose={onClose} />);

  // Only the two anchored steps exist.
  expect(screen.getByText("1 / 2")).toBeInTheDocument();
  expect(screen.getByText("Pick your scope")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Next" }));
  expect(screen.getByText("Import File")).toBeInTheDocument();
  expect(screen.getByText(/Upload test cases from a JSON file/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalled();
  expect(tourDone()).toBe(true);
});

test("Skip tour also persists completion", () => {
  addAnchor("pbi");
  const onClose = vi.fn();
  render(<UiTour onClose={onClose} />);
  fireEvent.click(screen.getByText("Skip tour"));
  expect(onClose).toHaveBeenCalled();
  expect(tourDone()).toBe(true);
});
