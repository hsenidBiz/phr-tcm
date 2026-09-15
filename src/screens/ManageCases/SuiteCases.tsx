import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../../bindings";
import ScanProgress from "../../components/ScanProgress";
import { Button } from "../../components/ui/button";
import { Switch } from "../../components/ui/switch";
import { IconCollapseAll, IconConfirm, IconImport, IconUndo } from "../../lib/actionIcons";
import { CACHE, cacheKeys, persistentQuery } from "../../lib/cache";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { useOnScreen } from "../../hooks/useOnScreen";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { orderByGroups, orderGroupsAZ, sameOrder, sectionsOf, type SuiteCase } from "../../lib/suiteOrder";
import CaseOrderList, { sectionLabel } from "./CaseOrderList";
import FileOrderDialog, { type OrderFile } from "./FileOrderDialog";
import { markSuiteFloating, useFloatRank } from "./floatingSuites";
import { loadSuiteCases, suiteCasesKey } from "./suiteCasesQuery";

/** One expanded suite: its cases in Azure DevOps' order, re-orderable and
 * selectable. Order lives here (Apply saves it, Reset drops it); which
 * rows are selected lives in the screen, because a selection can span
 * the suites of a plan. */
export default function SuiteCases({
  org,
  project,
  planId,
  suiteId,
  suiteName,
  selected,
  onToggle,
}: {
  org: string;
  project: string;
  planId: number;
  suiteId: number;
  /** For the case list's accessible name: several lists can be open at once. */
  suiteName: string;
  /** Ids of this suite's cases the screen holds selected. */
  selected: Set<number>;
  /** Cases the user just checked (on) or unchecked (off). */
  onToggle: (cases: SuiteCase[], on: boolean) => void;
}) {
  const qc = useQueryClient();
  const key = suiteCasesKey(org, project, planId, suiteId);
  // Reopening a suite must not re-read it: two Azure DevOps calls per
  // suite made Suite Management feel like it was loading from scratch
  // every time, even standing still on one PBI. Served from disk at once,
  // refreshed in the background after five minutes - and this screen's own
  // edits (reorder, copy in, new suite) invalidate the key, so the user's
  // own changes never wait for that.
  const cases = useQuery({
    queryKey: key,
    ...persistentQuery({
      key: cacheKeys.suiteCases(org, project, planId, suiteId),
      fetcher: () => loadSuiteCases(org, project, planId, suiteId),
      ...CACHE.structure,
      staleMs: 5 * 60_000,
    }),
    retry: false,
  });

  // The order on screen. Starts as the server's and drifts as the user
  // drags; Apply sends it, Reset throws it away. A fresh read replaces it.
  const [order, setOrder] = useState<SuiteCase[]>([]);
  // Remembered app-wide like the other screens' group switches. Turning
  // it ON arranges the list (a real reorder - Apply order lights up);
  // OFF only hides the headers. A suite opened with it already on is NOT
  // rearranged: nothing the user did not ask for may make the list dirty.
  const [grouped, setGrouped] = useState(() => {
    try {
      return localStorage.getItem("tcm-v2-group-manage") === "on";
    } catch {
      return false;
    }
  });
  const setGroupedAndRemember = (on: boolean) => {
    setGrouped(on);
    try {
      localStorage.setItem("tcm-v2-group-manage", on ? "on" : "off");
    } catch {
      // session-only
    }
    if (on) setOrder((o) => orderByGroups(o));
  };
  // Folded groups, remembered by name across suites and sessions - the
  // same store every other grouped list in the app keeps its own in.
  const [collapsed, toggleCollapsed, collapseGroups, expandGroups] = usePersistedStringSet(
    "tcm-v2-manage-collapsed-groups",
  );
  const groupNames = grouped ? [...new Set(sectionsOf(order).map((x) => sectionLabel(x.name)))] : [];
  const anyOpen = groupNames.some((n) => !collapsed.has(n));
  useEffect(() => {
    if (cases.data) setOrder(cases.data);
  }, [cases.data]);
  const dirty = cases.data ? !sameOrder(order, cases.data) : false;
  // The floating bar follows the user down a long suite: it shows once this
  // suite's own toolbar has scrolled out of view while its cases are still
  // on screen, and stands down when the toolbar comes back - the same deal
  // as the Import tab's floating Review button. An unsaved order keeps it up
  // regardless, so a change is never out of reach.
  const [toolbarRef, toolbarOnScreen] = useOnScreen();
  const [listRef, listOnScreen] = useOnScreen();
  const following = !toolbarOnScreen && listOnScreen;
  const floating = dirty || following;
  const rank = useFloatRank(suiteId);
  useEffect(() => {
    markSuiteFloating(suiteId, floating);
  }, [suiteId, floating]);
  useEffect(() => () => markSuiteFloating(suiteId, false), [suiteId]);

  const apply = useMutation({
    mutationFn: () => unwrap(commands.reorderSuiteCases(org, project, suiteId, order.map((c) => c.id))),
    onSuccess: (serverIds) => {
      toast.success("Order saved.");
      // Write the server's own order into the cache first so `dirty`
      // reads false at once, instead of Apply flipping back on for the
      // beat before the refetch lands.
      const byId = new Map(order.map((c) => [c.id, c]));
      const next = serverIds.map((id) => byId.get(id)).filter((c): c is SuiteCase => c != null);
      qc.setQueryData(key, next);
      qc.invalidateQueries({ queryKey: key });
    },
    onError: (e) => toast.error(`Could not save the order: ${e.message}`),
  });

  // The files the dialog is arranging; null = no dialog. Parsing happens
  // here, where the IPC and its error toast live; the dialog only arranges.
  const [orderFiles, setOrderFiles] = useState<OrderFile[] | null>(null);
  const pickFiles = useMutation({
    mutationFn: async () => {
      const picked = await open({
        multiple: true,
        directory: false,
        filters: [{ name: "Test case files", extensions: ["json"] }],
      });
      const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
      const out: OrderFile[] = [];
      for (const path of paths) {
        const parsed = await unwrapStr(commands.parseImportFile(path));
        out.push({ path, name: path.split(/[\\/]/).pop() ?? path, cases: parsed.cases });
      }
      return out;
    },
    onSuccess: (files) => {
      if (files.length === 0) return;
      setOrderFiles((cur) => {
        const known = new Set((cur ?? []).map((f) => f.path));
        return [...(cur ?? []), ...files.filter((f) => !known.has(f.path))];
      });
    },
    onError: (e) => toast.error(`Could not read the file: ${e.message ?? e}`),
  });

  const busy = apply.isPending || pickFiles.isPending;

  if (cases.isLoading) return <ScanProgress label="Loading test cases" className="my-2" />;
  if (cases.isError) return <p className="my-2 text-sm text-danger">{cases.error.message}</p>;
  if (!cases.data || cases.data.length === 0)
    return <p className="my-2 text-sm text-muted">No test cases in this suite.</p>;

  return (
    <div className="my-2 space-y-2">
      <div ref={toolbarRef} className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
          <IconConfirm aria-hidden />
          {apply.isPending ? "Saving" : "Apply order"}
        </Button>
        <Button size="sm" variant="ghost" disabled={!dirty || busy} onClick={() => cases.data && setOrder(cases.data)}>
          <IconUndo aria-hidden />
          Reset
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => pickFiles.mutate()}>
          <IconImport aria-hidden />
          {pickFiles.isPending ? "Reading files" : "Apply order from files"}
        </Button>
        <label className="ml-2 flex items-center gap-2 text-xs text-muted">
          <Switch checked={grouped} onCheckedChange={setGroupedAndRemember} ariaLabel="Group by title" />
          Group by title
        </label>
        {grouped && (
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            title="Every group together, groups A to Z"
            onClick={() => setOrder((o) => orderGroupsAZ(o))}
          >
            A-Z groups
          </Button>
        )}
        {grouped && groupNames.length > 0 && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => (anyOpen ? collapseGroups(groupNames) : expandGroups(groupNames))}
          >
            <IconCollapseAll aria-hidden />
            {anyOpen ? "Collapse groups" : "Expand groups"}
          </Button>
        )}
      </div>
      <div ref={listRef}>
      <CaseOrderList
        cases={order}
        selected={selected}
        ariaLabel={`Test cases in ${suiteName}`}
        onChange={setOrder}
        onSelect={(next) => {
          const added = order.filter((c) => next.has(c.id) && !selected.has(c.id));
          const removed = order.filter((c) => !next.has(c.id) && selected.has(c.id));
          if (added.length) onToggle(added, true);
          if (removed.length) onToggle(removed, false);
        }}
        disabled={busy}
        grouped={grouped}
        collapsed={collapsed}
        onToggleCollapsed={toggleCollapsed}
      />
      </div>
      {rank != null &&
        // Bottom RIGHT of the app window, like the Import tab's floating
        // Review button: a long suite puts the toolbar a screen and a half
        // above the row being dragged. Stacked by rank, because several
        // suites can be open at once.
        //
        // Portalled to <body>: this screen renders inside AnimatedContent,
        // whose GSAP transform makes `fixed` mean the scroll region instead
        // of the window - which is why this bar, unportalled, scrolled away
        // with the page instead of staying put.
        createPortal(
          <div
            role="region"
            aria-label={`Order actions for ${suiteName}`}
            className="fixed right-6 z-40 flex items-center gap-2 rounded-full border border-border bg-surface p-2.5 shadow-2xl"
            style={{ bottom: `${1.5 + rank * 3.5}rem` }}
          >
            <span className="max-w-48 truncate pl-1 text-xs text-muted">{suiteName}</span>
            <Button size="sm" disabled={!dirty || busy} onClick={() => apply.mutate()}>
              <IconConfirm aria-hidden />
              {apply.isPending ? "Saving" : "Apply order"}
            </Button>
            <Button size="sm" variant="ghost" disabled={!dirty || busy} onClick={() => cases.data && setOrder(cases.data)}>
              <IconUndo aria-hidden />
              Reset
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => pickFiles.mutate()}>
              <IconImport aria-hidden />
              {pickFiles.isPending ? "Reading files" : "Apply order from files"}
            </Button>
          </div>,
          document.body,
        )}
      {orderFiles && orderFiles.length > 0 && (
        <FileOrderDialog
          suiteCases={order}
          files={orderFiles}
          onAddFiles={() => pickFiles.mutate()}
          onClose={() => setOrderFiles(null)}
          onApply={(next, placedTotal) => {
            setOrder(next);
            setOrderFiles(null);
            toast.info(
              `Placed ${placedTotal} of ${order.length} test cases from ${orderFiles.length === 1 ? "1 file" : `${orderFiles.length} files`}. Apply order to save.`,
            );
          }}
        />
      )}
    </div>
  );
}
