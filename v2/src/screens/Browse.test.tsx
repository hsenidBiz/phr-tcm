import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import Browse from "./Browse";

afterEach(() => clearMocks());

function Harness() {
  const [org, setOrg] = useState("");
  const [project, setProject] = useState("");
  return <Browse org={org} setOrg={setOrg} project={project} setProject={setProject} />;
}

function renderBrowse() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness />
    </QueryClientProvider>,
  );
}

function mockAll() {
  mockIPC((cmd, args) => {
    const a = args as Record<string, unknown>;
    switch (cmd) {
      case "list_orgs":
        return [{ name: "acme", url: "https://dev.azure.com/acme" }];
      case "list_projects":
        return a.organization === "acme" ? [{ id: "p1", name: "Web" }] : [];
      case "search_pbis":
        return a.project === "Web" && a.query === "login"
          ? [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }]
          : [];
      case "pbi_test_cases":
        return a.pbiId === 42
          ? [{ id: 201, title: "Valid login", tags: "smoke", automation_status: "Planned" }]
          : [];
    }
  });
}

test("full browse flow: org -> project -> search -> test cases", async () => {
  mockAll();
  renderBrowse();

  // Orgs auto-load into the select.
  await screen.findByRole("option", { name: "acme" });
  fireEvent.change(screen.getByRole("combobox", { name: /organization/i }), {
    target: { value: "acme" },
  });

  // Projects load for the chosen org.
  await screen.findByRole("option", { name: "Web" });
  fireEvent.change(screen.getByRole("combobox", { name: /project/i }), {
    target: { value: "Web" },
  });

  // Search on Enter.
  const search = screen.getByPlaceholderText("Title or ID");
  fireEvent.change(search, { target: { value: "login" } });
  fireEvent.keyDown(search, { key: "Enter" });
  const hit = await screen.findByText(/Login flow/);

  // Clicking the hit loads its linked test cases.
  fireEvent.click(hit);
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByRole("cell", { name: "Planned" })).toBeInTheDocument();
});

test("empty search result shows a friendly message", async () => {
  mockAll();
  renderBrowse();
  await screen.findByRole("option", { name: "acme" });
  fireEvent.change(screen.getByRole("combobox", { name: /organization/i }), {
    target: { value: "acme" },
  });
  await screen.findByRole("option", { name: "Web" });
  fireEvent.change(screen.getByRole("combobox", { name: /project/i }), {
    target: { value: "Web" },
  });
  const search = screen.getByPlaceholderText("Title or ID");
  fireEvent.change(search, { target: { value: "nothing" } });
  fireEvent.keyDown(search, { key: "Enter" });
  expect(await screen.findByText(/No PBIs match "nothing"/)).toBeInTheDocument();
});
