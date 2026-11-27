// The project's Test files, managed in one dialog: the list with sizes,
// Add files (a same name is asked about first), Remove with a confirm, Open
// folder - and every refusal shown inline, in Rust's own words.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { CaseScript } from "../../../src/bindings";
import TestFilesDialog, { fileSize } from "../../../src/screens/AutoRun/TestFilesDialog";

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
  scripts?: (CaseScript | null | undefined)[];
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
      <TestFilesDialog org="acme" project="Web" scripts={opts.scripts} onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose, folder };
}

const listed = () =>
  within(screen.getByRole("group", { name: "Test files" }))
    .getAllByRole("listitem")
    .map((li) => li.textContent);

test("lists each file with its size, and says where they live", async () => {
  mount({
    files: [
      { name: "appraisal.pdf", size: 1536, modified: "1" },
      { name: "cv.txt", size: 5, modified: "1" },
    ],
  });
  expect(screen.getByRole("heading", { name: "Test files" })).toBeInTheDocument();
  expect(
    screen.getByText("Scripts and API templates upload these by name. They stay on this machine."),
  ).toBeInTheDocument();
  await screen.findByText("appraisal.pdf");
  expect(listed()).toEqual([expect.stringContaining("1.5 KB"), expect.stringContaining("5 bytes")]);
  expect(screen.getByText("cv.txt")).toBeInTheDocument();
});

test("an empty folder says so", async () => {
  mount({ files: [] });
  expect(await screen.findByText(/No test files yet/)).toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Test files" })).not.toBeInTheDocument();
});

