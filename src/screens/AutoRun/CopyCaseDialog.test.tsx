// Copying one case's script between environments, in either direction: the
// confirmation says what comes across, what the other side keeps its own
// version of, and what the person still has to add.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { CaseCopyPlan } from "../../bindings";
import { setActiveEnvId } from "../../lib/activeEnv";
import { toast } from "../../lib/toast";
import CopyCaseDialog from "./CopyCaseDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
  localStorage.clear();
});

const env = (id: string, name: string) => ({
  id, name, start_url: "", allowed_origins: [], db_id: "d", test_environment: false,
  test_prefix: "AUTOTEST", has_default_password: false, db_auto_approve: false,
});
const DEFAULT = env("env-00000001", "Default");
const QA = env("env-00000002", "QA");
const EMPTY_PLAN: CaseCopyPlan = {
  target_has_script: false, components_added: [], components_kept: [], missing_components: [],
  areas_added: [], areas_kept: [], uses_default_area: false, recipe_added: false, recipe_address: null,
  missing_accounts: [], missing_api: [],
};

function mount(
  plan: Partial<CaseCopyPlan>,
  direction?: "to" | "from",
  handlers: Record<string, (a: Record<string, unknown>) => unknown> = {},
) {
  setActiveEnvId(DEFAULT.id);
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (handlers[cmd]) return handlers[cmd](a);
    if (cmd === "env_list") return { active: DEFAULT.id, environments: [DEFAULT, QA] };
    if (cmd.startsWith("auto_run_copy_case")) return { ...EMPTY_PLAN, ...plan };
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CopyCaseDialog caseId={7} org="acme" project="Web" direction={direction} onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

async function pick(label: string, option: string) {
  fireEvent.click(await screen.findByRole("combobox", { name: label }));
  fireEvent.click(await screen.findByRole("option", { name: option }));
}

test("offers only the other environments as the target", async () => {
  mount({});
  fireEvent.click(await screen.findByRole("combobox", { name: "Copy to" }));
  expect(await screen.findByRole("option", { name: "QA" })).toBeInTheDocument();
  expect(screen.queryByRole("option", { name: "Default" })).not.toBeInTheDocument();
});

test("lists what a case copy brings, keeps and lacks before copying", async () => {
  const { calls, onClose } = mount({
    components_added: ["Login"],
    areas_added: ["Cycle Setup"],
    recipe_added: true,
    recipe_address: "https://hr.example.com/",
    components_kept: ["Open menu"],
    areas_kept: ["Manage Cycle"],
    missing_accounts: ["hr.admin"],
    missing_components: ["Approve"],
    missing_api: ["fixture-1"],
    uses_default_area: true,
  });
  await pick("Copy to", "QA");
  expect(await screen.findByText("Brings: component Login, area Cycle Setup, the sign-in recording.")).toBeInTheDocument();
  expect(screen.getByText(/https:\/\/hr\.example\.com\//)).toBeInTheDocument();
  expect(screen.getByText("Uses QA's own: component Open menu, area Manage Cycle.")).toBeInTheDocument();
  expect(screen.getByText("Add the account hr.admin in QA's Accounts before running.")).toBeInTheDocument();
  expect(screen.getByText(/Approve/)).toHaveTextContent("the case will fail until it is added");
  expect(screen.getByText("QA lacks the API data this case needs: fixture-1.")).toBeInTheDocument();
  expect(screen.getByText("This case uses its module's default area: check QA has it.")).toBeInTheDocument();
  // Nothing was written by looking.
  expect(calls.some((c) => c.cmd === "auto_run_copy_case")).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "auto_run_copy_case")!.args).toEqual({
    organization: "acme", project: "Web", caseId: 7, activeId: DEFAULT.id, targetId: QA.id, replace: false,
  });
  expect(toast.success).toHaveBeenCalledWith("Copied case #7 into QA");
});

test("asks before replacing the target's script", async () => {
  const { calls, onClose } = mount({ target_has_script: true });
  await pick("Copy to", "QA");
  expect(await screen.findByText("QA already has a script for this case. Copying replaces it.")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Replace QA's script" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "auto_run_copy_case")!.args.replace).toBe(true);
});

test("a refused plan shows as its sentence and offers no copy", async () => {
  mount({}, "to", {
    auto_run_copy_case_plan: () => {
      throw "that is the environment you are in - to bring a case in, use Copy from environment on the case instead";
    },
  });
  await pick("Copy to", "QA");
  expect(await screen.findByRole("alert")).toHaveTextContent("use Copy from environment on the case instead");
  expect(screen.queryByRole("button", { name: "Copy" })).not.toBeInTheDocument();
});

test("a refusal while copying shows inline and keeps the dialog open", async () => {
  const { onClose } = mount({}, "to", {
    auto_run_copy_case: () => {
      throw "the environment was switched - nothing was copied; look at the case again in the environment now active";
    },
  });
  await pick("Copy to", "QA");
  fireEvent.click(await screen.findByRole("button", { name: "Copy" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("The environment changed while this dialog was open");
  expect(onClose).not.toHaveBeenCalled();
});

test("copying from another environment uses the pull commands and names the active one as the target", async () => {
  const { calls, onClose } = mount({ target_has_script: true }, "from");
  await pick("Copy from", "QA");
  expect(await screen.findByText("Default already has a script for this case. Copying replaces it.")).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "auto_run_copy_case_from_plan")!.args).toEqual({
    organization: "acme", project: "Web", caseId: 7, activeId: DEFAULT.id, sourceId: QA.id,
  });
  fireEvent.click(screen.getByRole("button", { name: "Replace Default's script" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(calls.find((c) => c.cmd === "auto_run_copy_case_from")!.args).toEqual({
    organization: "acme", project: "Web", caseId: 7, activeId: DEFAULT.id, sourceId: QA.id, replace: true,
  });
});

test("a plan result is announced politely", async () => {
  mount({ missing_accounts: ["hr.admin"] });
  await pick("Copy to", "QA");
  const line = await screen.findByText("Add the account hr.admin in QA's Accounts before running.");
  expect(line.closest("[aria-live]")).toHaveAttribute("aria-live", "polite");
});

test("after a needs-confirm refusal the dialog asks to replace instead of repeating the refusal", async () => {
  let has = false;
  const { calls } = mount({}, "to", {
    auto_run_copy_case_plan: () => ({ ...EMPTY_PLAN, target_has_script: has }),
    auto_run_copy_case: (a) => {
      if (!a.replace) throw "that environment already has a script for this case - confirm to replace it";
      return EMPTY_PLAN;
    },
  });
  await pick("Copy to", "QA");
  const copy = await screen.findByRole("button", { name: "Copy" });
  has = true;
  fireEvent.click(copy);
  fireEvent.click(await screen.findByRole("button", { name: "Replace QA's script" }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "auto_run_copy_case")).toHaveLength(2));
  expect(calls.filter((c) => c.cmd === "auto_run_copy_case")[1].args.replace).toBe(true);
});

test("a copy refused because the environment changed is put in screen wording", async () => {
  mount({}, "to", {
    auto_run_copy_case: () => {
      throw "the environment was switched - nothing was copied; look at the case again in the environment now active";
    },
  });
  await pick("Copy to", "QA");
  fireEvent.click(await screen.findByRole("button", { name: "Copy" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The environment changed while this dialog was open - open it again.",
  );
});

test("a copy refused because Auto Run is busy says so in copy wording", async () => {
  mount({}, "to", {
    auto_run_copy_case: () => {
      throw "the environment cannot be switched while something is being recorded or run in Auto Run - finish or cancel it first";
    },
  });
  await pick("Copy to", "QA");
  fireEvent.click(await screen.findByRole("button", { name: "Copy" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Nothing can be copied while something is open or running in Auto Run - finish or close it first.",
  );
});

test("a blocked dialog says why and offers no copy", async () => {
  setActiveEnvId(DEFAULT.id);
  mockIPC((cmd) => (cmd === "env_list" ? { active: DEFAULT.id, environments: [DEFAULT, QA] } : null));
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CopyCaseDialog caseId={7} org="acme" project="Web" direction="from" blockedReason="Save or discard your changes first" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(await screen.findByRole("alert")).toHaveTextContent("Save or discard your changes first");
  expect(screen.queryByRole("combobox", { name: "Copy from" })).not.toBeInTheDocument();
});
