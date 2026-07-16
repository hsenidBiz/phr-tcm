import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MessageSquarePlus, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type PbiHit, type TestCase, type TestCaseFull } from "../bindings";
import PickPbiEmpty from "../components/PickPbiEmpty";
import { Button } from "../components/ui/button";
import { Input, Textarea } from "../components/ui/input";
import { Skeleton } from "../components/ui/skeleton";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { loadNotes, saveNote } from "../lib/caseNotes";
import { unwrapStr } from "../lib/ipc";
import { unwrap } from "../lib/ipc";

function toTestCase(c: TestCaseFull): TestCase {
  return {
    title: c.title,
    steps: c.steps,
    tags: c.tags,
    automation_status: c.automation_status,
    module_value: c.module_value,
    preconditions: c.preconditions,
    update_id: c.id,
  };
}

/** One case, fully spelled out (steps always visible), plus the personal
 * local comment: a scratchpad note saved on this machine only, never
 * written to Azure DevOps. */
function CaseCard({
  c,
  note,
  onSaveNote,
}: {
  c: TestCaseFull;
  note: string;
  onSaveNote: (text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note);

  return (
    <li className="rounded-md border border-border bg-surface">
      <div className="flex flex-wrap items-center gap-2 border-b border-border/60 px-3 py-2">
        <span className="id-mono text-faint">#{c.id}</span>
        <span className="text-sm font-medium text-text">{c.title}</span>
        <span className="ml-auto text-xs text-faint">
          {c.steps.length} steps · {c.automation_status}
          {c.tags && <> · {c.tags}</>}
        </span>
      </div>

      {c.preconditions && (
        <p className="whitespace-pre-wrap border-b border-border/60 px-3 py-2 text-xs text-muted">
          <span className="font-semibold">Preconditions: </span>
          {c.preconditions}
        </p>
      )}

      {c.steps.length > 0 && (
        <table className="w-full border-collapse text-xs">
          <thead>
            <tr className="text-left text-faint">
              <th className="w-8 px-3 py-1 font-medium">#</th>
              <th className="px-3 py-1 font-medium">Action</th>
              <th className="px-3 py-1 font-medium">Expected</th>
            </tr>
          </thead>
          <tbody>
            {c.steps.map((s, i) => (
              <tr key={i} className="border-t border-border/40 align-top">
                <td className="px-3 py-1 text-faint">{i + 1}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-text">{s.action}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-muted">{s.expected}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <div className="border-t border-border/60 px-3 py-2">
        {editing ? (
          <div className="space-y-2">
            <Textarea
              aria-label={`Comment for #${c.id}`}
              className="h-20 w-full"
              placeholder="e.g. Step 3 needs the new confirmation dialog"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                onClick={() => {
                  onSaveNote(draft);
                  setEditing(false);
                }}
              >
                Save comment
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setDraft(note);
                  setEditing(false);
                }}
              >
                Cancel
              </Button>
              <span className="text-[11px] text-faint">
                Saved on this device only — never sent to Azure DevOps.
              </span>
            </div>
          </div>
        ) : note ? (
          <div className="flex items-start gap-2 rounded-md bg-accent-soft/60 px-2.5 py-1.5">
            <p className="whitespace-pre-wrap text-xs text-text">{note}</p>
            <button
              className="ml-auto shrink-0 text-xs text-accent hover:underline"
              onClick={() => {
                setDraft(note);
                setEditing(true);
              }}
            >
              Edit
            </button>
          </div>
        ) : (
          <button
            className="flex items-center gap-1.5 rounded px-1 py-0.5 text-xs text-faint transition-colors hover:text-accent"
            onClick={() => {
              setDraft("");
              setEditing(true);
            }}
          >
            <MessageSquarePlus size={13} />
            Add comment
          </button>
        )}
      </div>
    </li>
  );
}

/** The View Test Cases tab: a read-only, everything-visible view of the
 * PBI's cases (the old "View in browser" home), plus personal local
 * comments for tracking needed changes. */
export default function ViewCases({
  org,
  project,
  pbi,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [search, setSearch] = useState("");
  const [notes, setNotes] = useState<Record<string, string>>(() => loadNotes(org));

  const pbiId = pbi?.id ?? null;
  // Same key as Edit Test Cases so tab switches reuse the cached fetch.
  const queryKey = ["pbi-tcs", org, pbiId, prefs.moduleRef, prefs.preconditionsRef];
  const cases = useQuery({
    queryKey,
    queryFn: () =>
      unwrap(commands.pbiTestCasesFull(org, pbiId!, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: Boolean(org && pbiId != null),
    retry: false,
  });

  const list = cases.data ?? [];
  const q = search.trim().toLowerCase();
  const visible = useMemo(
    () =>
      q
        ? list.filter(
            (c) =>
              c.title.toLowerCase().includes(q) ||
              `#${c.id}`.includes(q) ||
              String(c.id).includes(q) ||
              c.tags.toLowerCase().includes(q),
          )
        : list,
    [list, q],
  );

  const viewHtml = useMutation({
    mutationFn: () =>
      unwrapStr(
        commands.viewQueueHtml(
          visible.map(toTestCase),
          pbiId != null ? `PBI #${pbiId}` : "",
        ),
      ),
    onError: (e) => toast.error(`Could not open the report: ${e.message ?? e}`),
  });

  if (!org || !project || !pbi) {
    return (
      <PickPbiEmpty
        message="Pick an organization, project and PBI in the bar above to view its test cases."
        org={org}
        project={project}
        onPickPbi={onPickPbi}
      />
    );
  }

  return (
    <section className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-semibold text-muted">
          {q ? `${visible.length} of ${list.length} Test Cases` : `${list.length} Total Test Cases`}
        </h2>
        <button
          aria-label="Refresh"
          title="Refresh"
          className="rounded p-1 text-muted hover:text-accent"
          onClick={() => qc.invalidateQueries({ queryKey })}
        >
          <RefreshCw size={14} />
        </button>
        <div className="ml-auto">
          <Button
            variant="outline"
            size="sm"
            disabled={visible.length === 0 || viewHtml.isPending}
            onClick={() => viewHtml.mutate()}
          >
            View in browser
          </Button>
        </div>
      </div>

      {list.length > 0 && (
        <div className="flex gap-2">
          <Input
            aria-label="Search test cases"
            className="w-56 px-2 py-1"
            placeholder="Filter by name or id"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      )}

      {cases.isLoading && <Skeleton className="h-24" />}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
      {cases.data && list.length === 0 && (
        <p className="text-sm text-muted">No test cases here yet.</p>
      )}
      {list.length > 0 && visible.length === 0 && (
        <p className="text-sm text-muted">No test cases match "{search.trim()}".</p>
      )}

      <ul className="space-y-2">
        {visible.map((c) => (
          <CaseCard
            key={c.id}
            c={c}
            note={notes[String(c.id)] ?? ""}
            onSaveNote={(text) => setNotes(saveNote(org, c.id, text))}
          />
        ))}
      </ul>
    </section>
  );
}
