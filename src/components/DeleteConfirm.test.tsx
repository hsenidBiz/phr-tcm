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


/** Tick the acknowledgement, then fire the delete - the two-step every
 * real deletion now takes. */
function armAndDelete() {
  fireEvent.click(
    screen.getByRole("checkbox", {
      name: "I understand these test cases will be permanently deleted",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: /Permanently delete 2/ }));
}

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
  expect(screen.getByRole("button", { name: /Permanently delete 2/ })).toBeInTheDocument();
});

/** The wording owes the user the truth: Azure DevOps deletes test cases
 * permanently, with no bin behind it - the dialog must say "permanent"
 * outright and never dress the action up as recoverable. */
test("it says permanent, and never claims recoverability", () => {
  mount();
  expect(screen.getByText(/This is permanent/)).toBeInTheDocument();
  expect(screen.getByText(/run history cannot be/)).toBeInTheDocument();
  expect(screen.queryByText(/recoverable/i)).not.toBeInTheDocument();
});

/** No acknowledgement, no delete: the button stays dead until the
 * finality is explicitly accepted - the one-click slip is the failure
 * mode an irreversible action cannot afford. */
test("the delete button is dead until the permanence is acknowledged", () => {
  mount();
  expect(screen.getByRole("button", { name: /Permanently delete 2/ })).toBeDisabled();
  fireEvent.click(
    screen.getByRole("checkbox", {
      name: "I understand these test cases will be permanently deleted",
    }),
  );
  expect(screen.getByRole("button", { name: /Permanently delete 2/ })).toBeEnabled();
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
  armAndDelete();

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
  armAndDelete();

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
  armAndDelete();

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
  armAndDelete();

  expect(await screen.findByText(/2 could not be deleted/)).toBeInTheDocument();
  expect(screen.getByText(/Nothing was deleted/)).toBeInTheDocument();
  expect(screen.queryByText(/Permanently deleted \d/)).not.toBeInTheDocument();
  expect(onDeleted).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});
