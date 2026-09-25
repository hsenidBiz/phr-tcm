import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { resetForTests } from "../lib/notifications";
import { toast } from "../lib/toast";
import { useMentions } from "./useMentions";

vi.mock("../lib/toast", () => ({ toast: { info: vi.fn() } }));

// announce() (the shared toast/OS-notification funnel, in the real,
// unmocked assignedAlerts module) checks document.hasFocus() itself to
// decide toast vs OS notification - force that the same way a real
// focused window would, rather than mocking exports announce does not
// call through this module's own boundary.
let hasFocusSpy: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  localStorage.clear();
  resetForTests();
  vi.mocked(toast.info).mockClear();
  hasFocusSpy = vi.spyOn(document, "hasFocus").mockReturnValue(true);
});
afterEach(() => {
  clearMocks();
  localStorage.clear();
  resetForTests();
  hasFocusSpy.mockRestore();
});

function Probe() {
  useMentions("acme", "Web");
  return null;
}

function mount() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe />
    </QueryClientProvider>,
  );
}

const stored = () =>
  JSON.parse(localStorage.getItem("tcm-v2-notifications:acme") ?? "[]") as Array<{ id: string; title: string; kind: string }>;

test("a work-item mention reaches the bell and a toast, asked for this org and project", async () => {
  const asked: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "recent_mentions") {
      asked.push(args);
      return [
        {
          source: "work-item", item_id: 41, item_type: "Product Backlog Item", item_title: "Leave requests",
          comment_id: 7, author: "Sam", excerpt: "@Avin can you check this?",
          created_date: new Date(Date.now() - 3_600_000).toISOString(),
        },
      ];
    }
  });
  mount();
  await waitFor(() => expect(stored().map((n) => n.id)).toEqual(["mention:wi:41:7"]));
  expect(asked).toEqual([{ organization: "acme", project: "Web" }]);
  expect(stored()[0]).toMatchObject({ kind: "mention", title: "Sam mentioned you on Product Backlog Item #41" });
  // announce()'s toast branch runs off a separate effect than the one that
  // wrote the bell entry above - wait for it rather than racing it.
  await waitFor(() =>
    expect(toast.info).toHaveBeenCalledWith("Sam mentioned you on Product Backlog Item #41", {
      description: "@Avin can you check this?",
      duration: 10_000,
    }),
  );
});

test("a failed check is logged, raises nothing and shows no toast", async () => {
  const logged: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "recent_mentions") throw { kind: "Network", detail: "Can't reach Azure DevOps." };
    if (cmd === "log_ui") logged.push((args as { message: string }).message);
  });
  mount();
  await waitFor(() => expect(logged.some((m) => m.startsWith("mentions: work-item check failed"))).toBe(true));
  expect(stored()).toEqual([]);
  expect(toast.info).not.toHaveBeenCalled();
  // A failure must not start the org's 24h first-run window either - only
  // a successful check may.
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBeNull();
});
