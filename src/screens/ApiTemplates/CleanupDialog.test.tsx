// Clean up test-made drafts: the filters preview again, a line with no
// delete template cannot be ticked, the exact confirm sentence, Cancel,
// results as they come, Stop, and the sentence for an empty preview.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CleanupDialog, { confirmSentence, NOTHING_MATCHES } from "./CleanupDialog";

vi.mock("../../lib/uiLog", () => ({ logUi: vi.fn() }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const env = (id: string, name: string, prefix: string) => ({
  id,
  name,
  start_url: "",
  allowed_origins: [],
  db_id: "db-1",
  test_environment: true,
  test_prefix: prefix,
  has_default_password: false,
});

const ENVS = { active: "env-aaaa0001", environments: [env("env-aaaa0001", "Default", "AUTOTEST"), env("env-aaaa0002", "Staging", "STAGE")] };

const DAY = 86_400_000;

const line = (id: string, name: string, deletable: boolean, kind = "cycle") => ({
  entry: {
    environment: "env-aaaa0001",
    kind,
    id,
    name,
    created_at: new Date(Date.now() - 9 * DAY).toISOString(),
    fixture: "draft-cycle",
    run_id: "run-1",
    status: "present",
  },
  deletable,
  note: deletable ? null : `no proven delete template for ${kind}`,
});

type Call = { cmd: string; args: Record<string, unknown> };

/** The dialog over a preview of `lines`; `run` answers the cleanup. */
function mount(lines: unknown[], run: () => unknown = () => ({ results: [], total: 0, stopped: false })) {
  const calls: Call[] = [];
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args: a });
      if (cmd === "db_databases") return [];
      if (cmd === "env_list") return ENVS;
      if (cmd === "auto_run_cleanup_preview") return lines;
      if (cmd === "auto_run_cleanup_run") return run();
      if (cmd === "auto_run_cleanup_stop") return null;
      return undefined;
    },
    { shouldMockEvents: true },
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <CleanupDialog org="acme" project="proj" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

const previews = (calls: Call[]) => calls.filter((c) => c.cmd === "auto_run_cleanup_preview").map((c) => c.args);
const last = <T,>(xs: T[]): T | undefined => xs[xs.length - 1];

test("the preview starts from the active environment, its prefix and 7 days, and every filter change previews again", async () => {
  const { calls } = mount([]);
  await waitFor(() =>
    expect(last(previews(calls))).toEqual({
      organization: "acme",
      project: "proj",
      environment: "env-aaaa0001",
      prefix: "AUTOTEST",
      olderThanDays: 7,
    }),
  );

  fireEvent.change(screen.getByRole("spinbutton", { name: "Older than (days)" }), { target: { value: "10" } });
  await waitFor(() => expect(last(previews(calls))).toMatchObject({ olderThanDays: 10 }));

  fireEvent.change(screen.getByRole("textbox", { name: "Name starts with" }), { target: { value: "QA" } });
  await waitFor(() => expect(last(previews(calls))).toMatchObject({ prefix: "QA", olderThanDays: 10 }));

  // Another environment brings its own prefix.
  fireEvent.click(screen.getByRole("combobox", { name: "Environment" }));
  fireEvent.click(screen.getByRole("option", { name: "Staging" }));
  await waitFor(() =>
    expect(last(previews(calls))).toMatchObject({ environment: "env-aaaa0002", prefix: "STAGE", olderThanDays: 10 }),
  );
  expect(screen.getByRole("textbox", { name: "Name starts with" })).toHaveValue("STAGE");
  // Deleting is only for the active environment.
  expect(screen.getByText("Clean up only deletes in the active environment. Switch to Staging first.")).toBeInTheDocument();
});

test("each line shows its kind, name, id, age and fixture; a line with no delete template cannot be ticked", async () => {
  mount([line("274", "AUTOTEST cycle", true), line("9", "AUTOTEST suite", false, "suite")]);
  const row = await screen.findByRole("listitem", { name: "cycle AUTOTEST cycle" });
  expect(row).toHaveTextContent("cycle");
  expect(row).toHaveTextContent("id 274");
  expect(row).toHaveTextContent("9 days old");
  expect(row).toHaveTextContent("made by draft-cycle");

  const ticked = screen.getByRole("checkbox", { name: "Delete cycle AUTOTEST cycle" });
  await waitFor(() => expect(ticked).toHaveAttribute("aria-checked", "true"));

  const other = screen.getByRole("listitem", { name: "suite AUTOTEST suite" });
  expect(other).toHaveTextContent("no proven delete template for suite");
  const box = within(other).getByRole("checkbox", { name: "Delete suite AUTOTEST suite" });
  expect(box).toHaveAttribute("aria-checked", "false");
  expect(box).toHaveAttribute("aria-disabled", "true");
  fireEvent.click(box);
  expect(box).toHaveAttribute("aria-checked", "false");
  expect(screen.getByRole("button", { name: "Delete 1 drafts" })).toBeEnabled();
});

