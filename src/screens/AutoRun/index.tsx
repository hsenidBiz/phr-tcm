// Supervised and unattended auto-run: the app drives a real Edge window
// through a case's steps, either with a person watching and deciding the
// verdict, or unattended with the machine proposing one.
//
// Nothing a script or a run does reaches Azure DevOps by itself. A
// person reviewing a finished run and pressing Send (`RunReview`) is the
// one door out - see `autorun::publish` on the Rust side.

import { ChevronDown, ChevronRight } from "lucide-react";
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState, type ReactNode } from "react";
import { commands, type PbiHit } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Checkbox } from "../../components/ui/checkbox";
import { Collapse, useSettled } from "../../components/ui/collapse";
import { groupIndices } from "../../lib/grouping";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { Button } from "../../components/ui/button";
import ActionDock from "../../components/ActionDock";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { unwrap, unwrapStr } from "../../lib/ipc";
import {
  IconAccounts,
  IconCancel,
  IconClearScripts,
  IconEdit,
  IconImport,
  IconModulePaths,
  IconRecipe,
  IconRecord,
  IconRun,
  IconSiteAddress,
  IconUnattended,
} from "../../lib/actionIcons";
import { open } from "@tauri-apps/plugin-dialog";
import { toast } from "../../lib/toast";
import { Modal } from "../../components/ui/modal";
import AccountsDialog from "./AccountsDialog";
import ModulePathsDialog from "./ModulePathsDialog";
import PastRuns from "./PastRuns";
import RecipeEditor from "./RecipeEditor";
import RecordSignInDialog from "./RecordSignInDialog";
import ReplayPane from "./ReplayPane";
import RunPane from "./RunPane";
import RunReview from "./RunReview";
import ScriptEditor from "./ScriptEditor";
import SiteAddressDialog, { siteHost } from "./SiteAddressDialog";

// How many imported case ids the success toast spells out before it falls
// back to a count - the same shape as the assigned-work notification
// summary. A 60-case import naming every one of them is unreadable.
const MAX_IDS_IN_TOAST = 10;

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** One line of the Setup card: what it is, where it stands, one button.
 * A labelled group, so a screen reader hears the row's name with its state
 * and its button. */
function SetupRow({
  label,
  state,
  children,
}: {
  label: string;
  state: ReactNode;
  children: ReactNode;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border/60 pt-3 first-of-type:border-t-0 first-of-type:pt-0"
    >
      <span className="w-28 shrink-0 text-xs font-medium text-muted">{label}</span>
      <span className="min-w-0 flex-1 text-sm text-text">{state}</span>
      {children}
    </div>
  );
}

