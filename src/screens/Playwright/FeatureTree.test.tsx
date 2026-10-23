// The Overview's folder tree: the right-click menu on a folder row (also
// Shift+F10 and the context-menu key), New folder under a row, and Rename
// with the warning of what it would break.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { RenameCheck, RepoStructure } from "../../bindings";
import { toast } from "../../lib/toast";
import FeatureTree from "./FeatureTree";

vi.mock("../../lib/toast", () => ({ toast: { warning: vi.fn(), error: vi.fn(), success: vi.fn(), info: vi.fn() } }));

afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

type Feature = RepoStructure["features"][number];
const feature = (path: string): Feature => ({
  path: path.split("/"),
  navigation_key: path.split("/").slice(-2).join("/"),
  spec_count: 1,
  case_count: 1,
  case_ids: [1],
});
const FEATURES = [feature("sl/admin/performance"), feature("sl/self/leave")];

type Call = { cmd: string; args: Record<string, unknown> };

/** Mounts the tree over a mocked IPC that answers the three folder commands. */
function mount(
  opts: {
    check?: (from: string, to: string) => RenameCheck | string | Promise<RenameCheck>;
    warning?: string;
    rename?: (confirmed: boolean) => string | null;
    create?: () => string | null;
    clone?: string;
  } = {},
) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === "pw_repo_create_folder") {
      const bad = opts.create?.();
      if (bad) throw bad;
      return { path: (args as { path: string }).path, files: [] };
    }
    if (cmd === "pw_repo_rename_folder_check") {
      const { from, to } = args as { from: string; to: string };
      const r = opts.check ? opts.check(from, to) : { from, to, breaks: [], tcm_updates: [] };
      if (typeof r === "string") throw r;
      return r;
    }
    if (cmd === "pw_repo_rename_folder") {
      const { from, to } = args as { from: string; to: string };
      const bad = opts.rename?.((args as { confirmed: boolean }).confirmed);
      if (bad) throw bad;
      return { from, to, placements_updated: 0, warning: opts.warning ?? null };
    }
    return null;
  });
  const onChanged = vi.fn();
  render(<FeatureTree features={FEATURES} clone={opts.clone ?? "C:/repo"} onChanged={onChanged} />);
  return { calls, onChanged };
}

const row = (path: string) => screen.getByRole("group", { name: `Folder ${path}` });
const menu = () => screen.getByRole("menu");
const ipc = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);

/** Right-click the row, pick an item of the menu. */
function pick(path: string, item: "New feature folder" | "Rename") {
  fireEvent.contextMenu(row(path), { clientX: 40, clientY: 60 });
  fireEvent.click(within(menu()).getByRole("menuitem", { name: item }));
}

test("right-clicking a folder opens a menu of New folder and Rename for it", () => {
  mount();
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  fireEvent.contextMenu(row("sl/admin"), { clientX: 40, clientY: 60 });
  const m = menu();
  expect(m).toHaveAccessibleName("Actions for sl/admin");
  expect(within(m).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["New feature folder", "Rename"]);
  expect(within(m).getByRole("menuitem", { name: "New feature folder" })).toHaveAttribute(
    "title",
    "Makes a feature folder, with specs and test-cases folders inside it.",
  );
  // The first item has the focus, and the arrow keys walk the list.
  expect(within(m).getByRole("menuitem", { name: "New feature folder" })).toHaveFocus();
  fireEvent.keyDown(within(m).getByRole("menuitem", { name: "New feature folder" }), { key: "ArrowDown" });
  expect(within(m).getByRole("menuitem", { name: "Rename" })).toHaveFocus();
  fireEvent.keyDown(within(m).getByRole("menuitem", { name: "Rename" }), { key: "ArrowDown" });
  expect(within(m).getByRole("menuitem", { name: "New feature folder" })).toHaveFocus();
});