test("Delete asks with the exact sentence, and Cancel deletes nothing", async () => {
  const { calls } = mount([line("274", "AUTOTEST a", true), line("275", "AUTOTEST b", true)]);
  fireEvent.click(await screen.findByRole("button", { name: "Delete 2 drafts" }));
  expect(screen.getByText("Delete 2 drafts from Default? This cannot be undone.")).toBeInTheDocument();
  expect(confirmSentence(2, "Default")).toBe("Delete 2 drafts from Default? This cannot be undone.");

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByText(confirmSentence(2, "Default"))).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "auto_run_cleanup_run")).toBe(false);

  // Unticking one changes the count.
  fireEvent.click(screen.getByRole("checkbox", { name: "Delete cycle AUTOTEST b" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete 1 drafts" }));
  expect(screen.getByText(confirmSentence(1, "Default"))).toBeInTheDocument();
});

test("results come in as each delete ends, and Stop calls the command", async () => {
  let finish: (v: unknown) => void = () => undefined;
  const { calls } = mount([line("274", "AUTOTEST a", true), line("275", "AUTOTEST b", true)], () => new Promise((r) => (finish = r)));
  fireEvent.click(await screen.findByRole("button", { name: "Delete 2 drafts" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "auto_run_cleanup_run")?.args).toEqual({
    organization: "acme",
    project: "proj",
    environment: "env-aaaa0001",
    prefix: "AUTOTEST",
    olderThanDays: 7,
    entries: [
      { kind: "cycle", id: "274" },
      { kind: "cycle", id: "275" },
    ],
  }));

  await act(() => emit("autorun-cleanup-progress", { done: 1, total: 2, kind: "cycle", id: "274", outcome: "deleted" }));
  const results = await screen.findByRole("list", { name: "Results" });
  expect(results).toHaveTextContent("cycle AUTOTEST a: deleted");

  await act(() =>
    emit("autorun-cleanup-progress", { done: 2, total: 2, kind: "cycle", id: "275", outcome: "failed at Delete: expected status 200, got 500" }),
  );
  await waitFor(() => expect(results).toHaveTextContent("cycle AUTOTEST b: failed at Delete: expected status 200, got 500"));

  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "auto_run_cleanup_stop")).toBe(true));

  await act(async () => finish({ results: [], total: 2, stopped: true }));
  expect(await screen.findByRole("button", { name: "Close" })).toBeInTheDocument();
});

test("a tick is the kind and id together: cycle 42 and suite 42 tick apart, and only the ticked pair is sent", async () => {
  const { calls } = mount(
    [line("42", "AUTOTEST cycle", true), line("42", "AUTOTEST suite", true, "suite")],
    () => new Promise(() => undefined),
  );
  const suite = await screen.findByRole("checkbox", { name: "Delete suite AUTOTEST suite" });
  const cycle = screen.getByRole("checkbox", { name: "Delete cycle AUTOTEST cycle" });
  await waitFor(() => expect(suite).toHaveAttribute("aria-checked", "true"));
  fireEvent.click(suite);
  expect(suite).toHaveAttribute("aria-checked", "false");
  expect(cycle).toHaveAttribute("aria-checked", "true");

  fireEvent.click(screen.getByRole("button", { name: "Delete 1 drafts" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_cleanup_run")?.args.entries).toEqual([{ kind: "cycle", id: "42" }]),
  );
  await act(() => emit("autorun-cleanup-progress", { done: 1, total: 1, kind: "cycle", id: "42", outcome: "deleted" }));
  expect(await screen.findByRole("list", { name: "Results" })).toHaveTextContent("cycle AUTOTEST cycle: deleted");
});

test("an empty preview says so and offers nothing to delete", async () => {
  mount([]);
  expect(await screen.findByText(NOTHING_MATCHES)).toBeInTheDocument();
  expect(NOTHING_MATCHES).toBe("No test-made drafts match.");
  expect(screen.getByRole("button", { name: "Delete 0 drafts" })).toBeDisabled();
});
