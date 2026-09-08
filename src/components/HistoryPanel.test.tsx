import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import HistoryPanel from "./HistoryPanel";

afterEach(() => clearMocks());

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <HistoryPanel org="acme" project="Web" itemId={42} />
    </QueryClientProvider>,
  );
}

const revision = (over: Partial<Record<string, unknown>>) => ({
  rev: 2,
  by: "Avin Alwis",
  avatar_url: "",
  at: "2026-08-04T08:00:00Z",
  fields: [],
  links_added: [],
  links_removed: [],
  state_from: "New",
  state_to: "In Progress",
  comment_added: false,
  ...over,
});

/// ADO's avatar URLs demand the bearer token, which a bare <img src>
/// cannot carry - so every shown avatar must have come through the
/// authenticated avatar_b64 fetch as a data URI, the same path the
/// comments list takes. This pins the History tab to that path.
test("avatars go through the authenticated fetch, never the raw URL", async () => {
  const RAW = "https://dev.azure.com/acme/_apis/GraphProfile/MemberAvatars/x";
  const fetched: string[] = [];
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "work_item_history":
        return [revision({ avatar_url: RAW })];
      case "avatar_b64": {
        fetched.push((args as { url: string }).url);
        return "QkFTRTY0"; // any base64 payload
      }
    }
  });
  renderPanel();

  const img = await screen.findByRole("presentation");
  expect(img).toHaveAttribute("src", "data:image/png;base64,QkFTRTY0");
  expect(fetched).toEqual([RAW]);
});

test("a revision without an avatar shows the initials disc instead", async () => {
  mockIPC((cmd) => {
    if (cmd === "work_item_history") return [revision({ avatar_url: "" })];
  });
  renderPanel();

  expect(await screen.findByText("AA")).toBeInTheDocument();
  expect(screen.queryByRole("presentation")).not.toBeInTheDocument();
});
