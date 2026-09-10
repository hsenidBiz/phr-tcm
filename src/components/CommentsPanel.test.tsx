import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Toaster } from "sonner";
import { afterEach, expect, test } from "vitest";
import CommentsPanel from "./CommentsPanel";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const STAMP = "2026-07-12T01:00:00Z";
const COMMENTS = [
  {
    id: 7,
    text: "Looks good",
    text_html: "<div>Looks <b>good</b></div>",
    created_by: "Ada",
    created_by_id: "u-ada",
    created_date: STAMP,
    modified_date: "2026-07-12T02:00:00Z", // edited later
    avatar_url: "",
  },
  {
    id: 8,
    text: "plain",
    text_html: "<div>plain</div>",
    created_by: "Bob",
    created_by_id: "u-bob",
    created_date: STAMP,
    modified_date: STAMP, // never edited
    avatar_url: "",
  },
];

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <CommentsPanel org="acme" project="Web" itemId={42} />
      <Toaster />
    </QueryClientProvider>,
  );
}

function mockAll(capture: { updated?: Record<string, unknown>[]; added?: Record<string, unknown>[] }) {
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "work_item_comments":
        return COMMENTS;
      case "connected_user":
        return { id: "u-ada", display_name: "Ada" };
      case "avatar_b64":
        return null;
      case "update_comment":
        capture.updated?.push(args as Record<string, unknown>);
        return null;
      case "add_comment":
        capture.added?.push(args as Record<string, unknown>);
        return null;
    }
  });
}

/// Comments render as the rich text ADO stores (not flattened), say when
/// they were edited, and offer Edit only on the signed-in user's own -
/// the way ADO's own form does.
test("renders rich text, marks edits, and offers Edit only on your own comments", async () => {
  mockAll({});
  renderPanel();

  const bold = await screen.findByText("good");
  expect(bold.tagName).toBe("STRONG");
  expect(screen.getAllByText(/edited/)).toHaveLength(1);
  await waitFor(() => expect(screen.getAllByRole("button", { name: "Edit" })).toHaveLength(1));
});

/// Edit opens the same markdown editor the description uses, prefilled
/// with the comment as markdown; Update sends the new HTML to the one
/// comment being edited, in ADO's own Cancel / Update pairing.
test("Edit then Update rewrites that comment as HTML", async () => {
  const updated: Record<string, unknown>[] = [];
  mockAll({ updated });
  renderPanel();

  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  const editor = screen.getByLabelText("Comment (markdown)");
  expect(editor).toHaveValue("Looks **good**");

  // Unchanged text keeps Update off - nothing to send.
  expect(screen.getByRole("button", { name: "Update" })).toBeDisabled();
  fireEvent.change(editor, { target: { value: "Looks **great**" } });
  fireEvent.click(screen.getByRole("button", { name: "Update" }));

  await waitFor(() => expect(updated).toHaveLength(1));
  expect(updated[0].id).toBe(42);
  expect(updated[0].commentId).toBe(7);
  expect(String(updated[0].text)).toContain("<strong>great</strong>");
  expect(await screen.findByText("Comment updated.")).toBeInTheDocument();
  // The editor closes; the row is back to its rendered form.
  expect(screen.queryByLabelText("Comment (markdown)")).not.toBeInTheDocument();
});

/// The composer speaks markdown too: a list typed here reaches ADO as the
/// HTML it renders, not as literal dashes.
test("a new comment is posted as HTML rendered from markdown", async () => {
  const added: Record<string, unknown>[] = [];
  mockAll({ added });
  renderPanel();

  const box = await screen.findByLabelText("New comment (markdown)");
  fireEvent.change(box, { target: { value: "- item" } });
  fireEvent.click(screen.getByRole("button", { name: /Post/ }));

  await waitFor(() => expect(added).toHaveLength(1));
  expect(String(added[0].text)).toContain("<li>item</li>");
  expect(await screen.findByText("Comment added.")).toBeInTheDocument();
});
