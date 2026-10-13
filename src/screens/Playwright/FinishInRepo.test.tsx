// Finish in the repo: one card per feature folder, the spec picker, the
// read-only prompt and Copy. Rust builds and checks the prompt
// (tests/suite/pw_repo_handoff.rs); here it is a mocked command that
// echoes the choice. Every value is made up.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ExportResult, HandoffCard, HandoffChoice } from "../../bindings";
import FinishInRepo, { HANDOFF_NOTE } from "./FinishInRepo";

const copyText = vi.hoisted(() => vi.fn(async (_text: string) => {}));
vi.mock("../../lib/clipboard", () => ({ copyText: (t: string) => copyText(t) }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const PC = "sl/admin/performance/pc-cycle-setup";
const RM = "sl/admin/performance/rating-methods";

function result(ids: number[]): ExportResult {
  return {
    files: [],
    cases: ids.map((id) => [id, `case-${id}.spec.ts`]),
    user_keys: [],
    missing_navigation: [],
    sections: [],
    record_error: null,
    skipped: [],
  };
}

/** A stand-in for Rust: PC's cases default to its one spec, RM's to one per case. */
function cardsFor(cases: number[], choices: HandoffChoice[]): HandoffCard[] {
  const groups: [string, number[], string[]][] = [
    [PC, cases.filter((id) => id < 200), ["pc-cycle-setup"]],
    [RM, cases.filter((id) => id >= 200), ["rating-items", "rating-methods"]],
  ];
  return groups
    .filter(([, ids]) => ids.length > 0)
    .map(([seg, ids, stems]) => {
      const chosen = choices.find((c) => c.seg === seg);
      const stem = chosen ? chosen.stem : stems.length === 1 ? stems[0] : null;
      return { seg, case_ids: ids, stems, stem, prompt: `prompt ${seg} ${ids.join(" ")} ${stem ?? "one-per-case"}` };
    });
}

type Call = { cmd: string; args: Record<string, unknown> };

function mount(ids: number[], refuse?: string) {
  const calls: Call[] = [];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    if (cmd === "pw_handoff") {
      if (refuse) throw refuse;
      return cardsFor(args.cases as number[], args.choices as HandoffChoice[]);
    }
    return null;
  });
  render(<FinishInRepo clone={"C:\\my clone"} org="org" project="proj" result={result(ids)} />);
  return calls;
}

const handoffs = (calls: Call[]) => calls.filter((c) => c.cmd === "pw_handoff");
const last = (calls: Call[]) => handoffs(calls)[handoffs(calls).length - 1];

test("one card with the default spec, the prompt read-only, and Copy", async () => {
  const calls = mount([101, 102]);
  const box = (await screen.findByLabelText(`Prompt for ${PC}`)) as HTMLTextAreaElement;
  expect(box.value).toBe(`prompt ${PC} 101 102 pc-cycle-setup`);
  expect(box.readOnly).toBe(true);
  expect(screen.getByText(HANDOFF_NOTE)).toBeTruthy();
  expect(screen.getByText(`${PC}: #101, #102`)).toBeTruthy();
  expect(handoffs(calls)[0].args).toMatchObject({ clone: "C:\\my clone", org: "org", project: "proj", cases: [101, 102], choices: [] });
  expect(screen.getByRole("combobox", { name: `Spec for ${PC}` }).textContent).toContain("Add to pc-cycle-setup.spec.ts");

  fireEvent.click(screen.getByRole("button", { name: `Copy the prompt for ${PC}` }));
  await waitFor(() => expect(copyText).toHaveBeenCalledWith(`prompt ${PC} 101 102 pc-cycle-setup`));
  expect(await screen.findByText("Copied")).toBeTruthy();
});

test("several feature folders make one card each, in order", async () => {
  mount([101, 201]);
  expect(await screen.findByLabelText(`Prompt for ${PC}`)).toBeTruthy();
  expect(screen.getByLabelText(`Prompt for ${RM}`)).toBeTruthy();
  const boxes = screen.getAllByRole("textbox") as HTMLTextAreaElement[];
  expect(boxes.map((b) => b.value)).toEqual([`prompt ${PC} 101 pc-cycle-setup`, `prompt ${RM} 201 one-per-case`]);
  expect(screen.getByRole("combobox", { name: `Spec for ${RM}` }).textContent).toContain("One spec per case");
});

test("the picker sends the choice for its folder only", async () => {
  const calls = mount([101, 201]);
  await screen.findByLabelText(`Prompt for ${RM}`);
  fireEvent.click(screen.getByRole("combobox", { name: `Spec for ${RM}` }));
  fireEvent.click(await screen.findByRole("option", { name: "Add to rating-items.spec.ts" }));
  await waitFor(() =>
    expect((screen.getByLabelText(`Prompt for ${RM}`) as HTMLTextAreaElement).value).toBe(`prompt ${RM} 201 rating-items`),
  );
  expect(last(calls)?.args.choices).toEqual([{ seg: RM, stem: "rating-items" }]);

  fireEvent.click(screen.getByRole("combobox", { name: `Spec for ${PC}` }));
  fireEvent.click(await screen.findByRole("option", { name: "One spec per case" }));
  await waitFor(() =>
    expect((screen.getByLabelText(`Prompt for ${PC}`) as HTMLTextAreaElement).value).toBe(`prompt ${PC} 101 one-per-case`),
  );
  expect(last(calls)?.args.choices).toEqual([
    { seg: RM, stem: "rating-items" },
    { seg: PC, stem: null },
  ]);
});

test("a new spec name is sent only once it follows the rule", async () => {
  const calls = mount([101]);
  await screen.findByLabelText(`Prompt for ${PC}`);
  fireEvent.click(screen.getByRole("combobox", { name: `Spec for ${PC}` }));
  fireEvent.click(await screen.findByRole("option", { name: "New spec…" }));
  const name = screen.getByLabelText(`New spec name for ${PC}`);
  const before = handoffs(calls).length;
  fireEvent.change(name, { target: { value: "Cycle Dates" } });
  expect(screen.getByText("Use lower-case letters, digits and hyphens")).toBeTruthy();
  expect(handoffs(calls).length).toBe(before);
  fireEvent.change(name, { target: { value: "cycle-dates" } });
  await waitFor(() =>
    expect((screen.getByLabelText(`Prompt for ${PC}`) as HTMLTextAreaElement).value).toBe(`prompt ${PC} 101 cycle-dates`),
  );
  expect(last(calls)?.args.choices).toEqual([{ seg: PC, stem: "cycle-dates" }]);
  expect(screen.queryByText("Use lower-case letters, digits and hyphens")).toBeNull();
});

test("a refusal is shown, and nothing written shows nothing", async () => {
  mount([101], "Case 101 was not exported by this app, so there is nothing to finish for it here.");
  expect(await screen.findByText(/Case 101 was not exported by this app/)).toBeTruthy();
  expect(screen.queryByRole("textbox")).toBeNull();
  clearMocks();
});

test("an export that wrote nothing has no card and asks nothing", () => {
  const calls = mount([]);
  expect(screen.queryByText("Finish in the repo")).toBeNull();
  expect(handoffs(calls)).toHaveLength(0);
});
