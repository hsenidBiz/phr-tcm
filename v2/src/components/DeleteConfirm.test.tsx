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

/** The app never REQUESTS a permanent delete, and the wording has to say
 *  so. What it must NOT do is promise recovery: that is Azure DevOps' to
 *  give, its documentation is not consistent about test cases, and this
 *  app cannot check the outcome. Pinning the old absolute wording here is
 *  what would keep an unverifiable guarantee alive. */
test("it promises only what the app itself controls", () => {
  mount();
  expect(screen.getByText(/never requests a permanent/i)).toBeInTheDocument();
  expect(screen.getByText(/one-way from here/i)).toBeInTheDocument();
  expect(screen.queryByText(/never deletes anything permanently/i)).not.toBeInTheDocument();
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
      return CASES.map((c) => ({ id: c.id, deleted: true, error: null }));
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
        { id: 5001, deleted: true, error: null },
        { id: 5002, deleted: false, error: { kind: "Forbidden" } },
      ];
    }
    return null;
  });
  const { onClose } = mount();
  fireEvent.click(screen.getByRole("button", { name: /Delete 2/ }));

  expect(await screen.findByText(/1 could not be deleted/)).toBeInTheDocument();
  expect(screen.getByText(/don't have permission/i)).toBeInTheDocument();
  expect(screen.getByText("#5002")).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

/** The regression this panel was built wrong for. Azure DevOps refuses some
 *  deletes with a 400 and a sentence saying why; the outcome used to be
 *  flattened to a string in Rust, whose Display for that variant is the
 *  bare text "http 400". The user could report the number and nothing else.
 *  The explanation has to reach this list. */
test("Azure DevOps' own explanation is what the failure list shows", async () => {
  mockIPC((cmd) => {
    if (cmd === "delete_test_cases") {
      return [
        {
          id: 5001,
          deleted: false,
          error: {
            kind: "Http",
            detail: {
              status: 400,
              body: JSON.stringify({
                message: "VS402625: Work item 5001 cannot be deleted because it is in use.",
              }),
            },
          },
        },
        { id: 5002, deleted: true, error: null },
      ];
    }
    return null;
  });
  mount();
  fireEvent.click(screen.getByRole("button", { name: /Delete 2/ }));

  expect(await screen.findByText(/cannot be deleted because it is in use/)).toBeInTheDocument();
  expect(screen.queryByText(/http 400/i)).not.toBeInTheDocument();
});

/** When EVERY delete fails there is no "rest" that was recycled, and the
 *  selection must survive - those cases all still exist, and the user needs
 *  them selected to try again. */
test("an all-failed delete claims nothing and keeps the selection", async () => {
  mockIPC((cmd) => {
    if (cmd === "delete_test_cases") {
      return CASES.map((c) => ({ id: c.id, deleted: false, error: { kind: "Forbidden" } }));
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
