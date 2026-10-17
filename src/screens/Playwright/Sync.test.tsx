// The Playwright tab's Sync: each case's state, Update out of date, Update
// ticked, and what an updated case still needs in the repo. Rust works out
// the states and does the write (tests/suite/pw_repo_sync.rs); here they are
// mocked commands. Every value is made up.

import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { ExportResult, SyncRow } from "../../bindings";
import Sync, { NEEDS_REPO } from "./Sync";

afterEach(() => clearMocks());

const CLONE = "C:\\clone";
const IN = { in_repo: true, repo_note: "in fixture.md and index.json" };
const OUT = { in_repo: false, repo_note: null };
const ROWS: SyncRow[] = [
  { case_id: 10, title: "Ten", state: { kind: "Current" }, exported_at: "2026-10-01T10:00:00Z", has_script: true, ...IN },
  { case_id: 11, title: "Eleven", state: { kind: "OutOfDate", why: "changed since the export: steps" }, exported_at: "2026-10-01T10:00:00Z", has_script: true, ...IN },
  { case_id: 12, title: "Twelve", state: { kind: "NotInRepo" }, exported_at: "2026-10-01T10:00:00Z", has_script: true, ...OUT },
  { case_id: 13, title: "Thirteen", state: { kind: "NeverExported" }, exported_at: null, has_script: true, ...OUT },
  { case_id: 14, title: "Fourteen", state: { kind: "OutOfDate", why: "its script is no longer in Auto Run" }, exported_at: "2026-10-01T10:00:00Z", has_script: false, ...IN },
  { case_id: 15, title: "Fifteen", state: { kind: "InRepoNotFromApp" }, exported_at: null, has_script: true, in_repo: true, repo_note: "in specs/fifteen.spec.ts" },
  { case_id: 16, title: "Sixteen", state: { kind: "ExcludedInRepo" }, exported_at: null, has_script: true, in_repo: true, repo_note: "Excluded in the repo (in pc-cycle-setup.md)" },
];

type Call = { cmd: string; args: Record<string, unknown> };

function mount(opts: { update?: (args: Record<string, unknown>) => ExportResult | Promise<never>; rows?: SyncRow[] } = {}) {
  const calls: Call[] = [];
  let rows = opts.rows ?? ROWS;
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (cmd === "pw_sync_states") return rows;
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "pw_handoff") {
        const ids = args.cases as number[];
        return [{ seg: "sl/admin/pm", stem: "wizard", case_ids: ids, prompt: `finish ${ids.join(" ")}`, problem: null, already: [] }];
      }
      if (cmd === "pw_sync_update" || cmd === "pw_sync_add_new") {
        const cases = (args.cases as number[] | undefined) ?? ROWS.filter((r) => r.has_script && !r.in_repo).map((r) => r.case_id);
        const r = opts.update?.(args) ?? {
          files: [],
          cases: cases.map((id) => [id, `case-${id}.spec.ts`]),
          user_keys: [],
          missing_navigation: [],
          sections: [],
          record_error: null,
          skipped: [],
        };
        rows = rows.map((row) => (cases.includes(row.case_id) ? { ...row, state: { kind: "Current" } } : row));
        return r;
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let updated = 0;
  render(
    <QueryClientProvider client={qc}>
      <Sync clone={CLONE} org="org" project="proj" onUpdated={() => (updated += 1)} />
    </QueryClientProvider>,
  );
  return { calls, updated: () => updated };
}

const updates = (calls: Call[]) => calls.filter((c) => c.cmd === "pw_sync_update");
const addNews = (calls: Call[]) => calls.filter((c) => c.cmd === "pw_sync_add_new");