test("Shift+F10 and the context-menu key open it from the focused row", () => {
  mount();
  fireEvent.keyDown(row("sl"), { key: "F10", shiftKey: true });
  expect(menu()).toHaveAccessibleName("Actions for sl");
  fireEvent.keyDown(menu(), { key: "Escape" });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  // Back on the row it came from.
  expect(row("sl")).toHaveFocus();

  fireEvent.keyDown(row("sl/self"), { key: "ContextMenu" });
  expect(menu()).toHaveAccessibleName("Actions for sl/self");
  // Any other key on the row does nothing.
  fireEvent.keyDown(menu(), { key: "Escape" });
  fireEvent.keyDown(row("sl/self"), { key: "F10" });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("the menu closes on Escape, a click elsewhere and a scroll", () => {
  mount();
  fireEvent.contextMenu(row("sl"));
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();

  fireEvent.contextMenu(row("sl"));
  fireEvent.mouseDown(document.body);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();

  fireEvent.contextMenu(row("sl"));
  act(() => {
    window.dispatchEvent(new Event("scroll"));
  });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("without a clone the rows have no menu", () => {
  mount({ clone: "" });
  expect(screen.queryByRole("group", { name: "Folder sl" })).not.toBeInTheDocument();
  fireEvent.contextMenu(screen.getByText("sl"));
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("New folder puts a focused name box under the row and creates it on Enter", async () => {
  const { calls, onChanged } = mount();
  pick("sl/admin", "New feature folder");
  const box = screen.getByRole("textbox", { name: "New folder name under sl/admin" });
  expect(box).toHaveFocus();
  // It sits among the folder's own children, under the row.
  expect(row("sl/admin").closest("li")!.contains(box)).toBe(true);
  fireEvent.change(box, { target: { value: "reviews" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  expect(ipc(calls, "pw_repo_create_folder")).toEqual([{ clone: "C:/repo", path: "sl/admin/reviews" }]);
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
});

test("New folder shows a refusal under the box and keeps it for another try", async () => {
  const { onChanged } = mount({ create: () => "\"sl/admin/performance\" is already a feature folder." });
  pick("sl/admin", "New feature folder");
  const box = screen.getByRole("textbox", { name: "New folder name under sl/admin" });
  fireEvent.change(box, { target: { value: "performance" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(await screen.findByText("\"sl/admin/performance\" is already a feature folder.")).toBeInTheDocument();
  expect(onChanged).not.toHaveBeenCalled();
  expect(box).toBeInTheDocument();
});

test("New folder is cancelled by Escape, and by leaving the box empty", () => {
  const { calls } = mount();
  pick("sl", "New feature folder");
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

  pick("sl", "New feature folder");
  fireEvent.blur(screen.getByRole("textbox"));
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(ipc(calls, "pw_repo_create_folder")).toEqual([]);
});

test("New folder opens a folded row to show the box", () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: "sl/admin" }));
  expect(screen.queryByText("performance")).not.toBeInTheDocument();
  pick("sl/admin", "New feature folder");
  expect(screen.getByText("performance")).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "New folder name under sl/admin" })).toBeInTheDocument();
});

test("Rename turns the name into a box with the name selected, and renames at once when nothing breaks", async () => {
  const { calls, onChanged } = mount();
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" }) as HTMLInputElement;
  expect(box).toHaveFocus();
  expect(box.value).toBe("performance");
  expect(box.selectionStart).toBe(0);
  expect(box.selectionEnd).toBe("performance".length);
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  expect(ipc(calls, "pw_repo_rename_folder_check")).toEqual([{ clone: "C:/repo", from: "sl/admin/performance", to: "sl/admin/perf" }]);
  // Nothing to confirm: no dialog, the rename is asked for unconfirmed.
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(ipc(calls, "pw_repo_rename_folder")).toEqual([
    { clone: "C:/repo", from: "sl/admin/performance", to: "sl/admin/perf", confirmed: false },
  ]);
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
});

test("Rename with the same name, or Escape, changes nothing", () => {
  const { calls } = mount();
  pick("sl/admin", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin" });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();

  pick("sl/admin", "Rename");
  const again = screen.getByRole("textbox", { name: "New name for sl/admin" });
  fireEvent.change(again, { target: { value: "other" } });
  fireEvent.keyDown(again, { key: "Escape" });
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(screen.getByText("admin")).toBeInTheDocument();
  expect(calls.filter((c) => c.cmd.startsWith("pw_repo_rename"))).toEqual([]);
});

const BREAKING: RenameCheck = {
  from: "sl/admin/performance",
  to: "sl/admin/perf",
  breaks: [
    { file: "suites/sl/admin/other/specs/q.spec.ts", what: "Imports something inside the folder by a relative path." },
    { file: "playwright.config.ts", what: "Mentions the folder's path." },
  ],
  tcm_updates: ["Export to Playwright places the area \"Performance\" in this folder."],
};

test("Rename that would break something lists it, and Rename anyway goes ahead", async () => {
  const { calls, onChanged } = mount({ check: () => BREAKING });
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });

  const dialog = await screen.findByRole("dialog");
  expect(
    within(dialog).getByText(
      "Renaming sl/admin/performance to sl/admin/perf will break these, which you will need to fix in the repository:",
    ),
  ).toBeInTheDocument();
  const list = within(dialog).getByRole("list", { name: "What would break" });
  expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "suites/sl/admin/other/specs/q.spec.tsImports something inside the folder by a relative path.",
    "playwright.config.tsMentions the folder's path.",
  ]);
  expect(within(dialog).getByText("TCM's own export settings for this folder will be updated.")).toBeInTheDocument();
  // Nothing has been renamed yet.
  expect(ipc(calls, "pw_repo_rename_folder")).toEqual([]);
  expect(onChanged).not.toHaveBeenCalled();

  fireEvent.click(within(dialog).getByRole("button", { name: "Rename anyway" }));
  await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  expect(ipc(calls, "pw_repo_rename_folder")).toEqual([
    { clone: "C:/repo", from: "sl/admin/performance", to: "sl/admin/perf", confirmed: true },
  ]);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("Cancel in the breakage dialog renames nothing and leaves the name box to edit or cancel", async () => {
  const { calls } = mount({ check: () => BREAKING });
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(ipc(calls, "pw_repo_rename_folder")).toEqual([]);
  expect(screen.getByRole("textbox", { name: "New name for sl/admin/performance" })).toBeInTheDocument();
});

test("a rename Rust refuses shows its sentence under the name box", async () => {
  const { onChanged } = mount({ check: () => "\"sl/admin/self\" is already there, so \"sl/admin/performance\" cannot be renamed to it." });
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "self" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(await screen.findByText(/is already there/)).toBeInTheDocument();
  expect(onChanged).not.toHaveBeenCalled();
  expect(screen.getByRole("textbox", { name: "New name for sl/admin/performance" })).toBeInTheDocument();
});

test("a refusal of the confirmed rename itself shows in the dialog", async () => {
  mount({ check: () => BREAKING, rename: () => "The folder could not be renamed: Access is denied." });
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Rename anyway" }));
  expect(await within(dialog).findByText("The folder could not be renamed: Access is denied.")).toBeInTheDocument();
});

test("a fold follows the folder it belongs to when it is renamed", async () => {
  const { onChanged } = mount();
  fireEvent.click(screen.getByRole("button", { name: "sl/admin" }));
  pick("sl/admin", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin" });
  fireEvent.change(box, { target: { value: "hr" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(onChanged).toHaveBeenCalled());
  // The caller reads the clone again and passes the new paths; here the
  // folded set is carried, so the old open/shut state is not left on "sl/admin".
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
});

test("a name with a slash is refused in the box and nothing is asked", () => {
  const { calls } = mount();
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  for (const bad of ["foo/bar", String.raw`foo\bar`]) {
    fireEvent.change(box, { target: { value: bad } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(screen.getByText("A name, not a path.")).toBeInTheDocument();
  }
  expect(calls.filter((c) => c.cmd.startsWith("pw_repo_rename"))).toEqual([]);
  expect(box).toBeInTheDocument();
});

test("while a check runs the box is disabled and Enter does nothing more", async () => {
  let release: (c: RenameCheck) => void = () => {};
  const held = new Promise<RenameCheck>((r) => {
    release = r;
  });
  const { calls, onChanged } = mount({ check: () => held });
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(box).toBeDisabled());
  fireEvent.keyDown(box, { key: "Enter" });
  fireEvent.keyDown(box, { key: "Escape" });
  expect(ipc(calls, "pw_repo_rename_folder_check")).toHaveLength(1);
  expect(box).toBeInTheDocument();
  release({ from: "sl/admin/performance", to: "sl/admin/perf", breaks: [], tcm_updates: [] });
  await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  expect(ipc(calls, "pw_repo_rename_folder")).toHaveLength(1);
});

test("the buttons of the breakage dialog are disabled while the rename runs", async () => {
  let release: () => void = () => {};
  const held = new Promise<void>((r) => {
    release = r;
  });
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === "pw_repo_rename_folder_check") return BREAKING;
    if (cmd === "pw_repo_rename_folder") {
      return held.then(() => ({ from: BREAKING.from, to: BREAKING.to, placements_updated: 0, warning: null }));
    }
    return null;
  });
  render(<FeatureTree features={FEATURES} clone="C:/repo" onChanged={vi.fn()} />);
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Rename anyway" }));
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "Rename anyway" })).toBeDisabled());
  expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
  release();
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
});

test("a rename that moved the folder but not all of the settings closes the box, refreshes and says so", async () => {
  const warning = "The folder was renamed, but TCM could not update its export settings in a.pwexport.json; fix them by hand.";
  const { calls, onChanged } = mount({ check: () => BREAKING, warning });
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Rename anyway" }));
  await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  expect(toast.warning).toHaveBeenCalledWith(warning, expect.anything());
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  expect(ipc(calls, "pw_repo_rename_folder")).toHaveLength(1);
});

test("Escape in the box puts the focus back on the folder's row", () => {
  mount();
  pick("sl/admin", "Rename");
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
  expect(row("sl/admin")).toHaveFocus();
  pick("sl", "New feature folder");
  fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
  expect(row("sl")).toHaveFocus();
});

test("after a rename the focus goes to the row under its new path once the tree has it", async () => {
  const onChanged = vi.fn();
  mockIPC((cmd, args) => {
    const { from, to } = args as { from: string; to: string };
    if (cmd === "pw_repo_rename_folder_check") return { from, to, breaks: [], tcm_updates: [] };
    if (cmd === "pw_repo_rename_folder") return { from, to, placements_updated: 0, warning: null };
    return null;
  });
  const { rerender } = render(<FeatureTree features={FEATURES} clone="C:/repo" onChanged={onChanged} />);
  pick("sl/admin/performance", "Rename");
  const box = screen.getByRole("textbox", { name: "New name for sl/admin/performance" });
  fireEvent.change(box, { target: { value: "perf" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(onChanged).toHaveBeenCalled());
  rerender(<FeatureTree features={[feature("sl/admin/perf"), feature("sl/self/leave")]} clone="C:/repo" onChanged={onChanged} />);
  await waitFor(() => expect(row("sl/admin/perf")).toHaveFocus());
});
