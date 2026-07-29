import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import DeleteConfirm from "./DeleteConfirm";

afterEach(() => vi.restoreAllMocks());

const CASES = [
  { id: 5001, title: "Login - valid credentials" },
  { id: 5002, title: "Login - locked account is refused" },
];

function mount(onDeleted = vi.fn(), onClose = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <DeleteConfirm
        org="acme"
        project="Web"
        cases={CASES}
        onClose={onClose}
        onDeleted={onDeleted}
      />
    </QueryClientProvider>,
  );
  return { onDeleted, onClose };
}

/** A count is not checkable. The point of a confirmation is that you can
 *  look at it and see whether it is what you meant. */
test("every case is listed by id and title before anything happens", () => {
  mount();
  expect(screen.getByText("#5001")).toBeInTheDocument();
  expect(screen.getByText("Login - valid credentials")).toBeInTheDocument();
  expect(screen.getByText("#5002")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Delete 2/ })).toBeInTheDocument();
});

/** The app has no permanent delete, and the wording has to say so - a user
 *  deciding whether to press this needs to know it is recoverable. */
test("it says where they go and who can bring them back", () => {
  mount();
  expect(screen.getByText(/recycle bin/i)).toBeInTheDocument();
  expect(screen.getByText(/never deletes anything permanently/i)).toBeInTheDocument();
});

test("cancel sends nothing", () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    return null;
  });
  const { onClose, onDeleted } = mount();
  fireEvent.click(screen.getByRole("button", { name: /Cancel/ }));
  expect(onClose).toHaveBeenCalled();
  expect(onDeleted).not.toHaveBeenCalled();
  expect(calls).not.toContain("delete_test_cases");
});

test("delete sends exactly the listed ids", async () => {
  let sent: unknown = null;
  mockIPC((cmd, args) => {
    if (cmd === "delete_test_cases") {
      sent = args;
      return CASES.map((c) => ({ id: c.id, deleted: true, error: "" }));
    }
    return null;
  });
  const { onDeleted, onClose } = mount();
  fireEvent.click(screen.getByRole("button", { name: /Delete 2/ }));

  await waitFor(() => expect(onDeleted).toHaveBeenCalled());
  expect((sent as { ids: number[] }).ids).toEqual([5001, 5002]);
  expect(onClose).toHaveBeenCalled();
});

/** A partial failure must stay on screen naming what survived. Closing here
 *  would leave the user to work it out from a refreshed list. */
test("a partial failure names what was left behind and stays open", async () => {
  mockIPC((cmd) => {
    if (cmd === "delete_test_cases") {
      return [
        { id: 5001, deleted: true, error: "" },
        { id: 5002, deleted: false, error: "You do not have permission." },
      ];
    }
    return null;
  });
  const { onClose } = mount();
  fireEvent.click(screen.getByRole("button", { name: /Delete 2/ }));

  expect(await screen.findByText(/1 could not be deleted/)).toBeInTheDocument();
  expect(screen.getByText("You do not have permission.")).toBeInTheDocument();
  expect(screen.getByText("#5002")).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

/** When EVERY delete fails there is no "rest" that was recycled, and the
 *  selection must survive - those cases all still exist, and the user needs
 *  them selected to try again. */
test("an all-failed delete claims nothing and keeps the selection", async () => {
  mockIPC((cmd) => {
    if (cmd === "delete_test_cases") {
      return CASES.map((c) => ({ id: c.id, deleted: false, error: "You do not have permission." }));
    }
    return null;
  });
  const { onDeleted, onClose } = mount();
  fireEvent.click(screen.getByRole("button", { name: /Delete 2/ }));

  expect(await screen.findByText(/2 could not be deleted/)).toBeInTheDocument();
  expect(screen.getByText(/Nothing was deleted/)).toBeInTheDocument();
  expect(screen.queryByText(/moved to the recycle bin/)).not.toBeInTheDocument();
  expect(onDeleted).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});
