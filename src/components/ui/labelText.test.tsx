import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { render } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Badge } from "./badge";
import { Button } from "./button";
import { Kbd } from "./kbd";
import { trimLabels } from "./labelText";

// The shared controls centre their labels by the letters (index.css,
// `.label-trim`), so no call site has to remember to.
describe("labels are centred by their letters", () => {
  test("Button wraps its text in label-trim and leaves the icon alone", () => {
    const { getByRole } = render(
      <Button>
        <svg data-testid="icon" />
        Test files
      </Button>,
    );
    const button = getByRole("button", { name: "Test files" });
    const label = button.querySelector(".label-trim");
    expect(label?.textContent).toBe("Test files");
    expect(button.querySelector("svg")?.closest(".label-trim")).toBeNull();
  });

  test("text split across expressions stays one label, spaces kept", () => {
    const n = 2;
    const { getByRole } = render(
      <Button size="sm">
        <>
          Run {n} in runner
        </>
      </Button>,
    );
    const labels = getByRole("button").querySelectorAll(".label-trim");
    expect(labels).toHaveLength(1);
    expect(labels[0].textContent).toBe("Run 2 in runner");
  });

  test("Badge wraps its text in label-trim", () => {
    const { container } = render(<Badge>NEW</Badge>);
    expect(container.querySelector(".label-trim")?.textContent).toBe("NEW");
  });

  test("Kbd's key caps carry label-trim", () => {
    const { container } = render(<Kbd keys="mod+k" />);
    const caps = container.querySelectorAll(".label-trim");
    expect(caps.length).toBe(2);
  });

  test("trimLabels drops nothing and wraps no element", () => {
    const { container } = render(
      <div>
        {trimLabels([<b key="b">bold</b>, " ", null, false, "tail", 3])}
      </div>,
    );
    expect(container.textContent).toBe("bold tail3");
    expect(container.querySelector("b")?.closest(".label-trim")).toBeNull();
    expect(container.querySelector(".label-trim")?.textContent).toBe(" tail3");
  });

  test("index.css trims the label to cap height and baseline, at one line's height", () => {
    const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../index.css"), "utf-8");
    const rule = css.match(/\.label-trim \{([^}]*)\}/)?.[1] ?? "";
    expect(rule).toContain("text-box: trim-both cap alphabetic");
    expect(rule).toContain("min-height: 1lh");
    expect(rule).toContain("align-content: center");
  });
});
