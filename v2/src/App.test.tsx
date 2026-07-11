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
  });
  renderApp();
  expect(
    await screen.findByRole("button", { name: /sign in with microsoft/i }),
  ).toBeInTheDocument();
});

test("shows account and project list when signed in", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "auth_status") return { signed_in: true, account: "a@b.com" };
    if (cmd === "list_projects" && (args as { organization: string }).organization === "myorg")
      return [{ id: "1", name: "Proj One" }];
  });
  const { container } = renderApp();
  expect(await screen.findByText("a@b.com")).toBeInTheDocument();

  const input = await screen.findByPlaceholderText("Organization name");
  const { fireEvent } = await import("@testing-library/react");
  fireEvent.change(input, { target: { value: "myorg" } });
  expect(await screen.findByText("Proj One")).toBeInTheDocument();
  expect(container.textContent).not.toContain("Rate limited");
});
