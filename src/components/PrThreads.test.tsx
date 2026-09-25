import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import PrThreads from "./PrThreads";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const ATTACHMENT_URL =
  "https://dev.azure.com/acme/Web/_apis/git/repositories/repo/pullRequests/1/attachments/att-1?fileName=x.png";

function threadWithComment(content: string) {
  return [
    {
      id: 1,
      status: "",
      file_path: "src/App.tsx",
      line: 12,
      last_updated: "2026-07-12T01:00:00Z",
      comments: [
        {
          id: 1,
          author: "Ada",
          author_id: "u-ada",
          avatar: "",
          content,
          published: "2026-07-12T01:00:00Z",
          edited: false,
        },
      ],
    },
  ];
}

function renderThreads(threads: unknown[], imageResult: unknown[]) {
  mockIPC((cmd) => {
    switch (cmd) {
      case "pr_threads":
        return threads;
      case "comment_images":
        return imageResult;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <PrThreads org="acme" project="Web" repo="repo" prId={1} enabled finalized={false} />
    </QueryClientProvider>,
  );
}

/// A PR comment's attachment image gets 401 as a plain markdown image (the
/// WebView sends no bearer header) - the panel asks Rust for it and swaps
/// the markdown text for a blob: URL before it reaches the Markdown island,
/// which refuses a data: src outright.
test("an attachment image in a PR comment is swapped for its downloaded image", async () => {
  const { container } = renderThreads(
    threadWithComment(`Review Changes: ![image](${ATTACHMENT_URL})`),
    [{ url: ATTACHMENT_URL, data: "data:image/png;base64,iVBORw0KGgo=" }],
  );

  await waitFor(() => {
    const img = container.querySelector("img[src^='blob:']");
    expect(img).not.toBeNull();
  });
  expect(container.querySelector(`img[src="${ATTACHMENT_URL}"]`)).toBeNull();
});

/// When the download comes back empty (fetch failed, or the host guard
/// refused it), the thread shows a small note instead of a permanently
/// broken image icon.
test("a PR comment image that could not be fetched shows an unavailable note", async () => {
  const { container } = renderThreads(
    threadWithComment(`Review Changes: ![image](${ATTACHMENT_URL})`),
    [],
  );

  await screen.findByText("Image unavailable");
  expect(container.querySelector(`img[src="${ATTACHMENT_URL}"]`)).toBeNull();
});
