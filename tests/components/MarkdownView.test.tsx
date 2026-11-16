import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { afterEach, expect, test, vi } from "vitest";
import MarkdownView from "../../src/components/MarkdownView";

const h = vi.hoisted(() => ({ copyText: vi.fn(() => Promise.resolve()) }));
vi.mock("../../src/lib/clipboard", () => ({ copyText: h.copyText }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(() => Promise.resolve()) }));

afterEach(() => vi.clearAllMocks());

test("a fenced block that names a language gets its label and a Copy button", async () => {
  const { container } = render(<MarkdownView text={"```ts\nconst a = 1;\n```"} />);
  const copy = await screen.findByRole("button", { name: "Copy code" });
  expect(container.querySelector(".md-codebar-label")?.textContent).toBe("ts");
  fireEvent.click(copy);
  expect(h.copyText).toHaveBeenCalledWith("const a = 1;");
  expect(openUrl).not.toHaveBeenCalled();
});

test("a fenced block with no language has no bar", () => {
  const { container } = render(<MarkdownView text={"```\nplain\n```"} />);
  expect(container.querySelector(".md-codebar")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
});

test("buttons and handlers written in the markdown are still stripped", () => {
  const { container } = render(
    <MarkdownView text={'<button onclick="window.__b = 1">x</button> ```ts\nfoo\n```'} />,
  );
  expect(container.querySelector("button[onclick]")).toBeNull();
  expect(container.querySelectorAll("button")).toHaveLength(0);
});

test("single newlines run together", () => {
  const { container } = render(<MarkdownView text={"line one\nline two"} />);
  expect(container.querySelector("br")).toBeNull();
});

test("an ordered list that starts above 1 counts from there", () => {
  const { container } = render(<MarkdownView text={"3. three\n4. four"} />);
  expect((container.querySelector("ol") as HTMLElement).style.counterReset).toContain("2");
});

test("copy failure toasts instead of throwing", async () => {
  h.copyText.mockRejectedValueOnce(new Error("no"));
  render(<MarkdownView text={"```js\nx\n```"} />);
  fireEvent.click(await screen.findByRole("button", { name: "Copy code" }));
  await waitFor(() => expect(h.copyText).toHaveBeenCalled());
});

test("only http(s), mailto and tel links reach the system browser", () => {
  const { container } = render(
    <MarkdownView
      text={"[web](https://e.example) [mail](mailto:a@e.example) [tel](tel:+1) [rel](/docs/x) [hash](#top) [rel2](other.md)"}
    />,
  );
  const link = (name: string) => screen.getByRole("link", { name });
  for (const name of ["web", "mail", "tel"]) expect(fireEvent.click(link(name))).toBe(false);
  expect(openUrl).toHaveBeenCalledTimes(3);
  vi.mocked(openUrl).mockClear();
  for (const name of ["rel", "hash", "rel2"]) expect(fireEvent.click(link(name))).toBe(false);
  expect(openUrl).not.toHaveBeenCalled();
  expect(container.querySelector("a")).not.toBeNull();
});

test("a table scrolls inside its own wrapper", () => {
  const { container } = render(<MarkdownView text={"| a | b |\n|---|---|\n| 1 | 2 |"} />);
  expect(container.querySelector(".md-table-scroll > table")).not.toBeNull();
});
