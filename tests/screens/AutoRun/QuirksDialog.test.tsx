// The Known quirks dialog (opened from Auto Test's Setup card): each note
// with who wrote it, when and what the runs since have said, and the
// buttons to add, edit, retire, restore and delete one - every change its
// own command, saved as it is made. Closing with a note typed and not
// saved asks first. (These tests moved here from RecipeEditor.test.tsx,
// where the list used to live.)

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Quirk_Serialize } from "../../../src/bindings";
import QuirksDialog from "../../../src/screens/AutoRun/QuirksDialog";

afterEach(() => { clearMocks(); vi.clearAllMocks(); });

/** 2026-10-01T00:00:00Z */
const OCT_1 = "1790812800000";

function quirk(id: string, text: string, extra: Partial<Quirk_Serialize> = {}): Quirk_Serialize {
  return {
    id,
    text,
    by: "assistant",
    at: OCT_1,
    sources: [],
    confirmed: 0,
    last_confirmed: null,
    doubted: 0,
    status: "active",
    retired_reason: null,
    retired_at: null,
    retired_by: null,
    from: "autorun",
    ...extra,
  };
}

type Call = { cmd: string; args: Record<string, unknown> };

/** The quirk commands, acting on one list the way the Rust side does -
 * every call answers with the list as saved. `refuse` makes a command
 * fail with the given sentence instead. */
function mount(quirks: Quirk_Serialize[] = [], refuse: Record<string, string> = {}) {
  let list = [...quirks];
  const calls: Call[] = [];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    if (cmd === "auto_run_load_quirks") return list;
    if (!cmd.startsWith("auto_run_") || !cmd.endsWith("_quirk")) return null;
    calls.push({ cmd, args });
    if (refuse[cmd]) throw new Error(refuse[cmd]);
    const id = args.id as string;
    if (cmd === "auto_run_add_quirk") {
      list = [...list, quirk(`q${list.length + 100}`, String(args.text).trim(), { by: "person", at: "1790899200000" })];
    }
    if (cmd === "auto_run_edit_quirk") list = list.map((q) => (q.id === id ? { ...q, text: String(args.text).trim() } : q));
    if (cmd === "auto_run_retire_quirk") {
      list = list.map((q) =>
        q.id === id ? { ...q, status: "retired", retired_reason: (args.reason as string | null) ?? null, retired_at: OCT_1 } : q,
      );
    }
    if (cmd === "auto_run_restore_quirk") {
      list = list.map((q) => (q.id === id ? { ...q, status: "active", retired_reason: null, retired_at: null } : q));
    }
    if (cmd === "auto_run_delete_quirk") list = list.filter((q) => q.id !== id);
    return list;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <QuirksDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { onClose, calls };
}

