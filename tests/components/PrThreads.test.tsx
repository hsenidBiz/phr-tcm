import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { StrictMode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import PrThreads from "../../src/components/PrThreads";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(() => Promise.resolve()) }));

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
/// the markdown text for the downloaded data: URI, which the sanitiser
/// keeps on an <img>. jsdom enforces no CSP at all, so this test cannot see
/// the real app's img-src rule - that is pinned separately in
/// src/vendor-bundle.test.ts against tauri.conf.json.
test("an attachment image in a PR comment is swapped for its downloaded image", async () => {
  const { container } = renderThreads(
    threadWithComment(`Review Changes: ![image](${ATTACHMENT_URL})`),
    [{ url: ATTACHMENT_URL, data: "data:image/png;base64,iVBORw0KGgo=" }],
  );

  await waitFor(() => {
    expect(container.querySelector("img")?.getAttribute("src")).toBe(
      "data:image/png;base64,iVBORw0KGgo=",
    );
  });
  expect(container.querySelector(`img[src="${ATTACHMENT_URL}"]`)).toBeNull();
});

/// Under StrictMode, with the threads and the images already cached (a
/// reopened PR), the image is on screen from the first frame.
test("under StrictMode, a cached PR's images render from their data: URI", async () => {
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
  const { container } = render(
    <StrictMode>
      <QueryClientProvider client={qc}>
        <PrThreads org="acme" project="Web" repo="repo" prId={1} enabled finalized={false} />
      </QueryClientProvider>
    </StrictMode>,
  );

  await waitFor(() =>
    expect(container.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,iVBORw0KGgo="),
  );
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

/// While the download is still in flight the image must not flash as
/// unavailable.
test("a PR comment image still downloading does not show the unavailable note", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_threads") return threadWithComment(`![image](${ATTACHMENT_URL})`);
    if (cmd === "comment_images") return new Promise(() => {});
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <PrThreads org="acme" project="Web" repo="repo" prId={1} enabled finalized={false} />
    </QueryClientProvider>,
  );
  await screen.findByText("Ada");
  expect(screen.queryByText("Image unavailable")).toBeNull();
});

test("a link in a comment opens in the system browser and does not navigate", async () => {
  vi.mocked(openUrl).mockClear();
  const { container } = renderThreads(
    threadWithComment("See [the docs](https://example.com/docs) and [**bold**](https://example.com/b)"),
    [],
  );
  const link = await waitFor(() => {
    const a = container.querySelector("a[href='https://example.com/docs']");
    expect(a).not.toBeNull();
    return a!;
  });
  expect(fireEvent.click(link)).toBe(false); // default prevented
  expect(openUrl).toHaveBeenCalledWith("https://example.com/docs");

  // A click on markup nested inside the anchor still counts as the link.
  fireEvent.click(container.querySelector("a[href='https://example.com/b'] strong")!);
  expect(openUrl).toHaveBeenCalledWith("https://example.com/b");
});

test("a bare URL in a comment becomes a link", async () => {
  const { container } = renderThreads(
    threadWithComment("Plain https://example.com/a and www.example.org/b here"),
    [],
  );
  await waitFor(() => {
    expect(container.querySelector("a[href='https://example.com/a']")).not.toBeNull();
    expect(container.querySelector("a[href='http://www.example.org/b']")).not.toBeNull();
  });
});

test("script and event handlers in a comment are stripped", async () => {
  const { container } = renderThreads(
    threadWithComment('Hi <script>window.__x = 1</script><img src="x" onerror="window.__x = 2"> there'),
    [],
  );
  await screen.findByText(/there/);
  expect(container.querySelector("script")).toBeNull();
  expect(container.querySelector("[onerror]")).toBeNull();
  expect((window as unknown as { __x?: number }).__x).toBeUndefined();
});
