import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import App from "./App";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderApp() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
}

test("shows sign-in view when signed out (no sidebar)", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return null;
  });
  renderApp();
  expect(
    await screen.findByRole("button", { name: /sign in with microsoft/i }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Work" })).not.toBeInTheDocument();
});

test("signed in: sidebar sections switch screens", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
  });
  renderApp();
  expect(await screen.findByText("a@b.com")).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Test Cases" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Work" }));
  expect(screen.getByRole("heading", { name: "Work" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(screen.getByText("Appearance")).toBeInTheDocument();
});

test("update banner appears when a newer version exists", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return "0.3.0";
  });
  renderApp();
  expect(await screen.findByText(/Version 0.3.0 is available/)).toBeInTheDocument();
});

test("remembers org/project/section across launches", async () => {
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "acme", project: "Web", section: "work" }),
  );
  mockIPC((cmd, args) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_projects")
      return (args as { organization: string }).organization === "acme"
        ? [{ id: "p1", name: "Web" }]
        : [];
    if (cmd === "fetch_board") return { items: [], states_by_type: {} };
  });
  renderApp();
  expect(await screen.findByRole("heading", { name: "Work" })).toBeInTheDocument();
  const orgSelect = (await screen.findByRole("combobox", {
    name: /organization/i,
  })) as HTMLSelectElement;
  await waitFor(() => expect(orgSelect.value).toBe("acme"));
});
