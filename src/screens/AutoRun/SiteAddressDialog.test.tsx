// The site address, edited on its own: it is the recipe's start_url and
// allowed_origins, so a save must write those two and hand every other
// recipe field back exactly as it was read - and a refused address must
// show the app's own sentence and write nothing.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SiteAddressDialog, { siteHost } from "./SiteAddressDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const RECIPE = {
  start_url: "https://hr.example.internal/",
  steps: [
    { kind: "fill", selector: { role: "textbox", name: "Username" }, value: "{{username}}" },
    {
      kind: "when_visible",
      selector: { role: "button", name: "Continue here" },
      within_ms: 5000,
      then: [{ kind: "click", selector: { role: "button", name: "Continue here" } }],
    },
  ],
  after_sign_in: [{ kind: "click", selector: { css: "#menu" } }],
  signed_in: { css: "#m" },
  allowed_origins: ["https://sso.example.internal"],
  session_minutes: 90,
};

function mount(recipe: unknown, onSave: (args: unknown) => unknown = () => null) {
  const loads: number[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_recipe") {
      loads.push(1);
      return recipe;
    }
    if (cmd === "auto_run_save_recipe") return onSave(args);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SiteAddressDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { onClose, loads };
}

test("opens with the saved start address and allowed sites", async () => {
  mount(RECIPE);
  expect(screen.getByRole("heading", { name: "Site address" })).toBeInTheDocument();
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://hr.example.internal/"));
  expect((screen.getByRole("textbox", { name: "Also allowed" }) as HTMLTextAreaElement).value).toBe(
    "https://sso.example.internal",
  );
  // The note states what really happens to a saved session: sessions are
  // kept per account, and one the new site does not accept is replaced.
  expect(screen.getByText(/a saved sign-in is tried first/i)).toBeInTheDocument();
});

test("save writes start_url and allowed_origins and keeps every other recipe field", async () => {
  const calls: unknown[] = [];
  const { onClose, loads } = mount(RECIPE, (a) => {
    calls.push(a);
    return null;
  });
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://hr.example.internal/"));
  const before = loads.length;

  fireEvent.change(start, { target: { value: "  https://people.example.org/home  " } });
  fireEvent.change(screen.getByRole("textbox", { name: "Also allowed" }), {
    target: { value: "https://login.example.org\n\n   https://cdn.example.org:8443  \n" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual({
    organization: "acme",
    project: "Web",
    recipe: {
      ...RECIPE,
      start_url: "https://people.example.org/home",
      allowed_origins: ["https://login.example.org", "https://cdn.example.org:8443"],
    },
  });
  // The recipe was read back fresh at save time, not only when opened.
  expect(loads.length).toBeGreaterThan(before);
  await waitFor(() => expect(onClose).toHaveBeenCalled());
});

test("clearing the allowed sites saves an empty list", async () => {
  const calls: { recipe: { allowed_origins: string[] } }[] = [];
  mount(RECIPE, (a) => {
    calls.push(a as never);
    return null;
  });
  const also = (await screen.findByRole("textbox", { name: "Also allowed" })) as HTMLTextAreaElement;
  await waitFor(() => expect(also.value).toBe("https://sso.example.internal"));
  fireEvent.change(also, { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0].recipe.allowed_origins).toEqual([]);
});

test("an address the app refuses shows its sentence and the dialog stays open", async () => {
  const { onClose } = mount(RECIPE, () => {
    throw new Error("the start address must be a full http or https address");
  });
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://hr.example.internal/"));
  fireEvent.change(start, { target: { value: "hr.example.internal" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  expect(
    await screen.findByText("the start address must be a full http or https address"),
  ).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
  expect(start.value).toBe("hr.example.internal");
});

test("an empty start address cannot be saved", async () => {
  mount(RECIPE);
  const start = (await screen.findByRole("textbox", { name: "Start address" })) as HTMLInputElement;
  await waitFor(() => expect(start.value).toBe("https://hr.example.internal/"));
  fireEvent.change(start, { target: { value: "   " } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
});

test("with no recipe saved there is nothing to edit, and Save stays off", async () => {
  const calls: unknown[] = [];
  mount(null, (a) => {
    calls.push(a);
    return null;
  });
  expect(await screen.findByText(/no sign-in recipe yet/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  expect(calls).toEqual([]);
});

test("Cancel closes without saving", async () => {
  const calls: unknown[] = [];
  const { onClose } = mount(RECIPE, (a) => {
    calls.push(a);
    return null;
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
