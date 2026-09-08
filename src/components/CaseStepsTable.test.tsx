import { render, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import CaseStepsTable from "./CaseStepsTable";

const steps = [
  { action: "Open the published cycle.", expected: "The wizard opens at Step 1." },
  { action: "Read the Configuration Level header.", expected: "No Copy button is shown." },
];

/// The field existed everywhere except the app: it round-trips through the
/// JSON, renders in the browser page, and shows in a change diff - but no
/// screen displayed it. Someone who received a shared draft, which is
/// exactly who it is written for, had to export to a browser to read it.
test("expanding a case shows its reviewer notes", () => {
  render(
    <CaseStepsTable
      steps={steps}
      preconditions="A published cycle exists"
      reviewerNotes="Spec: **Step10-ManagePerformanceCycle.md** 7.7 (AC-3)"
    />,
  );

  expect(screen.getByText("Reviewer notes")).toBeInTheDocument();
  // Markdown, not raw text - a citation carries emphasis and links.
  expect(screen.getByText("Step10-ManagePerformanceCycle.md").tagName).toBe("STRONG");
  // Above the steps, the order the browser review page uses.
  const notes = screen.getByText("Reviewer notes");
  const table = screen.getByRole("table");
  expect(notes.compareDocumentPosition(table) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  // And the case still reads as before.
  expect(within(table).getByText("Open the published cycle.")).toBeInTheDocument();
  expect(screen.getByText(/A published cycle exists/)).toBeInTheDocument();
});

test("a case with no notes gets no empty panel", () => {
  const { rerender } = render(<CaseStepsTable steps={steps} />);
  expect(screen.queryByText("Reviewer notes")).not.toBeInTheDocument();

  // Whitespace is not a note either - the field is optional and an
  // assistant that wrote a blank line should not produce a heading.
  // Braces, not quotes: a JSX attribute string is literal, so "\n" in one
  // is a backslash and an n, which is not whitespace at all.
  rerender(<CaseStepsTable steps={steps} reviewerNotes={"   \n  "} />);
  expect(screen.queryByText("Reviewer notes")).not.toBeInTheDocument();
});

/// Reviewer notes are authored by an assistant from spec documents, so the
/// same rule as everywhere else applies: markup in the source must not
/// become markup in the app. `renderMarkdown` sanitises; this pins that the
/// call site actually uses it.
test("markup in a note cannot become live markup", () => {
  const { container } = render(
    <CaseStepsTable
      steps={steps}
      reviewerNotes={'Body is a <textarea> per spec.\n\n<img src=x onerror=alert(1)>'}
    />,
  );
  for (const el of container.querySelectorAll("*")) {
    for (const attr of el.attributes) {
      expect(attr.name.toLowerCase().startsWith("on")).toBe(false);
    }
  }
  expect(container.querySelector("img")?.getAttribute("onerror") ?? null).toBeNull();
  // The words survive even where the markup does not.
  expect(container.textContent).toContain("per spec");
});
