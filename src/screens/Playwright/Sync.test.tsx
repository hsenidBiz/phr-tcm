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
const ROWS: SyncRow[] = [
  { case_id: 10, title: "Ten", state: { kind: "Current" }, exported_at: "2026-10-01T10:00:00Z", has_script: true },
  { case_id: 11, title: "Eleven", state: { kind: "OutOfDate", why: "changed since the export: steps" }, exported_at: "2026-10-01T10:00:00Z", has_script: true },
  { case_id: 12, title: "Twelve", state: { kind: "NotInRepo" }, exported_at: "2026-10-01T10:00:00Z", has_script: true },
  { case_id: 13, title: "Thirteen", state: { kind: "NeverExported" }, exported_at: null, has_script: true },
  { case_id: 14, title: "Fourteen", state: { kind: "OutOfDate", why: "its script is no longer in Auto Run" }, exported_at: "2026-10-01T10:00:00Z", has_script: false },
];

type Call = { cmd: string; args: Record<string, unknown> };

function mount(opts: { update?: (args: Record<string, unknown>) => ExportResult | Promise<never> } = {}) {
  const calls: Call[] = [];
  let rows = ROWS;
  mockIPC(
    (cmd, raw) => {
      const args = (raw ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      if (cmd === "pw_sync_states") return rows;
      if (cmd === "list_test_case_fields") return [];
      if (cmd === "pw_sync_update") {
        const cases = args.cases as number[];
        const r = opts.update?.(args) ?? {
          files: [],
          cases: cases.map((id) => [id, `case-${id}.spec.ts`]),
          user_keys: [],
          missing_navigation: [],
          sections: [],
          record_error: null,
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

test("each case shows its state", async () => {
  mount();
  expect(await screen.findByText("#10 Ten")).toBeTruthy();
  expect(screen.getByText(/every other section and file stay as they are/)).toBeTruthy();
  expect(document.body.textContent).not.toMatch(/nothing else in the repo/i);
  expect(screen.getByText("Current")).toBeTruthy();
  expect(screen.getByText("Out of date: changed since the export: steps")).toBeTruthy();
  expect(screen.getByText("Not in the repo")).toBeTruthy();
  expect(screen.getByText("Never exported")).toBeTruthy();
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
  expect(updates(m.calls)[0].args).toMatchObject({ clone: CLONE, org: "org", project: "proj", cases: [11] });
  expect(await screen.findByText(`#11 → case-11.spec.ts: ${NEEDS_REPO}`)).toBeTruthy();
  expect(m.updated()).toBe(1);
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
  await screen.findByText("#10 Ten");
  const before = m.calls.filter((c) => c.cmd === "pw_sync_states").length;
  await emit("autorun-scripts-changed", { case_ids: [10] });
  await waitFor(() => expect(m.calls.filter((c) => c.cmd === "pw_sync_states").length).toBeGreaterThan(before));
});
