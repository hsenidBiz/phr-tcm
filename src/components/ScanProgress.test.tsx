import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import ScanProgress from "./ScanProgress";

test("with counts it is a determinate bar carrying accessible values", () => {
  render(<ScanProgress label="Scanning test plans" done={3} total={12} />);
  const bar = screen.getByRole("progressbar", { name: "Scanning test plans" });
  expect(bar).toHaveAttribute("aria-valuenow", "3");
  expect(bar).toHaveAttribute("aria-valuemax", "12");
  expect(screen.getByText("3 / 12")).toBeInTheDocument();
  // 3/12 -> 25% width, and it glows rather than sweeping.
  const fill = bar.firstElementChild as HTMLElement;
  expect(fill.style.width).toBe("25%");
  expect(fill.className).toContain("scan-glow");
  expect(fill.className).not.toContain("scan-sweep");
});

test("without counts it sweeps and claims no false progress", () => {
  render(<ScanProgress label="Loading test plans" />);
  const bar = screen.getByRole("progressbar", { name: "Loading test plans" });
  expect(bar).not.toHaveAttribute("aria-valuenow");
  const fill = bar.firstElementChild as HTMLElement;
  expect(fill.className).toContain("scan-sweep");
  expect(fill.style.width).toBe("");
});

test("a zero total does not divide by zero - it sweeps instead", () => {
  render(<ScanProgress label="Scanning" done={0} total={0} />);
  const fill = screen.getByRole("progressbar").firstElementChild as HTMLElement;
  expect(fill.className).toContain("scan-sweep");
});
