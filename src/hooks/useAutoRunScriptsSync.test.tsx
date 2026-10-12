// A script changed outside the screen (the assistant's save, a run clearing
// a mark, a delete) is read again when Rust says so: the Auto Run row
// updates with no reload.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { commands } from "../bindings";
import { scriptQueryKey, useAutoRunScriptsSync } from "./useAutoRunScriptsSync";

afterEach(() => {
  clearMocks();
});

/** One case's script, read the way the Auto Run rows read it. */
function Row({ caseId }: { caseId: number }) {
  const q = useQuery({
    queryKey: scriptQueryKey(caseId),
    queryFn: async () => {
      const r = await commands.autoRunLoadScript(caseId);
      return r.status === "ok" ? r.data : null;
    },
  });
  return <p>{q.data ? `#${caseId} ${q.data.title}` : `#${caseId} no script`}</p>;
}

function Host() {
  useAutoRunScriptsSync();
  return (
    <>
      <Row caseId={1} />
      <Row caseId={2} />
    </>
  );
}

test("a scripts-changed event reads again only the cases it names", async () => {
  const titles: Record<number, string | null> = { 1: null, 2: "first" };
  const loads: number[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_load_script") {
        const id = (args as { caseId: number }).caseId;
        loads.push(id);
        const title = titles[id];
        return title ? { case_id: id, title, steps: [] } : null;
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Host />
    </QueryClientProvider>,
  );
  await screen.findByText("#1 no script");
  await screen.findByText("#2 first");
  expect(loads.sort()).toEqual([1, 2]);

  // The assistant saved a script for case 1 and changed case 2's; only
  // case 1 is said to have changed.
  titles[1] = "added by the assistant";
  titles[2] = "changed";
  loads.length = 0;
  await act(async () => {
    await emit("autorun-scripts-changed", { case_ids: [1] });
  });
  await screen.findByText("#1 added by the assistant");
  expect(loads).toEqual([1]);
  expect(screen.getByText("#2 first")).toBeInTheDocument();

  // A delete moves case 2's aside: its row says it has none.
  titles[2] = null;
  await act(async () => {
    await emit("autorun-scripts-changed", { case_ids: [2] });
  });
  await waitFor(() => expect(screen.getByText("#2 no script")).toBeInTheDocument());
});

test("an event naming no case reads every script again", async () => {
  let loads = 0;
  mockIPC(
    (cmd) => {
      if (cmd === "auto_run_load_script") {
        loads += 1;
        return null;
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Host />
    </QueryClientProvider>,
  );
  await screen.findByText("#2 no script");
  await waitFor(() => expect(loads).toBe(2));
  await act(async () => {
    await emit("autorun-scripts-changed", { case_ids: [] });
  });
  await waitFor(() => expect(loads).toBe(4));
});
