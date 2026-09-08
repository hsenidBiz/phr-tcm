import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PowerRenameDialog, { type RenameTarget } from "./PowerRenameDialog";
import type { RenameRow } from "../lib/powerRename";

function mount(over: Partial<RenameTarget> = {}) {
  const apply = vi.fn(async (_rows: RenameRow[]) => [] as RenameRow[]);
  const target: RenameTarget = {
    label: "Azure DevOps",
    cases: [
      { id: 101, title: "TC-1 login works" },
      { id: 102, title: "TC-2 logout works" },
    ],
    undoable: true,
    apply,
    ...over,
  };
  render(<PowerRenameDialog target={target} onClose={vi.fn()} onDone={vi.fn()} />);
  return { apply };
}

const type = (label: string, value: string) =>
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
const rowFor = (id: string) => screen.getByText(id).closest("tr")!;

test("the preview updates as the rule is typed", () => {
  mount();
  type("Prefix", "Smoke: ");
  expect(within(rowFor("#101")).getByText("Smoke: TC-1 login works")).toBeInTheDocument();
  expect(within(rowFor("#102")).getByText("Smoke: TC-2 logout works")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Rename 2/ })).toBeEnabled();
});

/** The whole design: what is on screen IS the payload, not a forecast of
 *  what some other code will produce. */
test("apply sends exactly the strings the preview showed", async () => {
  const { apply } = mount();
  type("Find", "works");
  type("Replace with", "passes");

  // Read the preview, then apply, then compare against what was read.
  const shown = ["#101", "#102"].map(
    (id) => within(rowFor(id)).getAllByRole("cell")[2].textContent,
  );
  fireEvent.click(screen.getByRole("button", { name: /Rename 2/ }));

  await waitFor(() => expect(apply).toHaveBeenCalledTimes(1));
  expect(apply.mock.calls[0][0].map((r) => r.after)).toEqual(shown);
  expect(shown).toEqual(["TC-1 login passes", "TC-2 logout passes"]);
});

test("a title that would be empty blocks apply", () => {
  const { apply } = mount({ cases: [{ id: 101, title: "Login" }] });
  type("Find", "Login");
  expect(screen.getByText("A title cannot be empty.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Rename/ })).toBeDisabled();
  expect(apply).not.toHaveBeenCalled();
});

test("a title over the Azure DevOps limit blocks apply", () => {
  mount({ cases: [{ id: 101, title: "Login" }] });
  type("Suffix", "x".repeat(300));
  expect(screen.getByText(/Azure DevOps allows 255/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Rename/ })).toBeDisabled();
});

test("an invalid regex is reported and blocks apply", () => {
  mount();
  fireEvent.click(screen.getByLabelText(/Regular expression/i));
  type("Find", "(unclosed");
  expect(screen.getByRole("alert")).toHaveTextContent(/not valid/i);
  expect(screen.getByRole("button", { name: /Rename/ })).toBeDisabled();
});

/** A collision is legal here - this app treats a matching title as a
 *  duplicate and never as an update - so it warns and still applies. */
test("a collision warns but still applies", () => {
  mount({ cases: [{ id: 101, title: "Login v2" }], otherTitles: ["Login"] });
  type("Find", " v2");
  expect(screen.getByText(/already has this title/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Rename 1/ })).toBeEnabled();
});

/** Undo is offered only once something was actually written, and it is the
 *  same write in the other direction. */
test("undo puts the titles back", async () => {
  const { apply } = mount();
  expect(screen.queryByRole("button", { name: /Undo/ })).not.toBeInTheDocument();

  type("Prefix", "X ");
  fireEvent.click(screen.getByRole("button", { name: /Rename 2/ }));

  const undo = await screen.findByRole("button", { name: /Undo rename/ });
  fireEvent.click(undo);

  await waitFor(() => expect(apply).toHaveBeenCalledTimes(2));
  expect(apply.mock.calls[1][0].map((r) => r.after)).toEqual([
    "TC-1 login works",
    "TC-2 logout works",
  ]);
});

/** Drafts cannot fail to write, but Azure DevOps can - and a partial write
 *  must only offer back what actually landed. */
test("only the rows that were written can be undone", async () => {
  const apply = vi.fn(async (rows: RenameRow[]) => rows.slice(1));
  mount({ apply });
  type("Prefix", "X ");
  fireEvent.click(screen.getByRole("button", { name: /Rename 2/ }));

  const undo = await screen.findByRole("button", { name: /Undo rename/ });
  fireEvent.click(undo);
  await waitFor(() => expect(apply).toHaveBeenCalledTimes(2));
  expect(apply.mock.calls[1][0]).toHaveLength(1);
  expect(apply.mock.calls[1][0][0].after).toBe("TC-1 login works");
});
