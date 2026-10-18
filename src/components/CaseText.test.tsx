// Test case text shown formatted, and edited as the Markdown it is.

import { fireEvent, render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { useState } from "react";
import { expect, test } from "vitest";
import CaseText, { CaseTextField, PreviewToggle } from "./CaseText";

test("CaseText shows bold, a query fence and line breaks", () => {
  const { container } = render(
    <CaseText kind="prereq" text={"Click **Save**\nthen run\n\n```sql\nSELECT 1\n```"} />,
  );
  expect(container.querySelector("strong")?.textContent).toBe("Save");
  expect(container.querySelector("pre code")?.textContent).toContain("SELECT 1");
  expect(container.querySelector("p br")).not.toBeNull();
});

test("CaseText never renders markup from the text as elements", () => {
  for (const text of ["<img src=x onerror=alert(1)>", "<script>alert(1)</script>", "x <img src=x onerror=alert(1)>", "<code><img src=https://evil.invalid/x.png "]) {
    const { container, unmount } = render(<CaseText kind="step" text={text} />);
    expect(container.querySelectorAll("img,script"), text).toHaveLength(0);
    for (const el of container.querySelectorAll("*")) {
      for (const attr of el.attributes) expect(attr.name.startsWith("on"), text).toBe(false);
    }
    unmount();
  }
});

test("a plain COUNT(*) query shows without italics", () => {
  const { container } = render(<CaseText kind="step" text="SELECT COUNT(*) FROM t WHERE a * b > 1" />);
  expect(container.querySelector("em,strong")).toBeNull();
  expect(container.textContent?.trim()).toBe("SELECT COUNT(*) FROM t WHERE a * b > 1");
});

test("an empty text says so instead of showing an empty box", () => {
  render(<CaseText kind="step" text="  " empty="Nothing to preview" />);
  expect(screen.getByText("Nothing to preview")).toBeTruthy();
});

function Field({ initial }: { initial: string }) {
  const [value, setValue] = useState(initial);
  const [preview, setPreview] = useState(false);
  return (
    <>
      <CaseTextField
        aria-label="Preconditions"
        kind="prereq"
        value={value}
        onChange={setValue}
        preview={preview}
      />
      <PreviewToggle what="preconditions" on={preview} onToggle={() => setPreview((p) => !p)} />
      <output data-testid="value">{value}</output>
    </>
  );
}

test("the field keeps the Markdown typed, and Preview shows it formatted", () => {
  render(<Field initial="" />);
  const typed = "Signed in as **admin**\n- leave on\n- payroll off";
  const box = screen.getByLabelText("Preconditions") as HTMLTextAreaElement;
  expect(box.tagName).toBe("TEXTAREA");
  fireEvent.change(box, { target: { value: typed } });
  expect(screen.getByTestId("value").textContent).toBe(typed);

  const toggle = screen.getByRole("button", { name: "Preview preconditions" });
  expect(toggle.getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-pressed")).toBe("true");
  expect(screen.queryByLabelText("Preconditions")).toBeNull();
  expect(screen.getByText("admin").tagName).toBe("STRONG");
  expect(document.querySelectorAll(".md-preview li")).toHaveLength(2);

  // Back to editing: the Markdown is exactly as typed.
  fireEvent.click(toggle);
  expect((screen.getByLabelText("Preconditions") as HTMLTextAreaElement).value).toBe(typed);
});

/// jsdom draws no list markers, so the rule that shows a typed `1)` is
/// pinned at the source: an ordered list marked `paren` gets `)` markers.
test("a prerequisites list typed with ) is drawn with ) markers", () => {
  const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../index.css"), "utf8");
  expect(css).toMatch(/\.case-text ol\.paren > li::marker\s*\{[^}]*content:\s*counter\(list-item\) "\) "/);
  const { container } = render(<CaseText kind="prereq" text={"1) Login\n2) Open Leave"} />);
  expect(container.querySelector("ol.paren")).not.toBeNull();
});
