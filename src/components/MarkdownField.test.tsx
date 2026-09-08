import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test } from "vitest";
import MarkdownField from "./MarkdownField";
import { renderMarkdown } from "../lib/markdown";

/** Mirrors the drawer: the parent owns both the value and whether the
 * field is open, exactly as WorkItemDrawer's editingFields does. */
function Harness({ initial = "**bold** text" }: { initial?: string }) {
  const [value, setValue] = useState(initial);
  const [editing, setEditing] = useState(false);
  return (
    <MarkdownField
      label="Description"
      value={value}
      onChange={setValue}
      editing={editing}
      onStartEditing={() => setEditing(true)}
      renderHtml={(md) => renderMarkdown(md || "*Nothing to preview*")}
    />
  );
}

const editor = () => screen.getByLabelText("Description (markdown)") as HTMLTextAreaElement;

test("it starts rendered, not as a text box", () => {
  render(<Harness />);
  expect(screen.queryByLabelText("Description (markdown)")).not.toBeInTheDocument();
  expect(document.querySelector(".md-preview strong")?.textContent).toBe("bold");
});

test("clicking the rendered block opens the editor", () => {
  render(<Harness />);
  fireEvent.click(document.querySelector(".md-preview")!);
  expect(editor()).toBeInTheDocument();
  expect(editor().value).toBe("**bold** text");
});

test("there is a keyboard route into the editor too", () => {
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "Edit Description" }));
  expect(editor()).toBeInTheDocument();
});

test("editing shows the toolbar and a live preview of the result", () => {
  render(<Harness initial="" />);
  fireEvent.click(document.querySelector(".md-preview")!);

  expect(screen.getByRole("button", { name: "Bold" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Numbered list" })).toBeInTheDocument();
  expect(screen.getByText("Preview")).toBeInTheDocument();

  fireEvent.change(editor(), { target: { value: "# Title\n\n- one\n- two" } });
  // The preview under the editor tracks what was typed.
  const preview = document.querySelectorAll(".md-preview");
  const live = preview[preview.length - 1];
  expect(live.querySelector("h1")?.textContent).toBe("Title");
  expect(live.querySelectorAll("li")).toHaveLength(2);
});

test("a toolbar button formats the selection", () => {
  render(<Harness initial="make this bold" />);
  fireEvent.click(document.querySelector(".md-preview")!);

  const el = editor();
  el.setSelectionRange(10, 14); // "bold"
  fireEvent.click(screen.getByRole("button", { name: "Bold" }));

  expect(el.value).toBe("make this **bold**");
});

test("a toolbar button works with nothing selected", () => {
  render(<Harness initial="" />);
  fireEvent.click(document.querySelector(".md-preview")!);
  fireEvent.click(screen.getByRole("button", { name: "Bulleted list" }));
  expect(editor().value).toBe("- ");
});

test("an empty field previews a placeholder rather than an empty box", () => {
  render(<Harness initial="" />);
  expect(document.querySelector(".md-preview")?.textContent).toContain("Nothing to preview");
});
