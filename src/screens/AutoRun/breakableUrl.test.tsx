import { render } from "@testing-library/react";
import { expect, test } from "vitest";
import { breakableUrl } from "./SetupPanel";

test("a long address gets a break opportunity after each / . ? & =, and keeps its text", () => {
  const url = "https://hrmmainslqaautomation.example.com/login?next=/home&a=1";
  const { container } = render(<span>{breakableUrl(url)}</span>);
  expect(container.textContent).toBe(url);
  // https:/ / hrmm...example. com/ login? next= / ... every one of them breaks.
  expect(container.querySelectorAll("wbr").length).toBeGreaterThanOrEqual(8);
  expect(container.firstElementChild?.className ?? "").not.toContain("break-all");
  expect(container.innerHTML).not.toContain("break-all");
});
