// A script changed outside the screen (the assistant's save, a run clearing
// a mark, a delete) is read again when Rust says so: the Auto Run row
// updates with no reload.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { commands } from "../bindings";
import { setupViewKey } from "../screens/AutoRun/setupView";
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

/** A case's setup view, read the way the case card reads it. */
function Setup({ caseId }: { caseId: number }) {
  const q = useQuery({
    queryKey: setupViewKey("acme", "proj", caseId),
    queryFn: async () => {
      const r = await commands.autoRunSetupView("acme", "proj", caseId);
      return r.status === "ok" ? r.data : null;
    },
  });
  return <p>{q.data ? `setup ${caseId} ${q.data.fixture_name}` : `setup ${caseId} none`}</p>;
}

function SetupHost() {
  useAutoRunScriptsSync();
  return (
    <>
      <Setup caseId={1} />
      <Setup caseId={2} />
    </>
  );
}

test("the case card's setup view is read again with the script, for the cases named", async () => {
  const fixture: Record<number, string> = { 1: "old", 2: "old" };
  const reads: number[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_setup_view") {
        const id = (args as { caseId: number }).caseId;
        reads.push(id);
        return { fixture_name: fixture[id] };
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <SetupHost />
    </QueryClientProvider>,
  );
  await screen.findByText("setup 1 old");
  await screen.findByText("setup 2 old");
  fixture[1] = "new";
  fixture[2] = "new";
  reads.length = 0;
  await act(async () => {
    await emit("autorun-scripts-changed", { case_ids: [1] });
  });
  await screen.findByText("setup 1 new");
  expect(reads).toEqual([1]);
  expect(screen.getByText("setup 2 old")).toBeInTheDocument();

  // No case named: every setup view is read again.
  await act(async () => {
    await emit("autorun-scripts-changed", { case_ids: [] });
  });
  await screen.findByText("setup 2 new");
});

test("a scripts-changed event also reads the divergences again", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let fetches = 0;
  function Divs() {
    useQuery({
      queryKey: ["autorun-divergences", "acme", "Web"],
      queryFn: async () => {
        fetches += 1;
        return [];
      },
    });
    return null;
  }
  function Both() {
    useAutoRunScriptsSync();
    return <Divs />;
  }
  render(
    <QueryClientProvider client={qc}>
      <Both />
    </QueryClientProvider>,
  );
  await waitFor(() => expect(fetches).toBe(1));
  await act(async () => {
    await emit("autorun-scripts-changed", { case_ids: [7] });
  });
  await waitFor(() => expect(fetches).toBe(2));
});