test("each case shows its state", async () => {
  mount();
  expect(await screen.findByText("Ten")).toBeTruthy();
  expect(screen.getByText(/every other section and file stay as they are/)).toBeTruthy();
  expect(document.body.textContent).not.toMatch(/nothing else in the repo/i);
  expect(screen.getByText("Current")).toBeTruthy();
  expect(screen.getAllByText("Out of date")).toHaveLength(2);
  expect(screen.getByText("changed since the export: steps")).toBeTruthy();
  expect(screen.getByText("its script is no longer in Auto Run")).toBeTruthy();
  // Each state is a badge in its tone.
  const tone = (text: string) => screen.getByText(text).closest("[class*='bg-']")?.className ?? "";
  expect(tone("Current")).toContain("text-success");
  expect(tone("Never exported")).toContain("border-border");
  expect(tone("Not in the repo")).toContain("text-info");
  expect(tone("In the repo (not from this app)")).toContain("text-info");
  expect(tone("Excluded in the repo")).toContain("text-muted");
  expect(screen.getByText("#10")).toBeTruthy();
  expect(screen.getByText("Not in the repo")).toBeTruthy();
  expect(screen.getByText("Never exported")).toBeTruthy();
  expect(screen.getByText("In the repo (not from this app)")).toBeTruthy();
  expect(screen.getByText("in specs/fifteen.spec.ts")).toBeTruthy();
  // When, in local time, not the raw UTC stamp.
  const local = new Date("2026-10-01T10:00:00Z").toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
  expect(screen.getAllByText(`exported ${local}`).length).toBe(4);
  expect(document.body.textContent).not.toContain("2026-10-01T10:00:00Z");
  // A case whose script is gone cannot be ticked, nor counted as updatable.
  const gone = screen.getByRole("checkbox", { name: "Tick #14 Fourteen" });
  expect(gone.hasAttribute("disabled") || gone.getAttribute("aria-disabled") === "true" || gone.hasAttribute("data-disabled")).toBe(true);
  expect(screen.getByRole("button", { name: "Update out of date (1)" })).toBeTruthy();
  expect((screen.getByRole("button", { name: "Update ticked (0)" }) as HTMLButtonElement).disabled).toBe(true);
});

test("Update out of date sends only the out-of-date cases and says what each still needs", async () => {
  const m = mount();
  fireEvent.click(await screen.findByRole("button", { name: "Update out of date (1)" }));
  await waitFor(() => expect(updates(m.calls)).toHaveLength(1));
  // Out of date cases are this app's own: nothing is sent to replace.
  expect(updates(m.calls)[0].args).toMatchObject({ clone: CLONE, org: "org", project: "proj", cases: [11], replace: [] });
  expect(await screen.findByText(`#11 → case-11.spec.ts: ${NEEDS_REPO}`)).toBeTruthy();
  expect(m.updated()).toBe(1);
  // Finish in the repo: the prompt for exactly what was written.
  expect(((await screen.findByLabelText("Prompt for sl/admin/pm/wizard")) as HTMLTextAreaElement).value).toBe("finish 11");
  expect(m.calls.find((c) => c.cmd === "pw_handoff")?.args).toMatchObject({ clone: CLONE, org: "org", project: "proj", cases: [11] });
  // The states are read again.
  await waitFor(() => expect(screen.getByRole("button", { name: "Update out of date (0)" })).toBeTruthy());
});

