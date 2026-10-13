// Finish in the repo: one card per test-case file, the read-only prompt and
// Copy, a card that says why it has no prompt, the cases a spec already has,
// and a refusal clearing the cards. Rust builds and checks the prompt
// (tests/suite/pw_repo_handoff.rs); here it is a mocked command. Every value
// is made up.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ExportResult, HandoffCard } from "../../bindings";
import FinishInRepo, { HANDOFF_NOTE } from "./FinishInRepo";

const copyText = vi.hoisted(() => vi.fn(async (_text: string) => {}));
vi.mock("../../lib/clipboard", () => ({ copyText: (t: string) => copyText(t) }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const PC = "sl/admin/performance/pc-cycle-setup";
const RM = "sl/admin/performance/rating-methods";
const PC_LABEL = `${PC}/pc-cycle-setup`;
const RM_LABEL = `${RM}/rating-methods`;

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

/**
 * A stand-in for Rust: ids under 200 are pc-cycle-setup, the rest
 * rating-methods; 150 is already in a spec, 199 has no user key.
 */
function cardsFor(cases: number[]): HandoffCard[] {
  const groups: [string, string, number[]][] = [
    [PC, "pc-cycle-setup", cases.filter((id) => id < 200)],
    [RM, "rating-methods", cases.filter((id) => id >= 200)],
  ];
  return groups
    .filter(([, , ids]) => ids.length > 0)
    .map(([seg, stem, all]) => {
      const already = all.includes(150) ? ["#150 is already in specs/pc-cycle-setup.spec.ts: change that test with /edit-spec in the repo."] : [];
      const ids = all.filter((id) => id !== 150);
      if (ids.includes(199)) {
        return { seg, stem, case_ids: ids, prompt: "", problem: "Case 199 needs a user key mapped in the export first.", already };
      }
      const prompt = ids.length ? `prompt ${seg} ${ids.join(" ")}` : "";
      return { seg, stem, case_ids: ids, prompt, problem: null, already };
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
      return cardsFor(args.cases as number[]);
    }
    return null;
  });
  const view = render(<FinishInRepo clone={"C:\\my clone"} org="org" project="proj" result={result(ids)} />);
  return { calls, view };
}

const handoffs = (calls: Call[]) => calls.filter((c) => c.cmd === "pw_handoff");

test("one card with its file, the prompt read-only, and Copy", async () => {
  const { calls } = mount([101, 102]);
  const box = (await screen.findByLabelText(`Prompt for ${PC_LABEL}`)) as HTMLTextAreaElement;
  expect(box.value).toBe(`prompt ${PC} 101 102`);
  expect(box.readOnly).toBe(true);
  expect(screen.getByText(HANDOFF_NOTE)).toBeTruthy();
  expect(screen.getByText(`${PC}: #101, #102 into specs/pc-cycle-setup.spec.ts`)).toBeTruthy();
  expect(handoffs(calls)[0].args).toEqual({ clone: "C:\\my clone", org: "org", project: "proj", cases: [101, 102] });
  // No spec to pick: the spec is the test-case file's.
  expect(screen.queryByRole("combobox")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: `Copy the prompt for ${PC_LABEL}` }));
  await waitFor(() => expect(copyText).toHaveBeenCalledWith(`prompt ${PC} 101 102`));
  expect(await screen.findByText("Copied")).toBeTruthy();
});

test("several test-case files make one card each, in order", async () => {
  mount([101, 201]);
  expect(await screen.findByLabelText(`Prompt for ${PC_LABEL}`)).toBeTruthy();
  const boxes = screen.getAllByRole("textbox") as HTMLTextAreaElement[];
  expect(boxes.map((b) => b.value)).toEqual([`prompt ${PC} 101`, `prompt ${RM} 201`]);
  expect(screen.getByLabelText(`Prompt for ${RM_LABEL}`)).toBeTruthy();
});

test("a card with a problem shows only why, and the others still get a prompt", async () => {
  mount([150, 199, 201]);
  expect(await screen.findByText("Case 199 needs a user key mapped in the export first.")).toBeTruthy();
  // Only the sentence and the already-in-a-spec list: no folder line, no prompt, no Copy.
  expect(screen.getByText(/#150 is already in specs\/pc-cycle-setup\.spec\.ts/)).toBeTruthy();
  expect(screen.queryByText(new RegExp(`^${PC}:`))).toBeNull();
  expect(screen.queryByLabelText(`Prompt for ${PC_LABEL}`)).toBeNull();
  expect(screen.queryByRole("button", { name: `Copy the prompt for ${PC_LABEL}` })).toBeNull();
  expect((screen.getByLabelText(`Prompt for ${RM_LABEL}`) as HTMLTextAreaElement).value).toBe(`prompt ${RM} 201`);
});

test("a case a spec already has is listed beside the prompt, and alone shows no prompt", async () => {
  mount([101, 150]);
  expect(await screen.findByText(`${PC}: #101 into specs/pc-cycle-setup.spec.ts`)).toBeTruthy();
  expect(screen.getByText(/#150 is already in specs\/pc-cycle-setup\.spec\.ts: change that test with \/edit-spec/)).toBeTruthy();
  clearMocks();
});

test("every case already in a spec: the list only", async () => {
  mount([150]);
  expect(await screen.findByText(/#150 is already in/)).toBeTruthy();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.queryByRole("button", { name: /Copy the prompt/ })).toBeNull();
});

test("a refusal is shown and clears the cards of an earlier export", async () => {
  const calls: Call[] = [];
  let refuse = "";
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    if (cmd === "pw_handoff") {
      if (refuse) throw refuse;
      return cardsFor(args.cases as number[]);
    }
    return null;
  });
  const { rerender } = render(<FinishInRepo clone="C:/c" org="org" project="proj" result={result([101])} />);
  expect(await screen.findByLabelText(`Prompt for ${PC_LABEL}`)).toBeTruthy();
  refuse = "Case 102 was not exported by this app, so there is nothing to finish for it here.";
  rerender(<FinishInRepo clone="C:/c" org="org" project="proj" result={result([102])} />);
  expect(await screen.findByText(/Case 102 was not exported by this app/)).toBeTruthy();
  expect(screen.queryByRole("textbox")).toBeNull();
});

test("an export that wrote nothing has no card and asks nothing", () => {
  const { calls } = mount([]);
  expect(screen.queryByText("Finish in the repo")).toBeNull();
  expect(handoffs(calls)).toHaveLength(0);
});
