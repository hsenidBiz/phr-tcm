// The site address, edited on its own: it belongs to the ACTIVE environment
// (its start address and allowed sites), never to the sign-in recipe, so a
// save goes through env_save and leaves the recipe file alone. An empty
// address means "use the recipe's", said in words. A refused address shows
// the app's own sentence and the dialog stays open.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SiteAddressDialog, { siteHost } from "./SiteAddressDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const RECIPE = {
  start_url: "https://hr.example.internal/",
  steps: [{ kind: "fill", selector: { role: "textbox", name: "Username" }, value: "{{username}}" }],
  signed_in: { css: "#m" },
  allowed_origins: ["https://sso.example.internal"],
  session_minutes: 90,
};

const QA = {
  id: "qa",
  name: "QA",
  start_url: "https://qa.example.internal/",
  allowed_origins: ["https://sso.qa.example.internal"],
  db_id: "db-qa",
  has_default_password: true,
  test_environment: true,
};

function listOf(env: Partial<typeof QA> = {}) {
  return { active: "qa", environments: [{ ...QA, ...env }] };
}

function mount(
  recipe: unknown,
  onSave: (args: unknown) => unknown = () => listOf(),
  list: unknown = listOf(),
) {
  const commandsSeen: string[] = [];
  mockIPC((cmd, args) => {
    commandsSeen.push(cmd);
    if (cmd === "auto_run_load_recipe") return recipe;
    if (cmd === "env_list") return list;
    if (cmd === "env_save") return onSave(args);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SiteAddressDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { onClose, commandsSeen };
}

test("opens with the active environment's address and allowed sites", async () => {
  mount(RECIPE);
  expect(screen.getByRole("heading", { name: "Site address" })).toBeInTheDocument();
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://qa.example.internal/"));
  expect((screen.getByRole("textbox", { name: "Also allowed" }) as HTMLTextAreaElement).value).toBe(
    "https://sso.qa.example.internal",
  );
  expect(screen.queryByText("Using the sign-in recipe's address")).not.toBeInTheDocument();
  // The note states what really happens to a saved session: a new address
  // forgets this environment's saved sign-ins (env_save drops them).
  expect(screen.getByText(/changing the address forgets this environment's saved sign-ins/i)).toBeInTheDocument();
});

test("with no address, Also allowed is off and shows the recipe's allowed sites instead", async () => {
  mount(RECIPE, undefined, listOf({ start_url: "", allowed_origins: [] }));
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start).toBeEnabled());
  const also = screen.getByRole("textbox", { name: "Also allowed" }) as HTMLTextAreaElement;
  expect(also).toBeDisabled();
  await waitFor(() => expect(also.value).toBe("https://sso.example.internal"));
  expect(screen.getByText("Also allowed needs a start address - until then the sign-in recipe's are used.")).toBeInTheDocument();

  // Typing an address turns the box on, for this environment's own sites.
  fireEvent.change(start, { target: { value: "https://people.example.org/" } });
  expect(also).toBeEnabled();
  expect(also.value).toBe("");
});

test("saving with no address sends no allowed sites, even if the environment had some", async () => {
  const calls: { env: { start_url: string; allowed_origins: string[] } }[] = [];
  mount(
    RECIPE,
    (a) => {
      calls.push(a as never);
      return listOf({ start_url: "", allowed_origins: [] });
    },
    listOf({ start_url: "", allowed_origins: ["https://stale.example.org"] }),
  );
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0].env.start_url).toBe("");
  expect(calls[0].env.allowed_origins).toEqual([]);
});

test("an environment with no address says it uses the recipe's, and shows that address as the placeholder", async () => {
  mount(RECIPE, undefined, listOf({ start_url: "", allowed_origins: [] }));
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  expect(await screen.findByText("Using the sign-in recipe's address")).toBeInTheDocument();
  expect(start.value).toBe("");
  await waitFor(() => expect(start).toHaveAttribute("placeholder", "https://hr.example.internal/"));
});

test("an environment with no address and no recipe address says nothing about the recipe's", async () => {
  mount(null, undefined, listOf({ start_url: "", allowed_origins: [] }));
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start).toBeEnabled());
  expect(screen.queryByText("Using the sign-in recipe's address")).not.toBeInTheDocument();
});

test("save writes the address to the active environment and keeps its other fields", async () => {
  const calls: unknown[] = [];
  const { onClose, commandsSeen } = mount(RECIPE, (a) => {
    calls.push(a);
    return listOf();
  });
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://qa.example.internal/"));

  fireEvent.change(start, { target: { value: "  https://people.example.org/home  " } });
  fireEvent.change(screen.getByRole("textbox", { name: "Also allowed" }), {
    target: { value: "https://login.example.org\n\n   https://cdn.example.org:8443  \n" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual({
    env: {
      id: "qa",
      name: "QA",
      start_url: "https://people.example.org/home",
      allowed_origins: ["https://login.example.org", "https://cdn.example.org:8443"],
      db_id: "db-qa",
      test_environment: true,
    },
  });
  // The recipe file is never written from here.
  expect(commandsSeen).not.toContain("auto_run_save_recipe");
  await waitFor(() => expect(onClose).toHaveBeenCalled());
});

test("clearing the address saves it empty, so the environment falls back to the recipe's", async () => {
  const calls: { env: { start_url: string; allowed_origins: string[] } }[] = [];
  const { onClose } = mount(RECIPE, (a) => {
    calls.push(a as never);
    return listOf({ start_url: "", allowed_origins: [] });
  });
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://qa.example.internal/"));
  fireEvent.change(start, { target: { value: "" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Also allowed" }), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0].env.start_url).toBe("");
  expect(calls[0].env.allowed_origins).toEqual([]);
  await waitFor(() => expect(onClose).toHaveBeenCalled());
});

test("an address the app refuses shows its sentence and the dialog stays open", async () => {
  const { onClose } = mount(RECIPE, () => {
    throw new Error("the start address must be a full http or https address");
  });
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://qa.example.internal/"));
  fireEvent.change(start, { target: { value: "qa.example.internal" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  expect(
    await screen.findByText("the start address must be a full http or https address"),
  ).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
  expect(start.value).toBe("qa.example.internal");
});

test("with no sign-in recipe the environment's address can still be set", async () => {
  mount(null);
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://qa.example.internal/"));
  expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
});

test("with no saved recipe the hints speak of the built-in sign-in, never the recipe's address", async () => {
  mount(null, undefined, listOf({ start_url: "", allowed_origins: [] }));
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start).toBeEnabled());
  expect(await screen.findByText("The built-in sign-in uses this address.")).toBeInTheDocument();
  expect(screen.getByText("Also allowed needs a start address first.")).toBeInTheDocument();
  expect(screen.queryByText(/sign-in recipe's/)).not.toBeInTheDocument();
});

test("with no environment to edit, Save stays off", async () => {
  mount(RECIPE, undefined, { active: "", environments: [] });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Start address" })).toBeDisabled());
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
});

test("Cancel closes without saving", async () => {
  const calls: unknown[] = [];
  const { onClose } = mount(RECIPE, (a) => {
    calls.push(a);
    return listOf();
  });
  await screen.findByRole("textbox", { name: "Start address" });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onClose).toHaveBeenCalled();
  expect(calls).toEqual([]);
});

test("siteHost names the host, and leaves text that is not an address alone", () => {
  expect(siteHost("https://hr.example.internal/login?x=1")).toBe("hr.example.internal");
  expect(siteHost("https://hr.example.internal:8443/")).toBe("hr.example.internal:8443");
  expect(siteHost("not an address")).toBe("not an address");
});
