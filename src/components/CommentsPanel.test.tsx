import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Toaster } from "./ui/toaster";
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

const ATTACHMENT_URL =
  "https://dev.azure.com/acme/Web/_apis/wit/attachments/att-1?fileName=x.png";
const COMMENT_WITH_IMAGE = {
  id: 9,
  text: "Review Changes",
  text_html: `<div>Review Changes: <img src="${ATTACHMENT_URL}"></div>`,
  created_by: "Ada",
  created_by_id: "u-ada",
  created_date: STAMP,
  modified_date: STAMP,
  avatar_url: "",
};

function mockWithImage(imageResult: unknown[]) {
  mockIPC((cmd) => {
    switch (cmd) {
      case "plugin:event|listen":
        return 1;
      case "plugin:event|unlisten":
        return null;
      case "work_item_comments":
        return [COMMENT_WITH_IMAGE];
      case "connected_user":
        return { id: "u-ada", display_name: "Ada" };
      case "avatar_b64":
        return null;
      case "comment_images":
        return imageResult;
    }
  });
}

/// A comment's attachment image gets 401 as a plain <img> (the WebView
/// sends no bearer header) - the panel asks Rust for it and swaps in the
/// data: URI it downloaded, the same way the work item drawer does for a
/// description.
test("an attachment image in a comment is swapped for its downloaded data: URI", async () => {
  mockWithImage([{ url: ATTACHMENT_URL, data: "data:image/png;base64,iVBORw0KGgo=" }]);
  const { container } = renderPanel();

  await waitFor(() => {
    const img = container.querySelector("img[src^='data:image/png']");
    expect(img).not.toBeNull();
  });
  expect(container.querySelector(`img[src="${ATTACHMENT_URL}"]`)).toBeNull();
});

/// When the download comes back empty (fetch failed, or the host guard
/// refused it), the comment shows a small note instead of a permanently
/// broken image icon.
test("an attachment image that could not be fetched shows an unavailable note", async () => {
  mockWithImage([]);
  const { container } = renderPanel();

  await screen.findByText("Image unavailable");
  expect(container.querySelector(`img[src="${ATTACHMENT_URL}"]`)).toBeNull();
});

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
