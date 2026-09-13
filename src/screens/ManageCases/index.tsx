import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import RelinkDialog from "../../components/RelinkDialog";
import { Button } from "../../components/ui/button";
import { IconAddToFolder, IconConfirm, IconImport, IconMoveToPbi, IconNewFolder, IconUndo } from "../../lib/actionIcons";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { buildTree, flattenTree } from "../../lib/suiteTree";
import { orderFromFile, sameOrder, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList from "./CaseOrderList";
import NewFolderDialog from "./NewFolderDialog";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

/** The suite's cases in Azure DevOps' own order. The entries carry the
 * order and the points carry the names; a case with several
 * configurations has several points and one row. */
export async function loadSuiteCases(
  org: string,
  project: string,
  planId: number,
  suiteId: number,
): Promise<SuiteCase[]> {
  const [entries, points] = await Promise.all([
    unwrap(commands.listSuiteEntries(org, project, suiteId)),
    unwrap(commands.listTestPoints(org, project, planId, suiteId)),
  ]);
  const names = new Map<number, string>();
  for (const p of points) {
    if (p.test_case_id != null && !names.has(p.test_case_id)) names.set(p.test_case_id, p.test_case_name);
  }
  return entries
    .filter((e) => e.entry_type === "testCase")
    .map((e) => ({ id: e.id, title: names.get(e.id) ?? `Test case ${e.id}` }));
}

/** Bulk operations on one suite's test cases: re-order them, move them to
 * another PBI, copy them into folders. The suite comes from the plan tree;
 * everything below the picker works on that one suite. */
export default function ManageCases({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [picked, setPicked] = useState<PickedSuite | null>(null);
  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  const planId = picked?.planId ?? 0;
  const suiteId = picked?.suite.id ?? 0;
  const cases = useQuery({
    queryKey: ["suite-cases", org, project, planId, suiteId],
    queryFn: () => loadSuiteCases(org, project, planId, suiteId),
    enabled: Boolean(picked),
    retry: false,
  });

  // A fresh read (new suite, or a reload after a save) replaces the
  // working order and clears the selection: rows may have gone.
  useEffect(() => {
    if (cases.data) {
      setOrder(cases.data);
      setSelected(new Set());
    }
  }, [cases.data]);

  const dirty = cases.data ? !sameOrder(order, cases.data) : false;

  const apply = useMutation({
    mutationFn: () => unwrap(commands.reorderSuiteCases(org, project, suiteId, order.map((c) => c.id))),
    onSuccess: () => {
      toast.success("Order saved.");
      qc.invalidateQueries({ queryKey: ["suite-cases", org, project, planId, suiteId] });
    },
    onError: (e) => toast.error(`Could not save the order: ${e.message}`),
  });

  const [relinkOpen, setRelinkOpen] = useState(false);

  /** A draft .json carries each uploaded case's id and, after the
   * optimizer's grouping pass, its tester_order. The file only proposes:
   * the list re-orders on screen and Apply order is what saves it. */
  const fromFile = useMutation({
    mutationFn: async () => {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Test case files", extensions: ["json"] }],
      });
      if (typeof path !== "string") return null;
      const parsed = await unwrapStr(commands.parseImportFile(path));
      return orderFromFile(order, parsed.cases);
    },
    onSuccess: (result) => {
      if (!result) return;
      if (result.matched === 0) {
        toast.warning("No test case in that file is in this suite. The file needs ids from an upload.");
        return;
      }
      setOrder(result.order);
      toast.info(`Placed ${result.matched} of ${order.length} test cases from the file. Apply order to save.`);
    },
    onError: (e) => toast.error(`Could not read the file: ${e.message ?? e}`),
  });

  const pbiId =
    picked?.suite.suite_type === "requirementTestSuite" ? (picked.suite.requirement_id ?? null) : null;
  const selectedCases = order.filter((c) => selected.has(c.id));

  const [folderOpen, setFolderOpen] = useState(false);
  const [targetFolder, setTargetFolder] = useState("");
  useEffect(() => setTargetFolder(""), [suiteId]);

  const INDENT = "    ";
  /** Static suites of the plan, tree order, indented: the only places a
   * folder can be created in or cases copied to. */
  const staticSuites = useMemo(
    () =>
      picked
        ? flattenTree(buildTree(picked.siblings))
            .filter(({ suite }) => suite.suite_type === "staticTestSuite")
            .map(({ suite, depth }) => ({ id: suite.id, label: `${INDENT.repeat(depth)}${suite.name}`, name: suite.name }))
        : [],
    [picked],
  );
  const parents = useMemo(
    () =>
      picked
        ? [
            ...(picked.rootSuiteId != null ? [{ id: picked.rootSuiteId, label: `Plan root (${picked.planName})` }] : []),
            ...staticSuites.map(({ id, label }) => ({ id, label })),
          ]
        : [],
    [picked, staticSuites],
  );
  const folderTargets = staticSuites.filter((s) => s.id !== suiteId);

  const addToFolder = useMutation({
    mutationFn: () => unwrap(commands.addCasesToSuite(org, project, planId, Number(targetFolder), selectedCases.map((c) => c.id))),
    onSuccess: (added) => {
      const folder = folderTargets.find((f) => String(f.id) === targetFolder)?.name ?? "the folder";
      toast.success(
        `Added ${added.length} test case${added.length === 1 ? "" : "s"} to ${folder}. ${added.length === 1 ? "It stays" : "They stay"} in ${picked!.suite.name} too.`,
      );
      setSelected(new Set());
    },
    onError: (e) => toast.error(`Could not add to the folder: ${e.message}`),
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  const busy = apply.isPending || fromFile.isPending || addToFolder.isPending;

  return (
    <div className="space-y-4">
      <SuitePicker org={org} project={project} picked={picked} onPick={setPicked} />
      {!picked && (
        <p className="text-sm text-muted">
          Pick a test plan and a suite. Its test cases appear here in the order Azure DevOps shows them.
        </p>
      )}
      {picked && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
              <IconConfirm aria-hidden />
              {apply.isPending ? "Saving" : "Apply order"}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!dirty || busy}
              onClick={() => cases.data && setOrder(cases.data)}
            >
              <IconUndo aria-hidden />
              Reset
            </Button>
            <Button size="sm" variant="ghost" disabled={busy || order.length === 0} onClick={() => fromFile.mutate()}>
              <IconImport aria-hidden />
              {fromFile.isPending ? "Reading file" : "Apply tester order from file"}
            </Button>
            <span className="mx-1 h-5 w-px bg-border" aria-hidden />
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || pbiId == null || selectedCases.length === 0}
              onClick={() => setRelinkOpen(true)}
            >
              <IconMoveToPbi aria-hidden />
              Move to PBI
            </Button>
            {pbiId == null && (
              <span className="text-xs text-faint">Move to PBI works on a PBI suite.</span>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="ghost" disabled={busy || parents.length === 0} onClick={() => setFolderOpen(true)}>
              <IconNewFolder aria-hidden />
              New folder
            </Button>
            <label className="flex items-center gap-2 text-xs text-muted">
              Folder
              <select
                aria-label="Folder"
                className="rounded-md border border-border bg-surface px-2 py-1.5 text-sm text-text focus:border-accent focus:outline-none disabled:opacity-50"
                value={targetFolder}
                disabled={busy || folderTargets.length === 0}
                onChange={(e) => setTargetFolder(e.target.value)}
              >
                <option value="">Pick a folder</option>
                {folderTargets.map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.label}
                  </option>
                ))}
              </select>
            </label>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || !targetFolder || selectedCases.length === 0}
              onClick={() => addToFolder.mutate()}
            >
              <IconAddToFolder aria-hidden />
              {addToFolder.isPending ? "Adding" : "Add to folder"}
            </Button>
            <span className="text-xs text-faint">Adding copies the selected cases; they stay in this suite.</span>
          </div>
          {cases.isLoading && <ScanProgress label="Loading test cases" />}
          {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
          {cases.data && cases.data.length === 0 && (
            <p className="text-sm text-muted">No test cases in this suite.</p>
          )}
          {cases.data && cases.data.length > 0 && (
            <CaseOrderList
              cases={order}
              selected={selected}
              onChange={setOrder}
              onSelect={setSelected}
              disabled={busy}
            />
          )}
          {relinkOpen && pbiId != null && (
            <RelinkDialog
              org={org}
              project={project}
              fromPbi={pbiId}
              cases={selectedCases}
              onClose={() => setRelinkOpen(false)}
              onMoved={() => {
                // The moved cases have left this suite: read it again.
                setSelected(new Set());
                qc.invalidateQueries({ queryKey: ["suite-cases", org, project, planId, suiteId] });
              }}
            />
          )}
          {folderOpen && picked && parents.length > 0 && (
            <NewFolderDialog
              org={org}
              project={project}
              planId={planId}
              parents={parents}
              defaultParentId={parents[0].id}
              caseIds={selectedCases.map((c) => c.id)}
              sourceName={picked.suite.name}
              onClose={() => setFolderOpen(false)}
              onCreated={() => {
                // The tree has a new suite: the picker and the folder
                // list both read from this query.
                qc.invalidateQueries({ queryKey: ["plans-suites", org, project] });
                setSelected(new Set());
              }}
            />
          )}
        </>
      )}
    </div>
  );
}
