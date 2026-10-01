// The project's Test files, managed in one dialog: the list with sizes,
// Add files (a same name is asked about first), Remove with a confirm, Open
// folder - and every refusal shown inline, in Rust's own words.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import TestFilesDialog, { fileSize } from "./TestFilesDialog";

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

type File = { name: string; size: number; modified: string };
type Call = { cmd: string; args: Record<string, unknown> };

/** A folder that answers like Rust's: `files` is the folder; add and remove
 * change it; `picked` is what the file dialog hands back; `refuse` makes a
 * command fail with that sentence. */
function mount(opts: {
  files?: File[];
  picked?: string[] | null;
  refuse?: Partial<Record<string, string>>;
}) {
  const folder: File[] = [...(opts.files ?? [])];
  const calls: Call[] = [];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    const refused = opts.refuse?.[cmd];
    if (refused) throw refused;
    if (cmd === "test_files_list") return folder.map((f) => ({ ...f }));
    if (cmd === "plugin:dialog|open") return opts.picked ?? null;
    if (cmd === "test_files_add") {
      const name = String(args.path).split(/[\\/]/).pop()!;
      const i = folder.findIndex((f) => f.name.toLowerCase() === name.toLowerCase());
      if (i >= 0 && !args.replace) throw `"${name}" is already in Test files - replace it, or rename your copy first`;
      const added = { name, size: 2048, modified: "1" };
      if (i >= 0) folder[i] = added;
      else folder.push(added);
      return added;
    }
    if (cmd === "test_files_remove") {
      const i = folder.findIndex((f) => f.name === args.name);
      folder.splice(i, 1);
      return null;
    }
    if (cmd === "test_files_open_folder") return null;
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TestFilesDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose, folder };
}

const listed = () =>
  within(screen.getByRole("list", { name: "Test files" }))
    .getAllByRole("listitem")
    .map((li) => li.textContent);

test("lists each file with its size, and says where they live", async () => {
  mount({ files: [{ name: "appraisal.pdf", size: 1536, modified: "1" }, { name: "cv.txt", size: 5, modified: "1" }] });
  expect(screen.getByRole("heading", { name: "Test files" })).toBeInTheDocument();
  expect(screen.getByText("Scripts and API templates upload these by name. They stay on this machine.")).toBeInTheDocument();
  await screen.findByText("appraisal.pdf");
  expect(listed()).toEqual([expect.stringContaining("1.5 KB"), expect.stringContaining("5 bytes")]);
  expect(screen.getByText("cv.txt")).toBeInTheDocument();
});

test("an empty folder says so", async () => {
  mount({ files: [] });
  expect(await screen.findByText(/No test files yet/)).toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Test files" })).not.toBeInTheDocument();
});

test("Add files copies each picked file in, by its path", async () => {
  const { calls } = mount({ files: [], picked: ["C:\\Docs\\appraisal.pdf", "C:\\Docs\\cv.txt"] });
  await screen.findByText(/No test files yet/);
  fireEvent.click(screen.getByRole("button", { name: "Add files" }));
  await screen.findByText("cv.txt");
  const adds = calls.filter((c) => c.cmd === "test_files_add").map((c) => c.args);
  expect(adds).toEqual([
    expect.objectContaining({ organization: "acme", project: "Web", path: "C:\\Docs\\appraisal.pdf", replace: false }),
    expect.objectContaining({ path: "C:\\Docs\\cv.txt", replace: false }),
  ]);
  const dialog = calls.find((c) => c.cmd === "plugin:dialog|open")?.args as { options: { multiple: boolean } };
  expect(dialog.options.multiple).toBe(true);
  expect(screen.getByText("appraisal.pdf")).toBeInTheDocument();
});