test("Add files copies each picked file in, by its path", async () => {
  const { calls } = mount({
    files: [],
    picked: ["C:\\Docs\\appraisal.pdf", "C:\\Docs\\cv.txt"],
  });
  await screen.findByText(/No test files yet/);
  fireEvent.click(screen.getByRole("button", { name: "Add files" }));
  await screen.findByText("cv.txt");
  const adds = calls.filter((c) => c.cmd === "test_files_add").map((c) => c.args);
  expect(adds).toEqual([
    expect.objectContaining({
      organization: "acme",
      project: "Web",
      path: "C:\\Docs\\appraisal.pdf",
      replace: false,
    }),
    expect.objectContaining({ path: "C:\\Docs\\cv.txt", replace: false }),
  ]);
  const dialog = calls.find((c) => c.cmd === "plugin:dialog|open")?.args as {
    options: { multiple: boolean };
  };
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
  const second = await screen.findByRole("group", {
    name: "Replace appraisal.pdf?",
  });
  const adds = calls.filter((c) => c.cmd === "test_files_add");
  expect(adds[adds.length - 1]?.args).toEqual(expect.objectContaining({ path: "C:\\New\\CV.txt", replace: true }));

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

// Changed: Remove is an icon-only button now; its accessible name is still "Remove <file>".
test("Remove asks first; Keep leaves the file, Remove removes it", async () => {
  const { calls } = mount({
    files: [{ name: "cv.txt", size: 5, modified: "1" }],
  });
  await screen.findByText("cv.txt");

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  const confirm = screen.getByRole("group", { name: "Remove cv.txt?" });
  fireEvent.click(within(confirm).getByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("group", { name: "Remove cv.txt?" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "test_files_remove")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  fireEvent.click(
    within(screen.getByRole("group", { name: "Remove cv.txt?" })).getByRole("button", { name: "Remove" }),
  );
  expect(await screen.findByText(/No test files yet/)).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "test_files_remove")?.args).toEqual(
    expect.objectContaining({
      organization: "acme",
      project: "Web",
      name: "cv.txt",
    }),
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
  expect(
    await screen.findByText('"huge.zip" is larger than 25 MB - a test file can be at most 25 MB'),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Open folder" }));
  expect(await screen.findByText("Could not open the Test files folder.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  fireEvent.click(
    within(screen.getByRole("group", { name: "Remove cv.txt?" })).getByRole("button", { name: "Remove" }),
  );
  expect(await screen.findByText('"cv.txt" could not be removed - see Settings, Logs')).toBeInTheDocument();
});

test("a list that cannot be read says why", async () => {
  mount({
    refuse: {
      test_files_list: "the Test files folder could not be read - see Settings, Logs",
    },
  });
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

const names = (items: HTMLElement[]) => items.map((li) => li.querySelector("span")?.textContent);

test("groups files by kind in a fixed order, with a count, and shows no empty group", async () => {
  mount({
    files: [
      { name: "z.zip", size: 1, modified: "1" },
      { name: "a.json", size: 1, modified: "1" },
      { name: "b.PNG", size: 1, modified: "1" },
      { name: "c.pdf", size: 1, modified: "1" },
      { name: "d.xlsx", size: 1, modified: "1" },
      { name: "e.csv", size: 1, modified: "1" },
    ],
  });
  await screen.findByText("Documents · 1");
  // Changed: with several groups they all start closed, so open them to see the files.
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  await screen.findByText("c.pdf");
  const wrapper = screen.getByRole("group", { name: "Test files" });
  const kinds = within(wrapper)
    .getAllByRole("group")
    .map((g) => g.getAttribute("aria-label"));
  expect(kinds).toEqual(["Spreadsheets", "Documents", "Images", "Data", "Archives"]);
  expect(screen.queryByRole("group", { name: "Other" })).not.toBeInTheDocument();
  expect(screen.getByText("Spreadsheets · 2")).toBeInTheDocument();
  expect(screen.getByText("Documents · 1")).toBeInTheDocument();
  // The extension decides, in any case.
  expect(names(within(screen.getByRole("group", { name: "Images" })).getAllByRole("listitem"))).toEqual(["b.PNG"]);
});

test("an unknown or missing extension is Other, and files sort by name without regard to case", async () => {
  mount({
    files: [
      { name: "notes", size: 1, modified: "1" },
      { name: "Beta.txt", size: 1, modified: "1" },
      { name: "alpha.TXT", size: 1, modified: "1" },
      { name: "weird.xyz", size: 1, modified: "1" },
      { name: ".hidden", size: 1, modified: "1" },
      { name: "carol.md", size: 1, modified: "1" },
    ],
  });
  await screen.findByText("Other · 3");
  // Changed: two groups start closed, so open them before reading their rows.
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  const docs = within(screen.getByRole("group", { name: "Documents" })).getAllByRole("listitem");
  expect(names(docs)).toEqual(["alpha.TXT", "Beta.txt", "carol.md"]);
  const other = within(screen.getByRole("group", { name: "Other" })).getAllByRole("listitem");
  expect(names(other)).toEqual([".hidden", "notes", "weird.xyz"]);
  expect(screen.getByText("Other · 3")).toBeInTheDocument();
  const kinds = within(screen.getByRole("group", { name: "Test files" }))
    .getAllByRole("group")
    .map((g) => g.getAttribute("aria-label"));
  expect(kinds).toEqual(["Documents", "Other"]);
});

// ---- Folding, search and use --------------------------------------------

/** A script that uploads these files, in one step. */
const uploading = (...names: string[]) =>
  ({
    steps: [{ actions: names.map((file) => ({ kind: "upload", file })) }],
  }) as unknown as CaseScript;

const many: File[] = [
  { name: "a.xlsx", size: 1, modified: "1" },
  { name: "b.xlsx", size: 1, modified: "1" },
  { name: "c.pdf", size: 1, modified: "1" },
  { name: "d.png", size: 1, modified: "1" },
];

test("groups start closed, and each folds and unfolds", async () => {
  mount({ files: many });
  const sheets = await screen.findByRole("button", { name: /Spreadsheets/ });
  expect(sheets).toHaveAttribute("aria-expanded", "false");
  // A closed group keeps its list in the page, hidden, so aria-controls always points at something.
  expect(screen.getByText("a.xlsx")).not.toBeVisible();
  fireEvent.click(sheets);
  expect(sheets).toHaveAttribute("aria-expanded", "true");
  expect(sheets).toHaveAttribute("aria-controls", expect.any(String));
  expect(screen.getByText("a.xlsx")).toBeVisible();
  expect(screen.getByText("c.pdf")).not.toBeVisible();
  fireEvent.click(sheets);
  expect(screen.getByText("a.xlsx")).not.toBeVisible();
});

test("a lone group starts open", async () => {
  mount({ files: [many[0], many[1]] });
  expect(await screen.findByText("a.xlsx")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Spreadsheets/ })).toHaveAttribute("aria-expanded", "true");
});

test("a lone group stays open when a second kind is added", async () => {
  mount({ files: [many[0], many[1]], picked: ["C:\Docs\appraisal.pdf"] });
  expect(await screen.findByText("a.xlsx")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Add files" }));
  await screen.findByText(/Documents/);
  expect(screen.getByRole("button", { name: /Spreadsheets/ })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("a.xlsx")).toBeVisible();
});

test("Expand all opens every group, and the same button then collapses them", async () => {
  mount({ files: many });
  fireEvent.click(await screen.findByRole("button", { name: "Expand all" }));
  expect(screen.getByText("a.xlsx")).toBeVisible();
  expect(screen.getByText("c.pdf")).toBeVisible();
  expect(screen.getByText("d.png")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(screen.getByText("a.xlsx")).not.toBeVisible();
  expect(screen.getByText("d.png")).not.toBeVisible();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeInTheDocument();
});

test("search filters across groups, opens the matching ones and hides the rest", async () => {
  mount({ files: many });
  const box = await screen.findByRole("textbox", { name: "Search test files" });
  fireEvent.change(box, { target: { value: "B." } });
  expect(screen.getByText("b.xlsx")).toBeInTheDocument();
  expect(screen.queryByText("a.xlsx")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Spreadsheets · 1/ })).toHaveAttribute("aria-expanded", "true");
  expect(screen.queryByRole("button", { name: /Documents/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Images/ })).not.toBeInTheDocument();

  fireEvent.change(box, { target: { value: "zzz" } });
  expect(screen.getByText(/No files match "zzz"/)).toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Test files" })).not.toBeInTheDocument();

  fireEvent.change(box, { target: { value: "" } });
  expect(screen.getByRole("button", { name: /Spreadsheets · 2/ })).toHaveAttribute("aria-expanded", "false");
});

test("each file says how many cases upload it, and an unused one says so", async () => {
  mount({
    files: [
      { name: "cv.txt", size: 1, modified: "1" },
      { name: "pic.txt", size: 1, modified: "1" },
      { name: "old.txt", size: 1, modified: "1" },
    ],
    // CV.TXT matches by name in any case; a case naming a file twice counts once.
    scripts: [uploading("cv.txt", "CV.TXT"), uploading("CV.txt", "pic.txt"), null],
  });
  const row = async (name: string) => (await screen.findByText(name)).closest("li") as HTMLElement;
  expect(within(await row("cv.txt")).getByText("2 cases")).toBeInTheDocument();
  expect(within(await row("pic.txt")).getByText("1 case")).toBeInTheDocument();
  expect(
    within(await row("cv.txt"))
      .getByText("2 cases")
      .closest("[title]"),
  ).toHaveAttribute("title", "Of the cases listed on Auto Test");
  // Changed: a file no listed case uploads shows no chip - never "Unused" - because
  // other PBIs, suites, templates and components may still use it.
  expect(within(await row("old.txt")).queryByText(/case|Unused/)).not.toBeInTheDocument();
  expect(screen.queryByText("Unused")).not.toBeInTheDocument();
});

test("a file named as a component input counts as used", async () => {
  const withInput = {
    steps: [{ actions: [{ kind: "use_component", component: "Attach", inputs: { file: "CV.txt", n: 3 } }] }],
  } as unknown as CaseScript;
  mount({ files: [{ name: "cv.txt", size: 1, modified: "1" }], scripts: [withInput] });
  expect(await screen.findByText("1 case")).toBeInTheDocument();
});

test("with no scripts, or one not read yet, no chip shows", async () => {
  const files = [{ name: "cv.txt", size: 1, modified: "1" }];
  mount({ files });
  await screen.findByText("cv.txt");
  expect(screen.queryByText("Unused")).not.toBeInTheDocument();
  expect(screen.queryByText(/case/)).not.toBeInTheDocument();
});

test("a script still unread hides every chip", async () => {
  mount({
    files: [{ name: "cv.txt", size: 1, modified: "1" }],
    scripts: [uploading("cv.txt"), undefined],
  });
  await screen.findByText("cv.txt");
  expect(screen.queryByText("Unused")).not.toBeInTheDocument();
  expect(screen.queryByText("1 case")).not.toBeInTheDocument();
});

test("Remove is icon-only, named for the file", async () => {
  mount({ files: [{ name: "cv.txt", size: 1, modified: "1" }] });
  const button = await screen.findByRole("button", { name: "Remove cv.txt" });
  expect(button).toHaveAttribute("title", "Remove cv.txt");
  expect(button.textContent).toBe("");
});

test("a file cases use asks first; Keep sends nothing, Remove file removes it", async () => {
  const { calls } = mount({
    files: [{ name: "cv.txt", size: 1, modified: "1" }],
    scripts: [uploading("cv.txt"), uploading("cv.txt")],
  });
  await screen.findByText("cv.txt");
  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  const ask = screen.getByRole("group", { name: "Remove cv.txt?" });
  expect(within(ask).getByText("2 listed cases upload this file. Remove it anyway?")).toBeInTheDocument();
  fireEvent.click(within(ask).getByRole("button", { name: "Keep" }));
  expect(calls.some((c) => c.cmd === "test_files_remove")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Remove cv.txt" }));
  fireEvent.click(
    within(screen.getByRole("group", { name: "Remove cv.txt?" })).getByRole("button", { name: "Remove file" }),
  );
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "test_files_remove")?.args).toEqual(expect.objectContaining({ name: "cv.txt" })),
  );
});

test("an unused file keeps the plain confirm and removes through the same command", async () => {
  const { calls } = mount({
    files: [{ name: "old.txt", size: 1, modified: "1" }],
    scripts: [uploading("cv.txt")],
  });
  await screen.findByText("old.txt");
  fireEvent.click(screen.getByRole("button", { name: "Remove old.txt" }));
  const ask = screen.getByRole("group", { name: "Remove old.txt?" });
  expect(within(ask).queryByText(/upload this file/)).not.toBeInTheDocument();
  fireEvent.click(within(ask).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "test_files_remove")).toBe(true));
});

test("an armed Remove goes when its group folds or the search changes", async () => {
  mount({ files: many });
  fireEvent.click(await screen.findByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: "Remove a.xlsx" }));
  expect(screen.getByRole("group", { name: "Remove a.xlsx?" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Spreadsheets/ }));
  fireEvent.click(screen.getByRole("button", { name: /Spreadsheets/ }));
  expect(screen.queryByRole("group", { name: "Remove a.xlsx?" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Remove a.xlsx" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Search test files" }), { target: { value: "xlsx" } });
  expect(screen.queryByRole("group", { name: "Remove a.xlsx?" })).not.toBeInTheDocument();
});
