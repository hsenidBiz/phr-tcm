// The window's title bar: the title, and on a beta build a Beta pill
// beside it so every screen says which build this is.

import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { isBetaVersion } from "../lib/changelog";
import TitleBar from "./TitleBar";

test("a stable build shows the title alone", () => {
  render(<TitleBar title="Test Case Manager" />);
  expect(screen.getByText("Test Case Manager")).toBeInTheDocument();
  expect(screen.queryByText("Beta")).not.toBeInTheDocument();
});

test("a beta build shows the Beta pill beside the title", () => {
  render(<TitleBar title="Test Case Manager" beta />);
  const pill = screen.getByText("Beta");
  // In the same group as the title, inside the drag region.
  expect(pill.parentElement).toBe(screen.getByText("Test Case Manager").parentElement);
});

test("the pill follows the version the app is running", () => {
  // What App passes as `beta`.
  expect(isBetaVersion("2.0.4-beta.2")).toBe(true);
  expect(isBetaVersion("2.0.3")).toBe(false);
  expect(isBetaVersion("dev")).toBe(false);
});
