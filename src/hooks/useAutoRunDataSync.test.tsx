// Data an assistant adds outside the screen (areas, components, the map,
// quirks, test files, accounts, fixtures) is read again when Rust says so:
// the Auto Run screen updates with no reload, and a burst of events is read
// once.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { AutorunDataKind } from "../bindings";
import { DATA_KIND_KEYS, useAutoRunDataSync } from "./useAutoRunDataSync";

afterEach(() => {
  clearMocks();
});

/** A query under `key`, counting how many times it was read. */
function Counted({ queryKey, counts, name }: { queryKey: unknown[]; counts: Record<string, number>; name: string }) {
  useQuery({
    queryKey,
    queryFn: async () => {
      counts[name] = (counts[name] ?? 0) + 1;
      return counts[name];
    },
  });
  return null;
}

function Host({ counts }: { counts: Record<string, number> }) {
  useAutoRunDataSync();
  return (
    <>
      <Counted name="nav" queryKey={["autorun-nav", "o", "p"]} counts={counts} />
      <Counted name="components" queryKey={["autorun-components", "o", "p"]} counts={counts} />
      <Counted name="fixtures" queryKey={["api-fixtures", "o", "p"]} counts={counts} />
    </>
  );
}

async function mount(counts: Record<string, number>) {
  mockIPC(() => null, { shouldMockEvents: true });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Host counts={counts} />
    </QueryClientProvider>,
  );
  await waitFor(() => expect(counts).toEqual({ nav: 1, components: 1, fixtures: 1 }));
}

test("a change to areas reads the areas again and not the components", async () => {
  const counts: Record<string, number> = {};
  await mount(counts);
  await act(async () => {
    await emit("autorun-data-changed", { kinds: ["areas"] });
  });
  await waitFor(() => expect(counts.nav).toBe(2));
  expect(counts.components).toBe(1);
  expect(counts.fixtures).toBe(1);
});

test("a change to fixtures reads the fixtures again", async () => {
  const counts: Record<string, number> = {};
  await mount(counts);
  await act(async () => {
    await emit("api-templates-changed", { id: "goals-remove-goal" });
  });
  await waitFor(() => expect(counts.fixtures).toBe(2));
  expect(counts.nav).toBe(1);
});

test("a burst of events is read once", async () => {
  const counts: Record<string, number> = {};
  await mount(counts);
  await act(async () => {
    for (let i = 0; i < 5; i++) await emit("autorun-data-changed", { kinds: ["areas"] });
    await emit("autorun-data-changed", { kinds: ["components"] });
  });
  await waitFor(() => expect(counts.components).toBe(2));
  expect(counts.nav).toBe(2);
  // Nothing more is read afterwards.
  await new Promise((r) => setTimeout(r, 500));
  expect(counts).toEqual({ nav: 2, components: 2, fixtures: 1 });
});

test("every kind of data has its query key", () => {
  const kinds: AutorunDataKind[] = ["areas", "components", "map", "mappingSummary", "quirks", "testFiles", "proposals"];
  for (const k of kinds) expect(DATA_KIND_KEYS[k].length).toBeGreaterThan(0);
  expect(Object.keys(DATA_KIND_KEYS).sort()).toEqual([...kinds].sort());
});
