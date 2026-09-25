import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, test, vi } from "vitest";
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
  return {
    qc,
    ...render(
      <QueryClientProvider client={qc}>
        <PrThreads org="acme" project="Web" repo="repo" prId={1} enabled finalized={false} />
      </QueryClientProvider>,
    ),
  };
}

/// A PR comment's attachment image gets 401 as a plain markdown image (the
/// WebView sends no bearer header) - the panel asks Rust for it and swaps
/// the markdown text for a blob: URL before it reaches the Markdown island,
/// which refuses a data: src outright. jsdom enforces no CSP at all, so
/// this test cannot see the real app's img-src rule - that is pinned
/// separately in src/vendor-bundle.test.ts against tauri.conf.json.
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

/// Each render must not mint a fresh blob: URL (the query cache holds the
/// data: URI, not the blob - only the view converts, in an effect keyed on
/// the query data), and whatever it did mint must be released once this
/// view no longer needs it, or every open PR review leaks another
/// same-sized allocation for the life of the session.
test("unmounting revokes the blob: URL it minted", async () => {
  const revoke = vi.spyOn(URL, "revokeObjectURL");
  const { container, unmount } = renderThreads(
    threadWithComment(`Review Changes: ![image](${ATTACHMENT_URL})`),
    [{ url: ATTACHMENT_URL, data: "data:image/png;base64,iVBORw0KGgo=" }],
  );

  let blobUrl = "";
  await waitFor(() => {
    const img = container.querySelector("img[src^='blob:']");
    expect(img).not.toBeNull();
    blobUrl = img!.getAttribute("src")!;
  });

  unmount();
  expect(revoke).toHaveBeenCalledWith(blobUrl);
});

test("a later fetch replacing the cached images revokes the earlier blob: URL", async () => {
  const revoke = vi.spyOn(URL, "revokeObjectURL");
  const { container, qc } = renderThreads(
    threadWithComment(`Review Changes: ![image](${ATTACHMENT_URL})`),
    [{ url: ATTACHMENT_URL, data: "data:image/png;base64,iVBORw0KGgo=" }],
  );

  let firstBlobUrl = "";
  await waitFor(() => {
    const img = container.querySelector("img[src^='blob:']");
    expect(img).not.toBeNull();
    firstBlobUrl = img!.getAttribute("src")!;
  });

  // Simulate a refetch bringing back different bytes for the same URL -
  // the same query key PrThreads itself computes (org, then the sorted
  // attachment URLs found in the comment).
  qc.setQueryData(["comment-images", "acme", [ATTACHMENT_URL]], [
    { url: ATTACHMENT_URL, data: "data:image/png;base64,AAAAAAAA" },
  ]);

  await waitFor(() => expect(revoke).toHaveBeenCalledWith(firstBlobUrl));
});

/// Reopening a PR whose threads and images are both already cached mounts
/// with the images in hand. The app runs in <StrictMode>, which mounts,
/// cleans up and mounts again: a batch minted outside the effect that
/// revokes it would be revoked by that rehearsal and still be rendered -
/// every image broken in a dev build. What is on screen must be alive,
/// and released on unmount; a data change releases only the old batch.
test("under StrictMode, a cached PR's images render from a live blob: URL", async () => {
  const revoke = vi.spyOn(URL, "revokeObjectURL");
  // Earlier tests in this file spied the same function; count only ours.
  revoke.mockClear();
  const threads = threadWithComment(`Review Changes: ![image](${ATTACHMENT_URL})`);
  mockIPC((cmd) => {
    if (cmd === "pr_threads") return threads;
    if (cmd === "comment_images") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["pr-threads", "acme", "Web", "repo", 1], threads);
  qc.setQueryData(["comment-images", "acme", [ATTACHMENT_URL]], [
    { url: ATTACHMENT_URL, data: "data:image/png;base64,iVBORw0KGgo=" },
  ]);
  const { container, unmount } = render(
    <StrictMode>
      <QueryClientProvider client={qc}>
        <PrThreads org="acme" project="Web" repo="repo" prId={1} enabled finalized={false} />
      </QueryClientProvider>
    </StrictMode>,
  );

  let shown = "";
  await waitFor(() => {
    const img = container.querySelector("img[src^='blob:']");
    expect(img).not.toBeNull();
    shown = img!.getAttribute("src")!;
  });
  expect(revoke).not.toHaveBeenCalledWith(shown);

  qc.setQueryData(["comment-images", "acme", [ATTACHMENT_URL]], [
    { url: ATTACHMENT_URL, data: "data:image/png;base64,AAAAAAAA" },
  ]);
  let next = "";
  await waitFor(() => {
    next = container.querySelector("img[src^='blob:']")?.getAttribute("src") ?? "";
    expect(next).not.toBe("");
    expect(next).not.toBe(shown);
  });
  expect(revoke).toHaveBeenCalledWith(shown);
  expect(revoke).not.toHaveBeenCalledWith(next);

  unmount();
  expect(revoke).toHaveBeenCalledWith(next);
});