test("a same name is asked about first: Replace sends replace, Keep sends nothing", async () => {
  const { calls } = mount({
    files: [
      { name: "cv.txt", size: 5, modified: "1" },
      { name: "appraisal.pdf", size: 9, modified: "1" },
    ],
    picked: ["C:\\New\\CV.txt", "C:\\New\\appraisal.pdf", "C:\\New\\fresh.png"],
  });
  await screen.findByText("cv.txt");
  fireEvent.click(screen.getByRole("button", { name: "Add files" }));

  // The new one goes straight in; the two clashes wait, one at a time.
  const first = await screen.findByRole("group", { name: "Replace CV.txt?" });
  expect(calls.filter((c) => c.cmd === "test_files_add").map((c) => c.args.path)).toEqual(["C:\\New\\fresh.png"]);
  expect(screen.getByRole("button", { name: "Add files" })).toBeDisabled();

  fireEvent.click(within(first).getByRole("button", { name: "Replace" }));
  const second = await screen.findByRole("group", { name: "Replace appraisal.pdf?" });
  const adds = calls.filter((c) => c.cmd === "test_files_add");
  expect(adds[adds.length - 1]?.args).toEqual(
    expect.objectContaining({ path: "C:\\New\\CV.txt", replace: true }),
  );

  fireEvent.click(within(second).getByRole("button", { name: "Keep the old one" }));
  await waitFor(() => expect(screen.queryByRole("group", { name: /^Replace/ })).not.toBeInTheDocument());
  expect(calls.filter((c) => c.cmd === "test_files_add").map((c) => [c.args.path, c.args.replace])).toEqual([
    ["C:\\New\\fresh.png", false],
    ["C:\\New\\CV.txt", true],
  ]);
});

test("a clash Rust reports, missed by a stale list, is asked about too", async () => {
  // The list says nothing is there; the folder does have it.
  const { calls, folder } = mount({ files: [], picked: ["C:\\New\\cv.txt"] });
  await screen.findByText(/No test files yet/);
  folder.push({ name: "cv.txt", size: 5, modified: "1" });
  fireEvent.click(screen.getByRole("button", { name: "Add files" }));
  const ask = await screen.findByRole("group", { name: "Replace cv.txt?" });
  fireEvent.click(within(ask).getByRole("button", { name: "Replace" }));
  await waitFor(() =>
    expect(calls.filter((c) => c.cmd === "test_files_add").map((c) => c.args.replace)).toEqual([false, true]),
  );
});

test("Remove asks first; Keep leaves the file, Remove removes it", async () => {
  const { calls } = mount({ files: [{ name: "cv.txt", size: 5, modified: "1" }] });
  await screen.findByText("cv.txt");

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  const confirm = screen.getByRole("group", { name: "Remove cv.txt?" });
  fireEvent.click(within(confirm).getByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("group", { name: "Remove cv.txt?" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "test_files_remove")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  fireEvent.click(within(screen.getByRole("group", { name: "Remove cv.txt?" })).getByRole("button", { name: "Remove" }));
  expect(await screen.findByText(/No test files yet/)).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "test_files_remove")?.args).toEqual(
    expect.objectContaining({ organization: "acme", project: "Web", name: "cv.txt" }),
  );
});

test("Open folder asks Rust to open the project's folder", async () => {
  const { calls } = mount({ files: [] });
  await screen.findByText(/No test files yet/);
  fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "test_files_open_folder")?.args).toEqual(
      expect.objectContaining({ organization: "acme", project: "Web" }),
    ),
  );
});

test("refusals are shown inline, in Rust's words", async () => {
  mount({
    files: [{ name: "cv.txt", size: 5, modified: "1" }],
    picked: ["C:\\Docs\\huge.zip"],
    refuse: {
      test_files_add: '"huge.zip" is larger than 25 MB - a test file can be at most 25 MB',
      test_files_open_folder: "Could not open the Test files folder.",
      test_files_remove: '"cv.txt" could not be removed - see Settings, Logs',
    },
  });
  await screen.findByText("cv.txt");

  fireEvent.click(screen.getByRole("button", { name: "Add files" }));
  expect(await screen.findByText('"huge.zip" is larger than 25 MB - a test file can be at most 25 MB')).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
  expect(await screen.findByText("Could not open the Test files folder.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  fireEvent.click(within(screen.getByRole("group", { name: "Remove cv.txt?" })).getByRole("button", { name: "Remove" }));
  expect(await screen.findByText('"cv.txt" could not be removed - see Settings, Logs')).toBeInTheDocument();
});

test("a list that cannot be read says why", async () => {
  mount({ refuse: { test_files_list: "the Test files folder could not be read - see Settings, Logs" } });
  expect(await screen.findByText("the Test files folder could not be read - see Settings, Logs")).toBeInTheDocument();
});

test("Close closes", async () => {
  const { onClose } = mount({ files: [] });
  await screen.findByText(/No test files yet/);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalled();
});

test("sizes read the way the run reports say them", () => {
  expect(fileSize(1)).toBe("1 byte");
  expect(fileSize(512)).toBe("512 bytes");
  expect(fileSize(1536)).toBe("1.5 KB");
  expect(fileSize(25 * 1024 * 1024)).toBe("25.0 MB");
});
