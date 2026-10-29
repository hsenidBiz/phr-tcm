// Find and replace on the report screen: hits say where they are, a replace
// shows what it would change and asks before it saves, and says what it left.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { FindReplace } from "./FindReplace";

const MAIN_TEXT = {
  subreport: null,
  object_id: "o3",
  name: "Text3",
  kind: "Text",
  section: "ReportHeaderSection1",
  matched: "text",
  text: "Old title",
  replaceable: true,
};
const SUB_FIELD = {
  subreport: "Goals",
  object_id: "o1",
  name: "Field1",
  kind: "Field",
  section: "DetailSection1",
  matched: "field",
  text: "{Command.old_name}",
  replaceable: false,
};
const outcome = (over: Record<string, unknown> = {}) => ({
  replaced: 3,
  skipped_locked: 1,
  changes: [],
  applied: false,
  ...over,
});

function mock(hits: unknown[], replace: (args: Record<string, unknown>) => unknown = () => outcome()) {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === "crystal_find") return hits;
    if (cmd === "crystal_replace") return replace(args as Record<string, unknown>);
    return undefined;
  });
  return calls;
}

afterEach(() => clearMocks());

const type = (label: string, value: string) =>
  fireEvent.change(screen.getByRole("textbox", { name: label }), { target: { value } });

test("find lists each hit with where it is", async () => {
  const calls = mock([MAIN_TEXT, SUB_FIELD]);
  render(<FindReplace onChanged={() => {}} />);
  type("Find", "old");
  fireEvent.click(screen.getByRole("button", { name: "Find" }));
  expect(await screen.findByText("2 matches")).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "crystal_find")?.args).toEqual({ text: "old", matchCase: false });
  expect(screen.getByText("Text3")).toBeInTheDocument();
  expect(screen.getByText("ReportHeaderSection1")).toBeInTheDocument();
  expect(screen.getByText("Old title")).toBeInTheDocument();
  expect(screen.getByText("Field1")).toBeInTheDocument();
  expect(screen.getByText("Subreport: Goals")).toBeInTheDocument();
  expect(screen.getAllByText("Has fields, cannot be replaced")).toHaveLength(1);
});

test("no hits says so", async () => {
  mock([]);
  render(<FindReplace onChanged={() => {}} />);
  type("Find", "zzz");
  fireEvent.click(screen.getByRole("button", { name: "Find" }));
  expect(await screen.findByText("No matches.")).toBeInTheDocument();
});

test("replace all shows the count first, then applies on a yes and says what was skipped", async () => {
  const onChanged = vi.fn();
  const calls = mock([], (args) => outcome({ applied: args.dryRun === false }));
  render(<FindReplace onChanged={onChanged} />);
  type("Find", "Old");
  type("Replace with", "New");
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));

  expect(await screen.findByText(/Replace 3 text objects\?/)).toBeInTheDocument();
  const replaces = () => calls.filter((c) => c.cmd === "crystal_replace");
  expect(replaces()).toHaveLength(1);
  expect(replaces()[0].args).toEqual({ find: "Old", replace: "New", matchCase: false, dryRun: true });
  expect(onChanged).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Yes, replace" }));
  expect(await screen.findByText("Replaced 3. 1 skipped: its text holds fields.")).toBeInTheDocument();
  expect(replaces()[1].args).toEqual({ find: "Old", replace: "New", matchCase: false, dryRun: false });
  expect(onChanged).toHaveBeenCalledTimes(1);
});

test("several skipped are counted in the plural", async () => {
  mock([], (args) => outcome({ skipped_locked: 2, applied: args.dryRun === false }));
  render(<FindReplace onChanged={() => {}} />);
  type("Find", "Old");
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
  fireEvent.click(await screen.findByRole("button", { name: "Yes, replace" }));
  expect(await screen.findByText("Replaced 3. 2 skipped: their text holds fields.")).toBeInTheDocument();
});

test("cancelling the confirmation saves nothing", async () => {
  const onChanged = vi.fn();
  const calls = mock([]);
  render(<FindReplace onChanged={onChanged} />);
  type("Find", "Old");
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("button", { name: "Yes, replace" })).not.toBeInTheDocument();
  expect(calls.filter((c) => c.cmd === "crystal_replace")).toHaveLength(1);
  expect(onChanged).not.toHaveBeenCalled();
});

test("nothing to change asks for no confirmation", async () => {
  mock([], () => outcome({ replaced: 0, skipped_locked: 0 }));
  render(<FindReplace onChanged={() => {}} />);
  type("Find", "Old");
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
  expect(await screen.findByText("Nothing to replace.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Yes, replace" })).not.toBeInTheDocument();
});

test("match case is passed through", async () => {
  const calls = mock([]);
  render(<FindReplace onChanged={() => {}} />);
  type("Find", "Old");
  fireEvent.click(screen.getByRole("checkbox", { name: "Match case" }));
  fireEvent.click(screen.getByRole("button", { name: "Find" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "crystal_find")).toBe(true));
  expect(calls.find((c) => c.cmd === "crystal_find")?.args).toEqual({ text: "Old", matchCase: true });
});

test("an empty find disables both buttons", () => {
  mock([]);
  render(<FindReplace onChanged={() => {}} />);
  expect(screen.getByRole("button", { name: "Find" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Replace all" })).toBeDisabled();
  type("Find", "x");
  expect(screen.getByRole("button", { name: "Find" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Replace all" })).toBeEnabled();
});

test("a refusal is shown in a sentence", async () => {
  mock([], () => {
    throw "Could not save A.rpt: access denied. Nothing was saved.";
  });
  render(<FindReplace onChanged={() => {}} />);
  type("Find", "Old");
  fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not save A.rpt");
});