test("each note shows who wrote it, when, what the runs since have said, and its cases", async () => {
  mount([
    quirk("q1", "the grid paginates at 50 rows", { by: "person" }),
    quirk("q2", "the save button needs the form to settle", {
      sources: [{ case_id: 7, steps: [2, 3], class: "not_found" }],
      confirmed: 3,
      last_confirmed: OCT_1,
    }),
    quirk("q3", "the menu opens on hover", { sources: [{ case_id: 9, steps: [1] }], doubted: 2 }),
    quirk("q4", "the leave handler wants a CSRF header", { from: "api" }),
    quirk("q5", "an old note", { status: "retired", retired_reason: "the menu changed", retired_at: OCT_1 }),
  ]);
  const active = await screen.findByRole("list", { name: "Active quirks" });
  const person = within(active).getByRole("listitem", { name: "the grid paginates at 50 rows" });
  expect(person).toHaveTextContent("Person - 2026-10-01");
  expect(person).not.toHaveTextContent("Confirmed");

  const confirmed = within(active).getByRole("listitem", { name: "the save button needs the form to settle" });
  expect(confirmed).toHaveTextContent("Assistant - 2026-10-01");
  expect(confirmed).toHaveTextContent("Confirmed 3x, last 2026-10-01");
  expect(confirmed).toHaveTextContent("From cases: 7 (steps 2, 3)");

  const doubted = within(active).getByRole("listitem", { name: "the menu opens on hover" });
  expect(doubted).toHaveTextContent("Did not help 2x");
  expect(doubted).toHaveTextContent("From cases: 9 (step 1)");

  expect(within(active).getByRole("listitem", { name: "the leave handler wants a CSRF header" })).toHaveTextContent(
    "Assistant (API templates)",
  );

  // Retired notes are folded away under their own count.
  expect(within(active).queryByText("an old note")).not.toBeInTheDocument();
  const toggle = screen.getByRole("button", { name: "Retired (1)" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  const retired = screen.getByRole("list", { name: "Retired quirks" });
  expect(within(retired).getByRole("listitem", { name: "an old note" })).toHaveTextContent("Retired 2026-10-01: the menu changed");
});

test("a note typed in is added as the person's, and the input clears", async () => {
  const { calls } = mount([]);
  expect(await screen.findByText("No notes yet.")).toBeInTheDocument();
  const input = screen.getByLabelText("Add a note") as HTMLInputElement;
  expect(screen.getByRole("button", { name: "Add note" })).toBeDisabled();
  fireEvent.change(input, { target: { value: "the grid paginates at 50 rows" } });
  fireEvent.click(screen.getByRole("button", { name: "Add note" }));
  const row = await screen.findByRole("listitem", { name: "the grid paginates at 50 rows" });
  expect(row).toHaveTextContent("Person");
  expect(calls).toEqual([
    { cmd: "auto_run_add_quirk", args: { organization: "acme", project: "Web", text: "the grid paginates at 50 rows" } },
  ]);
  expect(input.value).toBe("");
});

test("a full list refuses the note in the app's own words, and the typed note stays", async () => {
  const refusal =
    'This project already has 40 active notes - retire one before adding another. Best candidates - written by an assistant, never confirmed or more often unhelpful than helpful, oldest first: q1 "dates render as dd/mm".';
  mount([quirk("q1", "dates render as dd/mm")], { auto_run_add_quirk: refusal });
  const input = (await screen.findByLabelText("Add a note")) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "one more" } });
  fireEvent.click(screen.getByRole("button", { name: "Add note" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(refusal);
  expect(input.value).toBe("one more");
});

test("Edit changes a note's text in place", async () => {
  const { calls } = mount([quirk("q1", "dates render as dd/mm")]);
  const row = await screen.findByRole("listitem", { name: "dates render as dd/mm" });
  fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
  const box = within(row).getByLabelText("Note text");
  fireEvent.change(box, { target: { value: "dates render as dd/mm/yyyy" } });
  fireEvent.click(within(row).getByRole("button", { name: "Save note" }));
  expect(await screen.findByRole("listitem", { name: "dates render as dd/mm/yyyy" })).toBeInTheDocument();
  expect(calls).toEqual([
    { cmd: "auto_run_edit_quirk", args: { organization: "acme", project: "Web", id: "q1", text: "dates render as dd/mm/yyyy" } },
  ]);
});

test("Retire takes an optional reason, and Restore brings the note back", async () => {
  const { calls } = mount([quirk("q1", "dates render as dd/mm"), quirk("q2", "the grid paginates")]);
  const row = await screen.findByRole("listitem", { name: "dates render as dd/mm" });
  fireEvent.click(within(row).getByRole("button", { name: "Retire" }));
  fireEvent.change(within(row).getByLabelText("Why retire it (optional)"), { target: { value: "the format changed" } });
  fireEvent.click(within(row).getByRole("button", { name: "Retire note" }));
  await screen.findByRole("button", { name: "Retired (1)" });
  expect(within(screen.getByRole("list", { name: "Active quirks" })).queryByText("dates render as dd/mm")).not.toBeInTheDocument();
  expect(calls[0]).toEqual({
    cmd: "auto_run_retire_quirk",
    args: { organization: "acme", project: "Web", id: "q1", reason: "the format changed" },
  });

  // With no reason given, none is sent.
  const other = screen.getByRole("listitem", { name: "the grid paginates" });
  fireEvent.click(within(other).getByRole("button", { name: "Retire" }));
  fireEvent.click(within(other).getByRole("button", { name: "Retire note" }));
  await screen.findByRole("button", { name: "Retired (2)" });
  expect(calls[1].args.reason).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Retired (2)" }));
  const retired = screen.getByRole("list", { name: "Retired quirks" });
  const back = within(retired).getByRole("listitem", { name: "dates render as dd/mm" });
  expect(back).toHaveTextContent("the format changed");
  fireEvent.click(within(back).getByRole("button", { name: "Restore" }));
  await waitFor(() =>
    expect(within(screen.getByRole("list", { name: "Active quirks" })).getByText("dates render as dd/mm")).toBeInTheDocument(),
  );
  expect(calls[2]).toEqual({ cmd: "auto_run_restore_quirk", args: { organization: "acme", project: "Web", id: "q1" } });
});

test("Delete asks first, and Cancel leaves the note alone", async () => {
  const { calls } = mount([quirk("q1", "dates render as dd/mm")]);
  const row = await screen.findByRole("listitem", { name: "dates render as dd/mm" });
  fireEvent.click(within(row).getByRole("button", { name: "Delete" }));
  expect(within(row).getByText(/Delete this note for good\?/)).toBeInTheDocument();
  fireEvent.click(within(row).getByRole("button", { name: "Cancel" }));
  expect(within(row).queryByText(/Delete this note for good\?/)).not.toBeInTheDocument();
  expect(calls).toEqual([]);

  fireEvent.click(within(row).getByRole("button", { name: "Delete" }));
  fireEvent.click(within(row).getByRole("button", { name: "Delete note" }));
  await waitFor(() => expect(screen.queryByRole("listitem", { name: "dates render as dd/mm" })).not.toBeInTheDocument());
  expect(calls).toEqual([{ cmd: "auto_run_delete_quirk", args: { organization: "acme", project: "Web", id: "q1" } }]);
});

test("while one note is open, the other notes wait, and Enter adds a note once", async () => {
  const { calls } = mount([quirk("q1", "dates render as dd/mm"), quirk("q2", "the grid paginates")]);
  const first = await screen.findByRole("listitem", { name: "dates render as dd/mm" });
  const second = screen.getByRole("listitem", { name: "the grid paginates" });
  fireEvent.click(within(first).getByRole("button", { name: "Edit" }));
  fireEvent.change(within(first).getByLabelText("Note text"), { target: { value: "dates render as dd/mm/yyyy" } });
  for (const name of ["Edit", "Retire", "Delete"]) {
    expect(within(second).getByRole("button", { name })).toBeDisabled();
  }
  fireEvent.click(within(first).getByRole("button", { name: "Cancel" }));
  expect(within(second).getByRole("button", { name: "Edit" })).toBeEnabled();

  const input = screen.getByLabelText("Add a note");
  fireEvent.change(input, { target: { value: "the toast fades after 3s" } });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.keyDown(input, { key: "Enter" });
  await screen.findByRole("listitem", { name: "the toast fades after 3s" });
  expect(calls.filter((c) => c.cmd === "auto_run_add_quirk")).toHaveLength(1);
});

test("the dialog is named Known quirks, with its word defined and its list under it", async () => {
  mount();
  const heading = screen.getByRole("heading", { name: "Known quirks", level: 2 });
  expect(screen.getByRole("dialog", { name: "Known quirks" })).toContainElement(heading);
  expect(screen.getByText("quirks")).toHaveAttribute("data-term", "quirk");
  expect(await screen.findByRole("button", { name: "Add note" })).toBeInTheDocument();
  expect(screen.getByText(/Each change is saved as you make it\./)).toBeInTheDocument();
});

test("with nothing typed, Close closes at once", async () => {
  const { onClose } = mount([quirk("q1", "dates render as dd/mm")]);
  await screen.findByRole("listitem", { name: "dates render as dd/mm" });
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("group", { name: "Close without saving?" })).not.toBeInTheDocument();
});

test("closing with a note typed and not saved asks first; Keep editing stays, Discard and close leaves", async () => {
  const { onClose, calls } = mount();
  const input = (await screen.findByLabelText("Add a note")) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "the grid paginates at 50 rows" } });

  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  const ask = await screen.findByRole("group", { name: "Close without saving?" });
  expect(onClose).not.toHaveBeenCalled();

  fireEvent.click(within(ask).getByRole("button", { name: "Keep editing" }));
  expect(screen.queryByRole("group", { name: "Close without saving?" })).not.toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
  expect(input.value).toBe("the grid paginates at 50 rows");

  // Escape asks the same way.
  fireEvent.keyDown(window, { key: "Escape" });
  const again = await screen.findByRole("group", { name: "Close without saving?" });
  expect(onClose).not.toHaveBeenCalled();

  fireEvent.click(within(again).getByRole("button", { name: "Discard and close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(calls).toEqual([]);
});

test("an edit left open counts as unsaved, and once saved it does not", async () => {
  const { onClose } = mount([quirk("q1", "dates render as dd/mm")]);
  const row = await screen.findByRole("listitem", { name: "dates render as dd/mm" });
  fireEvent.click(within(row).getByRole("button", { name: "Edit" }));
  fireEvent.change(within(row).getByLabelText("Note text"), { target: { value: "dates render as dd/mm/yyyy" } });
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(await screen.findByRole("group", { name: "Close without saving?" })).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  fireEvent.click(within(row).getByRole("button", { name: "Save note" }));
  await screen.findByRole("listitem", { name: "dates render as dd/mm/yyyy" });
  await waitFor(() => expect(screen.getByRole("button", { name: "Close" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});