test("Update ticked sends the ticked cases", async () => {
  const m = mount();
  fireEvent.click(await screen.findByRole("checkbox", { name: "Tick #12 Twelve" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Tick #13 Thirteen" }));
  fireEvent.click(screen.getByRole("button", { name: "Update ticked (2)" }));
  await waitFor(() => expect(updates(m.calls)).toHaveLength(1));
  expect(updates(m.calls)[0].args.cases).toEqual([12, 13]);
  expect(updates(m.calls)[0].args.replace).toEqual([12, 13]);
  expect(await screen.findByText(`#13 → case-13.spec.ts: ${NEEDS_REPO}`)).toBeTruthy();
});

test("a refused update shows why and changes nothing", async () => {
  const m = mount({
    update: () => {
      throw "case 11 cannot be exported: latest run: Failed. Nothing was written.";
    },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Update out of date (1)" }));
  expect(await screen.findByText(/latest run: Failed\. Nothing was written\./)).toBeTruthy();
  expect(m.updated()).toBe(0);
  expect(screen.queryByText(new RegExp(NEEDS_REPO))).toBeNull();
});

test("a script change anywhere reads the states again", async () => {
  const m = mount();
  await screen.findByText("Ten");
  const before = m.calls.filter((c) => c.cmd === "pw_sync_states").length;
  await emit("autorun-scripts-changed", { case_ids: [10] });
  await waitFor(() => expect(m.calls.filter((c) => c.cmd === "pw_sync_states").length).toBeGreaterThan(before));
});

test("Add new counts the cases the repo does not have and leaves the rest to Rust", async () => {
  const m = mount();
  // 12 (gone from the repo) and 13 (never exported); not 15, which the repo has.
  fireEvent.click(await screen.findByRole("button", { name: "Add new (2)" }));
  await waitFor(() => expect(addNews(m.calls)).toHaveLength(1));
  expect(addNews(m.calls)[0].args).toEqual({ clone: CLONE, org: "org", project: "proj", moduleRef: null, preconditionsRef: null });
  expect(updates(m.calls)).toHaveLength(0);
  expect(await screen.findByText(`#13 → case-13.spec.ts: ${NEEDS_REPO}`)).toBeTruthy();
  expect(m.updated()).toBe(1);
});

test("Sync explains Add new in plain words", async () => {
  mount();
  await screen.findByText("Ten");
  expect(document.body.textContent).toContain("Add new exports every case the repo does not have yet.");
  expect(document.body.textContent).not.toMatch(/[–—]/);
});

test("a case in the repo not from this app is replaced only when ticked", async () => {
  const m = mount();
  // Update out of date never sends it.
  fireEvent.click(await screen.findByRole("button", { name: "Update out of date (1)" }));
  await waitFor(() => expect(updates(m.calls)).toHaveLength(1));
  expect(updates(m.calls)[0].args.cases).not.toContain(15);
  // Ticked, it goes as chosen to replace.
  fireEvent.click(await screen.findByRole("checkbox", { name: "Tick #15 Fifteen" }));
  fireEvent.click(screen.getByRole("button", { name: "Update ticked (1)" }));
  await waitFor(() => expect(updates(m.calls)).toHaveLength(2));
  expect(updates(m.calls)[1].args).toMatchObject({ cases: [15], replace: [15] });
});

test("Add new lists each new case it left out, with why", async () => {
  const m = mount({
    update: () => ({
      files: ["suites/_generated/case-13.spec.ts"],
      cases: [[13, "case-13.spec.ts"]],
      user_keys: [],
      missing_navigation: [],
      sections: [],
      record_error: null,
      skipped: [[12, "latest run: Failed"]],
    }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "Add new (2)" }));
  await waitFor(() => expect(addNews(m.calls)).toHaveLength(1));
  expect(await screen.findByText(`#13 → case-13.spec.ts: ${NEEDS_REPO}`)).toBeTruthy();
  expect(screen.getByText("Not added, and why:")).toBeTruthy();
  expect(screen.getByText("#12: latest run: Failed")).toBeTruthy();
});

test("a case the repo excludes has its own state and cannot be ticked", async () => {
  const m = mount();
  expect(await screen.findByText("Excluded in the repo")).toBeTruthy();
  expect(screen.getByText("Excluded in the repo (in pc-cycle-setup.md)")).toBeTruthy();
  const box = screen.getByRole("checkbox", { name: "Tick #16 Sixteen" });
  expect(box.hasAttribute("disabled") || box.getAttribute("aria-disabled") === "true" || box.hasAttribute("data-disabled")).toBe(true);
  fireEvent.click(box);
  expect((screen.getByRole("button", { name: "Update ticked (0)" }) as HTMLButtonElement).disabled).toBe(true);
  // Neither Update out of date nor Add new counts it.
  expect(screen.getByRole("button", { name: "Update out of date (1)" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Add new (2)" })).toBeTruthy();
  expect(m.calls.some((c) => c.cmd === "pw_sync_update")).toBe(false);
});

test("a case that builds its own draft stays in TCM: said why, never ticked, never counted", async () => {
  const reason = "builds its own draft with TCM fixture `f1`; Playwright has no fixture mechanism - stays TCM-only";
  const m = mount({
    rows: [
      ...ROWS,
      { case_id: 17, title: "Seventeen", state: { kind: "NotExportable", reason }, exported_at: null, has_script: true, ...OUT },
    ],
  });
  expect(await screen.findByText("Seventeen")).toBeTruthy();
  expect(screen.getByText("Not exportable")).toBeTruthy();
  expect(screen.getByText(`Stays in TCM: ${reason}`)).toBeTruthy();
  const tone = screen.getByText("Not exportable").closest("[class*='bg-']")?.className ?? "";
  expect(tone).toContain("text-muted");
  const box = screen.getByRole("checkbox", { name: "Tick #17 Seventeen" });
  expect(box.hasAttribute("disabled") || box.getAttribute("aria-disabled") === "true" || box.hasAttribute("data-disabled")).toBe(true);
  // Add new counts the same two cases as without it.
  expect(screen.getByRole("button", { name: "Add new (2)" })).toBeTruthy();
  expect((screen.getByRole("button", { name: "Update ticked (0)" }) as HTMLButtonElement).disabled).toBe(true);
  expect(m.calls.some((c) => c.cmd === "pw_sync_update")).toBe(false);
});

test("an Add new that writes and skips nothing says there is nothing new", async () => {
  const m = mount({
    update: () => ({ files: [], cases: [], user_keys: [], missing_navigation: [], sections: [], record_error: null, skipped: [] }),
  });
  fireEvent.click(await screen.findByRole("button", { name: "Add new (2)" }));
  await waitFor(() => expect(addNews(m.calls)).toHaveLength(1));
  expect(await screen.findByText("Nothing new to add")).toBeTruthy();
  expect(screen.queryByLabelText("Updated")).toBeNull();
});
