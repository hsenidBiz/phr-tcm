// The download list reads the run's folder again every time it mounts, so a
// file removed since the last look shows as gone - even under a client
// whose defaults would keep the first answer for good.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import RunDownloads from "./RunDownloads";

afterEach(() => {
  clearMocks();
});

test("a file removed since the list last mounted shows as gone", async () => {
  let onDisk = [{ name: "a.csv", size: 12 }];
  mockIPC((cmd) => (cmd === "auto_run_download_sizes" ? onDisk : null));
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity, refetchOnMount: false } },
  });
  const steps = [{ downloads: ["a.csv"] }];
  const view = render(
    <QueryClientProvider client={qc}>
      <RunDownloads runId="run-1" steps={steps} />
    </QueryClientProvider>,
  );
  expect(await screen.findByText("12 bytes")).toBeInTheDocument();
  view.unmount();

  onDisk = [];
  render(
    <QueryClientProvider client={qc}>
      <RunDownloads runId="run-1" steps={steps} />
    </QueryClientProvider>,
  );
  const list = screen.getByRole("list", { name: "Downloads" });
  expect(await within(list).findByText("no longer on this machine")).toBeInTheDocument();
  expect(within(list).getByRole("button", { name: "Open a.csv" })).toBeDisabled();
});
