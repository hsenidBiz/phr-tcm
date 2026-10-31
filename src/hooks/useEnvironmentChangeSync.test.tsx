// A switch, a copy or an import in Rust reaches the screens through one
// event: the per-environment queries are read again, the list too.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { render, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useEnvironmentChangeSync } from "./useEnvironmentChangeSync";

afterEach(() => clearMocks());

function Probe() {
  useEnvironmentChangeSync();
  return null;
}

test("autorun-environment-changed resets per-environment queries and refreshes the list", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  const qc = new QueryClient();
  qc.setQueryData(["autorun-script", 5], { steps: [] });
  qc.setQueryData(["test-files", "o", "p"], []);
  qc.setQueryData(["environments"], { active: "a" });
  render(
    <QueryClientProvider client={qc}>
      <Probe />
    </QueryClientProvider>,
  );
  await new Promise((r) => setTimeout(r, 20));
  await emit("autorun-environment-changed", { env_id: "env-00000002" });
  await waitFor(() => expect(qc.getQueryData(["autorun-script", 5])).toBeUndefined());
  expect(qc.getQueryData(["test-files", "o", "p"])).toEqual([]);
  expect(qc.getQueryState(["environments"])?.isInvalidated).toBe(true);
});
