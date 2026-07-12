import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import App from "./App";

afterEach(() => clearMocks());

function renderApp() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
}

test("shows sign-in button when signed out", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return null;
  });
  renderApp();
  expect(
    await screen.findByRole("button", { name: /sign in with microsoft/i }),
  ).toBeInTheDocument();
});

test("update banner appears when a newer version exists", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: false, account: null };
    if (cmd === "check_update") return "0.2.0";
  });
  renderApp();
  expect(await screen.findByText(/Version 0.2.0 is available/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Restart to update/ })).toBeInTheDocument();
});

test("remembers the last org and project across launches", async () => {
  localStorage.setItem(
    "tcm-v2-prefs",
    JSON.stringify({ org: "acme", project: "Web", mode: "tests" }),
  );
  mockIPC((cmd, args) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_projects")
      return (args as { organization: string }).organization === "acme"
        ? [{ id: "p1", name: "Web" }]
        : [];
  });
  renderApp();
  const { waitFor } = await import("@testing-library/react");
  const orgSelect = (await screen.findByRole("combobox", {
    name: /organization/i,
  })) as HTMLSelectElement;
  await waitFor(() => expect(orgSelect.value).toBe("acme"));
  localStorage.clear();
});

test("shows account and the browse screen when signed in", async () => {
  mockIPC((cmd) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "https://dev.azure.com/acme" }];
  });
  renderApp();
  expect(await screen.findByText("a@b.com")).toBeInTheDocument();
  expect(await screen.findByRole("option", { name: "acme" })).toBeInTheDocument();
});
