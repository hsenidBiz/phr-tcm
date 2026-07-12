import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands, type WorkItemDetail } from "../bindings";
import { unwrap } from "../lib/ipc";
import { Button } from "./ui/button";
import DateField from "./ui/datefield";
import { Input, Textarea } from "./ui/input";
import { Select } from "./ui/select";
import { Skeleton } from "./ui/skeleton";
import CommentsPanel from "./CommentsPanel";

type Draft = {
  title: string;
  state: string;
  assignedToUnique: string;
  activity: string;
  remaining: string;
  completed: string;
  original: string;
  startDate: string;
  finishDate: string;
  description: string;
};

function toDraft(d: WorkItemDetail): Draft {
  return {
    title: d.title,
    state: d.state,
    assignedToUnique: d.assigned_to_unique,
    activity: d.activity,
    remaining: d.remaining_work?.toString() ?? "",
    completed: d.completed_work?.toString() ?? "",
    original: d.original_estimate?.toString() ?? "",
    startDate: d.start_date.slice(0, 10),
    finishDate: d.finish_date.slice(0, 10),
    description: d.description_text,
  };
}

/** Right-side detail editor: PATCHes only the fields the user changed
 * (v1 optimistic-save rule); ADO rejections surface verbatim. */
export default function WorkItemDrawer({
  org,
  project,
  itemId,
  states,
  onClose,
  onSaved,
}: {
  org: string;
  project: string;
  itemId: number;
  states: string[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const qc = useQueryClient();
  const detail = useQuery({
    queryKey: ["wi-detail", org, project, itemId],
    queryFn: () => unwrap(commands.workItemDetail(org, project, itemId)),
    retry: false,
  });

  const members = useQuery({
    queryKey: ["members", org, project],
    queryFn: () => unwrap(commands.listTeamMembers(org, project)),
    staleTime: 24 * 60 * 60_000, // v1 cached members for 24h
  });

  const activities = useQuery({
    queryKey: ["activities", org, project, detail.data?.work_item_type],
    queryFn: () => unwrap(commands.activityValues(org, project, detail.data!.work_item_type)),
    enabled: Boolean(detail.data),
    staleTime: Infinity,
  });

  const [draft, setDraft] = useState<Draft | null>(null);
  useEffect(() => {
    if (detail.data) setDraft(toDraft(detail.data));
  }, [detail.data]);

  const save = useMutation({
    mutationFn: async () => {
      const d = detail.data!;
      const dr = draft!;
      const orig = toDraft(d);
      const patches: { reference_name: string; value: string }[] = [];
      const push = (ref: string, now: string, before: string) => {
        if (now !== before) patches.push({ reference_name: ref, value: now });
      };
      push("System.Title", dr.title, orig.title);
      push("System.State", dr.state, orig.state);
      push("System.AssignedTo", dr.assignedToUnique, orig.assignedToUnique);
      push("Microsoft.VSTS.Common.Activity", dr.activity, orig.activity);
      push("Microsoft.VSTS.Scheduling.RemainingWork", dr.remaining, orig.remaining);
      push("Microsoft.VSTS.Scheduling.CompletedWork", dr.completed, orig.completed);
      push("Microsoft.VSTS.Scheduling.OriginalEstimate", dr.original, orig.original);
      push("Microsoft.VSTS.Scheduling.StartDate", dr.startDate, orig.startDate);
      push("Microsoft.VSTS.Scheduling.FinishDate", dr.finishDate, orig.finishDate);
      if (dr.description !== orig.description) {
        patches.push({
          reference_name: d.description_field,
          value: `<div>${dr.description.replace(/\n/g, "<br>")}</div>`,
        });
      }
      if (patches.length === 0) return false;
      await unwrap(commands.updateWorkItem(org, project, itemId, patches));
      return true;
    },
    onSuccess: (changed) => {
      if (changed) {
        toast.success(`Saved #${itemId}`);
        qc.invalidateQueries({ queryKey: ["wi-detail", org, project, itemId] });
        onSaved();
      } else {
        toast.info("No changes to save.");
      }
    },
    onError: (e) => toast.error(`Save failed: ${e.message}`),
  });

  return (
    <aside className="drawer-in flex h-full w-96 shrink-0 flex-col border-l border-border bg-surface">
      <header className="flex items-center justify-between border-b border-border px-4 py-3">
        <span className="text-sm font-semibold text-text">
          <span className="id-mono text-faint">#{itemId}</span>{" "}
          {detail.data?.work_item_type}
        </span>
        <button aria-label="Close details" className="text-muted hover:text-text" onClick={onClose}>
          <X size={16} />
        </button>
      </header>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
        {detail.isLoading && <Skeleton className="h-40" />}
        {detail.isError && <p className="text-sm text-danger">{detail.error.message}</p>}

        {detail.data && draft && (
          <>
            <label className="block text-xs text-muted">
              Title
              <Input
                className="mt-1 w-full"
                value={draft.title}
                onChange={(e) => setDraft({ ...draft, title: e.target.value })}
              />
            </label>

            <div className="grid grid-cols-2 gap-2">
              <label className="block text-xs text-muted">
                State
                <Select
                  className="mt-1 w-full"
                  value={draft.state}
                  onChange={(e) => setDraft({ ...draft, state: e.target.value })}
                >
                  {!states.includes(draft.state) && <option>{draft.state}</option>}
                  {states.map((s) => (
                    <option key={s}>{s}</option>
                  ))}
                </Select>
              </label>
              <label className="block text-xs text-muted">
                Assigned to
                <Select
                  className="mt-1 w-full"
                  value={draft.assignedToUnique}
                  onChange={(e) => setDraft({ ...draft, assignedToUnique: e.target.value })}
                >
                  <option value="">(unassigned)</option>
                  {detail.data.assigned_to_unique &&
                    !(members.data ?? []).some(
                      (m) => m.unique_name === detail.data!.assigned_to_unique,
                    ) && (
                      <option value={detail.data.assigned_to_unique}>
                        {detail.data.assigned_to}
                      </option>
                    )}
                  {(members.data ?? []).map((m) => (
                    <option key={m.unique_name} value={m.unique_name}>
                      {m.display_name}
                    </option>
                  ))}
                </Select>
              </label>
            </div>

            {(activities.data?.length ?? 0) > 0 && (
              <label className="block text-xs text-muted">
                Activity
                <Select
                  className="mt-1 w-full"
                  value={draft.activity}
                  onChange={(e) => setDraft({ ...draft, activity: e.target.value })}
                >
                  <option value="">(none)</option>
                  {activities.data!.map((a) => (
                    <option key={a}>{a}</option>
                  ))}
                </Select>
              </label>
            )}

            <div className="grid grid-cols-3 gap-2">
              {(
                [
                  ["Remaining", "remaining"],
                  ["Completed", "completed"],
                  ["Original", "original"],
                ] as const
              ).map(([label, key]) => (
                <label key={key} className="block text-xs text-muted">
                  {label}
                  <Input
                    className="mt-1 w-full px-2"
                    type="number"
                    min="0"
                    step="0.5"
                    value={draft[key]}
                    onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
                  />
                </label>
              ))}
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div className="text-xs text-muted">
                Start date
                <DateField
                  className="mt-1"
                  ariaLabel="Start date"
                  value={draft.startDate}
                  onChange={(v) => setDraft({ ...draft, startDate: v })}
                />
              </div>
              <div className="text-xs text-muted">
                Finish date
                <DateField
                  className="mt-1"
                  ariaLabel="Finish date"
                  value={draft.finishDate}
                  onChange={(v) => setDraft({ ...draft, finishDate: v })}
                />
              </div>
            </div>

            <label className="block text-xs text-muted">
              Description
              <Textarea
                className="mt-1 h-28 w-full"
                value={draft.description}
                onChange={(e) => setDraft({ ...draft, description: e.target.value })}
              />
            </label>

            <div className="text-xs text-faint">
              {detail.data.area_path} · {detail.data.iteration_path}
            </div>

            <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
              {save.isPending ? "Saving" : "Save changes"}
            </Button>

            <CommentsPanel org={org} project={project} itemId={itemId} />
          </>
        )}
      </div>
    </aside>
  );
}