export default function AutoRun({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  const { prefs } = useFieldRefs(org, project);

  const cases = useQuery({
    queryKey: ["autorun-cases", org, pbi?.id, prefs.moduleRef, prefs.preconditionsRef],
    queryFn: () =>
      unwrap(
        commands.pbiTestCasesFull(org, pbi!.id, prefs.moduleRef, prefs.preconditionsRef),
      ),
    enabled: Boolean(org && pbi),
    retry: false,
  });

  // One script lookup per case, so the list can say which are drivable.
  const scripts = useQueries({
    queries: (cases.data ?? []).map((c) => ({
      queryKey: ["autorun-script", c.id],
      queryFn: () => unwrapStr(commands.autoRunLoadScript(c.id)),
      retry: false,
    })),
  });

  const [editing, setEditing] = useState<number | null>(null);
  const [accountsOpen, setAccountsOpen] = useState(false);
  const [recipeOpen, setRecipeOpen] = useState(false);
  const [recordOpen, setRecordOpen] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const [siteOpen, setSiteOpen] = useState(false);
  const [clearScriptsOpen, setClearScriptsOpen] = useState(false);
  const queryClient = useQueryClient();

  /** What the Setup card and the header line report. All three read small
   * local files, and each shares its query key with the dialog that edits
   * it, so a save there updates the card here. `?? null` because "nothing
   * saved yet" is an answer, not a missing one. */
  const setupReady = Boolean(org && project);
  const recipe = useQuery({
    queryKey: ["autorun-recipe", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadRecipe(org, project))) ?? null,
    enabled: setupReady,
    retry: false,
  });
  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: async () => (await unwrapStr(commands.autoRunListAccounts())) ?? null,
    retry: false,
  });
  const nav = useQuery({
    queryKey: ["autorun-nav", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadNav(org, project))) ?? null,
    enabled: setupReady,
    retry: false,
  });
  /** A count once its query has answered - `null` only while it has not
   * (pending) or could not (error). An answer of "no data" counts as none:
   * keyed on the answer rather than on the data, so the row can never sit
   * on "Loading…" after the query has already settled. */
  const accountCount = accounts.isSuccess ? (accounts.data?.length ?? 0) : null;
  const moduleCount = nav.isSuccess ? (nav.data?.modules.length ?? 0) : null;

  /** One file, many cases - the shape `save_autorun_script` writes, so an
   * assistant's whole-PBI output imports in one go. Every badge is
   * invalidated afterwards, or the rows would keep saying "No script"
   * for the cases that just gained one.
   *
   * The path goes to Rust rather than reading the file here and sending
   * its contents: `readFileB64` + `atob` decodes to a latin-1 binary
   * string, so any non-ASCII byte (an accent, a curly quote) came out
   * mojibake, and a UTF-8 BOM made the JSON look corrupt before it ever
   * reached the parser. Rust reads the bytes itself now, the same way
   * the test-case JSON importer does. */
  const importScripts = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        filters: [{ name: "Action scripts", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const r = await commands.autoRunImportScripts(org, project, path);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: async (ids) => {
      if (!ids) return;
      await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
      const shown = ids.slice(0, MAX_IDS_IN_TOAST);
      const rest = ids.length - shown.length;
      const caseList = `${shown.join(", ")}${rest > 0 ? `, and ${rest} more` : ""}`;
      toast.success(
        `Imported ${ids.length} script${ids.length === 1 ? "" : "s"} (case${
          ids.length === 1 ? "" : "s"
        } ${caseList}).`,
      );
    },
    // The generated `typedError` wrapper rethrows when the IPC call itself
    // rejects with an Error (rather than resolving to {status: "error"}) -
    // react-query's mutation still routes that here, same as an explicit
    // throw above, so this is the one place that needs to handle it.
    onError: (e) => toast.error(`Could not import that file: ${e.message}`),
  });
  /** The case ids queued for a run. `null` means no run is open. */
  const [running, setRunning] = useState<number[] | null>(null);
  /** The case ids queued for an UNATTENDED run. Its own state, separate
   * from `running` - the two panes are never open at once (both come from
   * the same selection dock), but they are different flows with different
   * dialogs. */
  const [replaying, setReplaying] = useState<number[] | null>(null);
  /** The run id under review, or null while no review dialog is open. An
   * unattended run opens straight into this once it finishes - see
   * ReplayPane's `onFinished` below. */
  const [reviewing, setReviewing] = useState<string | null>(null);
  /** Ticked cases, by id. A bulk run is these, in list order. */
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-autorun-group") === "on",
  );
  const [collapsed, toggleCollapsed] = usePersistedStringSet("tcm-v2-autorun-collapsed");

  const rows = cases.data ?? [];
  const settled = useSettled(rows.length > 0);
  /** Same title-prefix grouping View Test Cases uses, so a person reading
   * both screens is reading one idea. */
  const groups = useMemo(
    () => (grouped ? groupIndices(rows.map((c) => c.title)) : []),
    [grouped, rows],
  );
  /** The Module values of the loaded cases, for the Module paths dialog's
   * picker. A person can still type one that is not here. */
  const caseModules = useMemo(
    () =>
      Array.from(new Set(rows.map((c) => c.module_value.trim()).filter(Boolean))).sort((a, b) =>
        a.localeCompare(b),
      ),
    [rows],
  );
  const hasScript = (i: number) => Boolean(scripts[i]?.data);
  /** Only scripted cases can be run, so only they can be ticked. */
  const runnableIn = (indices: number[]) =>
    indices.filter(hasScript).map((i) => rows[i].id);

  /** Development-only housekeeping: wipe the saved scripts for every case
   * currently listed for this PBI. A missing script for one of them is not
   * an error - `store::clear_scripts` skips it - so this always hands over
   * the full list rather than just the ones the badges say are scripted. */
  const clearScripts = useMutation({
    mutationFn: () => unwrapStr(commands.autoRunClearScripts(rows.map((c) => c.id))),
    onSuccess: async (removed) => {
      setClearScriptsOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["autorun-script"] });
      toast.success(`${removed} script${removed === 1 ? "" : "s"} removed.`);
    },
    onError: (e) => toast.error(`Could not clear scripts: ${e.message}`),
  });

  const toggleOne = (id: number) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  /** A group heading ticks or clears every scripted case under it. The
   * "are they all on already" question is asked of `prev` inside the
   * updater, not of the `selected` this render closed over. */
  const toggleGroup = (indices: number[]) => {
    const ids = runnableIn(indices);
    setSelected((prev) => {
      const allOn = ids.length > 0 && ids.every((id) => prev.has(id));
      const next = new Set(prev);
      for (const id of ids) {
        if (allOn) next.delete(id);
        else next.add(id);
      }
      return next;
    });
  };

  /** List order, not click order - the run reads top to bottom the way
   * the screen does. */
  const selectedInOrder = rows.filter((c) => selected.has(c.id)).map((c) => c.id);

  /** One case row, by its index in `rows` - grouped and flat both render
   * the same thing, and `scripts[i]` is indexed the same way. */
  const row = (i: number) => {
    const c = rows[i];
    const ready = hasScript(i);
    return (
      <li
        key={c.id}
        className="flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm"
      >
        {/* Only a scripted case can be run, so only a scripted case can be
            ticked - a checkbox that selects something unrunnable would
            just make the count lie. */}
        <Checkbox
          checked={selected.has(c.id)}
          ariaLabel={`Select #${c.id}`}
          className={ready ? undefined : "invisible"}
          onCheckedChange={() => ready && toggleOne(c.id)}
        />
        <span className="id-mono text-faint">#{c.id}</span>
        {/* The title gets the room and wraps, the way Update Test Cases'
            rows do - a truncated title is exactly the part that tells two
            similar cases apart. */}
        <span className="min-w-0 flex-1 break-words text-text">{c.title}</span>
        {ready ? (
          <Badge className="shrink-0 bg-success/15 text-success">Script ready</Badge>
        ) : (
          <Badge className="shrink-0 bg-surface-2 text-faint">No script</Badge>
        )}
        {/* Script and Run are a pair of equals on every row: the same
            size and weight, so a list of twenty is not twenty bright
            buttons. The one primary action on the screen is running the
            selection, in the dock. */}
        <Button
          size="sm"
          variant="outline"
          className="shrink-0"
          aria-label={`Edit script for #${c.id}`}
          onClick={() => setEditing(c.id)}
        >
          <IconEdit aria-hidden />
          Script
        </Button>
        {ready && (
          <Button
            size="sm"
            variant="outline"
            className="shrink-0"
            aria-label={`Run #${c.id}`}
            onClick={() => setRunning([c.id])}
          >
            <IconRun aria-hidden />
            Run
          </Button>
        )}
      </li>
    );
  };

  if (!org || !pbi) {
    return <p className="text-sm text-muted">Pick a PBI in the bar above to auto-run its cases.</p>;
  }

  const needsProject = setupReady ? undefined : "Pick an organization and project first";
  const saved = recipe.data;
  const extraSites = saved ? saved.allowed_origins.length : 0;

  return (
    <div className="max-w-3xl space-y-4">
      {/* The same header line API Templates opens with: where this screen's
          runs go, at a glance, before anything else. */}
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs text-muted">
        <span>
          Project <span className="font-medium text-text">{project || "none picked"}</span>
        </span>
        {recipe.isSuccess && (
          <span>
            Runs against{" "}
            <span className="font-medium text-text">
              {saved ? siteHost(saved.start_url) : "no site set yet"}
            </span>
          </span>
        )}
        {accountCount != null && <span>{plural(accountCount, "account")}</span>}
        {moduleCount != null && <span>{plural(moduleCount, "module path")}</span>}
      </div>

      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Setup</h2>
        <div className="space-y-3">
          <SetupRow
            label="Site address"
            state={
              !setupReady ? (
                <span className="text-muted">{needsProject}</span>
              ) : recipe.isLoading ? (
                <span className="text-muted">Loading…</span>
              ) : recipe.isError ? (
                <span className="text-danger">The saved recipe could not be read</span>
              ) : saved ? (
                <>
                  <span className="id-mono break-all">{saved.start_url}</span>
                  {extraSites > 0 && (
                    <span className="ml-2 text-xs text-faint">
                      +{plural(extraSites, "allowed site")}
                    </span>
                  )}
                </>
              ) : (
                <span className="text-muted">Not set up yet</span>
              )
            }
          >
            {/* The address lives inside the recipe, so there is nothing to
                edit on its own until a recipe exists - without one, this
                row's button records the sign-in instead, which asks for
                the address first. */}
            {saved ? (
              <Button
                size="sm"
                variant="outline"
                aria-label="Edit site address"
                disabled={!setupReady}
                title={needsProject}
                onClick={() => setSiteOpen(true)}
              >
                <IconSiteAddress aria-hidden />
                Edit
              </Button>
            ) : (
              <Button
                size="sm"
                variant="outline"
                disabled={!setupReady || recipe.isLoading || recipe.isError}
                title={needsProject}
                onClick={() => setRecordOpen(true)}
              >
                <IconSiteAddress aria-hidden />
                Set up sign-in
              </Button>
            )}
          </SetupRow>

          <SetupRow
            label="Sign-in"
            state={
              !setupReady ? (
                <span className="text-muted">{needsProject}</span>
              ) : recipe.isLoading ? (
                <span className="text-muted">Loading…</span>
              ) : recipe.isError ? (
                <span className="text-danger">Could not be read - open it to see why</span>
              ) : saved ? (
                "Recipe saved"
              ) : (
                <span className="text-muted">Not set up</span>
              )
            }
          >
            {/* Record: sign in by hand once and the recipe is written.
                Edit: the recipe as JSON, for what a recording cannot say. */}
            <Button
              size="sm"
              variant="outline"
              aria-label="Record sign-in"
              disabled={!setupReady}
              title={needsProject}
              onClick={() => setRecordOpen(true)}
            >
              <IconRecord aria-hidden />
              Record
            </Button>
            <Button
              size="sm"
              variant="outline"
              aria-label="Edit sign-in recipe"
              disabled={!setupReady}
              title={needsProject}
              onClick={() => setRecipeOpen(true)}
            >
              <IconRecipe aria-hidden />
              Edit
            </Button>
          </SetupRow>

          <SetupRow
            label="Accounts"
            state={
              accounts.isError ? (
                <span className="text-danger">The accounts could not be read</span>
              ) : accounts.isPending || accountCount == null ? (
                <span className="text-muted">Loading…</span>
              ) : accountCount === 0 ? (
                <span className="text-muted">None yet</span>
              ) : (
                `${plural(accountCount, "account")} on this machine`
              )
            }
          >
            <Button
              size="sm"
              variant="outline"
              aria-label="Edit accounts"
              onClick={() => setAccountsOpen(true)}
            >
              <IconAccounts aria-hidden />
              Edit
            </Button>
          </SetupRow>

          <SetupRow
            label="Module paths"
            state={
              !setupReady ? (
                <span className="text-muted">{needsProject}</span>
              ) : nav.isError ? (
                <span className="text-danger">The module paths could not be read</span>
              ) : nav.isPending || moduleCount == null ? (
                <span className="text-muted">Loading…</span>
              ) : moduleCount === 0 ? (
                <span className="text-muted">None mapped yet</span>
              ) : (
                `${plural(moduleCount, "module path")} mapped`
              )
            }
          >
            <Button
              size="sm"
              variant="outline"
              aria-label="Edit module paths"
              disabled={!setupReady}
              title={needsProject}
              onClick={() => setNavOpen(true)}
            >
              <IconModulePaths aria-hidden />
              Edit
            </Button>
          </SetupRow>
        </div>
      </section>

      <section className="space-y-2">
        <h2 className="text-sm font-semibold text-text">
          Test cases
          {cases.data && <span className="ml-1.5 font-normal text-faint">({rows.length})</span>}
        </h2>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Button
            size="sm"
            variant="outline"
            disabled={importScripts.isPending}
            onClick={() => importScripts.mutate()}
          >
            <IconImport aria-hidden />
            Import scripts
          </Button>
          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox
              checked={grouped}
              ariaLabel="Group by title"
              onCheckedChange={(on) => {
                setGrouped(on);
                try {
                  localStorage.setItem("tcm-v2-autorun-group", on ? "on" : "off");
                } catch {
                  // storage unavailable -> the choice lasts this session
                }
              }}
            />
            Group by title
          </label>
          {/* Housekeeping shown wherever Auto Run is (dev, or unlocked) - the
              whole tab is gated in one place (`autoRunVisible` in
              lib/extras.ts), so no further gating belongs here. Disabled
              rather than hidden: a button that vanishes the moment it would
              do nothing invites "where did it go", where greyed-out with
              nothing to do reads as exactly that. Danger only on hover, the
              way the import queue's Remove reads: destructive, but
              secondary, and it still asks first. */}
          <Button
            size="sm"
            variant="outline"
            className="ml-auto hover:border-danger hover:bg-danger/10 hover:text-danger"
            disabled={!rows.some((_, i) => hasScript(i))}
            onClick={() => setClearScriptsOpen(true)}
          >
            <IconClearScripts aria-hidden />
            Clear scripts
          </Button>
        </div>
        <p className="text-xs text-faint">One JSON file can carry every case in this PBI.</p>

        {cases.isLoading && <p className="text-sm text-muted">Loading test cases…</p>}
        {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}

        {grouped ? (
          groups.map(({ name, indices }) => {
            const label = name || "Ungrouped";
            const shut = collapsed.has(label);
            return (
              <div key={label} className="space-y-1">
                <div className="flex w-full items-center gap-3 pb-1 pt-2">
                  {/* Left-anchored with a trailing rule - see ViewCases for why. */}
                  <button
                    aria-label={`${shut ? "Expand" : "Collapse"} group ${label}`}
                    title={shut ? "Expand group" : "Collapse group"}
                    className="text-muted transition-colors hover:text-accent"
                    onClick={() => toggleCollapsed(label)}
                  >
                    {shut ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                  </button>
                  {/* Same contract as the other grouped screens: selection
                      is the checkbox's job (scripted cases only), the TITLE
                      toggles the fold like the chevron. */}
                  {(() => {
                    const runnable = runnableIn(indices);
                    const on = runnable.filter((id) => selected.has(id)).length;
                    return (
                      <Checkbox
                        ariaLabel={`Select all in ${label}`}
                        checked={runnable.length > 0 && on === runnable.length}
                        indeterminate={on > 0 && on < runnable.length}
                        onCheckedChange={() => toggleGroup(indices)}
                      />
                    );
                  })()}
                  <button
                    className="group flex items-center gap-2"
                    title={shut ? "Expand group" : "Collapse group"}
                    onClick={() => toggleCollapsed(label)}
                  >
                    <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                      {label} ({indices.length})
                    </span>
                  </button>
                  <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                </div>
                <Collapse open={!shut} animateIn={settled}>
                  <ul className="space-y-1">{indices.map(row)}</ul>
                </Collapse>
              </div>
            );
          })
        ) : (
          <ul className="space-y-1">{rows.map((_, i) => row(i))}</ul>
        )}

        {/* Actions on the selection live bottom-right, in the one shared
            dock (see ActionDock): in place under the list, and floating
            bottom-right once that row has scrolled away. It only exists
            while something is ticked, so the screen never carries a
            permanently disabled button nobody can use. */}
        {selectedInOrder.length > 0 && (
          <ActionDock label="Run selection" surface className="pt-1">
            {(floating) => (
              <>
                {/* No "N cases selected" text - the count is already in the
                    button's own label, same as Run Tests' floating pill. */}
                <Button
                  size="sm"
                  tabIndex={floating ? -1 : undefined}
                  onClick={() => setRunning(selectedInOrder)}
                >
                  <IconRun aria-hidden />
                  Run {selectedInOrder.length} selected
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  tabIndex={floating ? -1 : undefined}
                  onClick={() => setReplaying(selectedInOrder)}
                >
                  <IconUnattended aria-hidden />
                  Run {selectedInOrder.length} unattended
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label="Clear selection"
                  title="Clear selection"
                  tabIndex={floating ? -1 : undefined}
                  className="rounded-full px-2 hover:text-danger"
                  onClick={() => setSelected(new Set())}
                >
                  <IconCancel aria-hidden />
                </Button>
              </>
            )}
          </ActionDock>
        )}
      </section>

      <PastRuns pbiId={pbi.id} onReview={setReviewing} />

      {accountsOpen && <AccountsDialog onClose={() => setAccountsOpen(false)} />}
      {recipeOpen && (
        <RecipeEditor org={org} project={project} onClose={() => setRecipeOpen(false)} />
      )}
      {recordOpen && (
        <RecordSignInDialog org={org} project={project} onClose={() => setRecordOpen(false)} />
      )}
      {siteOpen && (
        <SiteAddressDialog org={org} project={project} onClose={() => setSiteOpen(false)} />
      )}
      {navOpen && (
        <ModulePathsDialog
          org={org}
          project={project}
          caseModules={caseModules}
          onClose={() => setNavOpen(false)}
        />
      )}

      {clearScriptsOpen && (
        <Modal
          onClose={() => setClearScriptsOpen(false)}
          className="flex w-full max-w-md flex-col gap-4 p-5"
        >
          <h2 className="text-sm font-semibold text-text">Clear scripts?</h2>
          <p className="text-xs text-muted">
            This removes the scripts of the {rows.length} case{rows.length === 1 ? "" : "s"} listed
            for this PBI from this machine. Nothing in Azure DevOps changes.
          </p>
          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={clearScripts.isPending}
              onClick={() => setClearScriptsOpen(false)}
            >
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={clearScripts.isPending}
              onClick={() => clearScripts.mutate()}
            >
              <IconClearScripts aria-hidden />
              {clearScripts.isPending
                ? "Clearing"
                : `Clear ${rows.length} script${rows.length === 1 ? "" : "s"}`}
            </Button>
          </div>
        </Modal>
      )}

      {editing != null &&
        (() => {
          const c = (cases.data ?? []).find((x) => x.id === editing);
          if (!c) return null;
          return (
            <ScriptEditor
              caseId={c.id}
              title={c.title}
              steps={c.steps}
              org={org}
              project={project}
              onClose={() => setEditing(null)}
            />
          );
        })()}

      {running != null &&
        (() => {
          const picked = running
            .map((id) => rows.find((x) => x.id === id))
            .filter((c): c is (typeof rows)[number] => Boolean(c))
            .map((c) => ({ id: c.id, title: c.title }));
          if (picked.length === 0) return null;
          return (
            <RunPane
              org={org}
              project={project}
              pbiId={pbi.id}
              cases={picked}
              onClose={() => {
                setRunning(null);
                // The selection has been run - leaving it ticked invites a
                // second run of cases that were just decided.
                setSelected(new Set());
              }}
            />
          );
        })()}

      {replaying != null &&
        (() => {
          const picked = replaying
            .map((id) => rows.find((x) => x.id === id))
            .filter((c): c is (typeof rows)[number] => Boolean(c))
            .map((c) => ({ id: c.id, title: c.title, module: c.module_value }));
          if (picked.length === 0) return null;
          return (
            <ReplayPane
              org={org}
              project={project}
              pbiId={pbi.id}
              cases={picked}
              onClose={() => setReplaying(null)}
              onFinished={(runId) => {
                setReplaying(null);
                // Same reasoning as the supervised pane's onClose - the
                // selection has been run, so leaving it ticked invites a
                // second run of cases that were just decided.
                setSelected(new Set());
                void queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
                // Straight into its review rather than a toast pointing at
                // Past runs - every case is still unconfirmed at this point,
                // so there is nothing useful to do with this run BUT review it.
                setReviewing(runId);
              }}
            />
          );
        })()}

      {reviewing != null && (
        <RunReview
          // A different run id must mount a fresh instance - without this,
          // switching from reviewing one run straight to another (Past
          // Runs lets you) would carry the previous run's local edit state
          // (and its `baseline`) into a screen that has not loaded the
          // new run yet.
          key={reviewing}
          org={org}
          project={project}
          pbiTitle={pbi.title}
          pbiId={pbi.id}
          runId={reviewing}
          stepIds={Object.fromEntries(rows.map((c) => [c.id, c.step_ids]))}
          sharedSteps={Object.fromEntries(
            rows.map((c) => [c.id, c.steps.flatMap((s, i) => (s.shared != null ? [i + 1] : []))]),
          )}
          onClose={() => setReviewing(null)}
        />
      )}
    </div>
  );
}
