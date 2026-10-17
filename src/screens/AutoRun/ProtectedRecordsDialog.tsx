// The project's protected baseline: the records its tests must never edit
// or delete (a goal group, a rating method, a published cycle...).
//
// Kept per project in its Auto Run settings file, beside the save words,
// and saved as one list through `auto_run_set_protected` (Rust trims the
// names and drops blank and repeated ones). An export hands the list to the
// Playwright repo as docs/tcm-protected-baseline.md, and each case it
// writes points at that file. The usual PMS records are only OFFERED here;
// nothing is written until the person presses Save.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type NavView, type ProtectedKind, type ProtectedRecord } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconAdd, IconCancel, IconConfirm, IconRemove } from "../../lib/actionIcons";
import { toast } from "../../lib/toast";

/** The kinds, in the order the repo's baseline file lists them. */
const KINDS: { value: ProtectedKind; label: string }[] = [
  { value: "goal_group", label: "Goal group" },
  { value: "rating_method", label: "Rating method" },
  { value: "proficiency_scale", label: "Proficiency scale" },
  { value: "competency_group", label: "Competency group" },
  { value: "cycle", label: "Cycle" },
  { value: "other", label: "Other" },
];

/** The records a PMS project usually keeps fixed. Offered, never written
 * on their own. */
export const USUAL_PMS_RECORDS: ProtectedRecord[] = [
  { kind: "goal_group", name: "Customer Focus" },
  { kind: "goal_group", name: "Operational Excellence" },
  { kind: "rating_method", name: "5-Point Scale" },
  { kind: "rating_method", name: "Four Level Scale" },
  { kind: "proficiency_scale", name: "6-level proficiency scale" },
  { kind: "competency_group", name: "Core Competencies" },
  { kind: "competency_group", name: "Technical Competencies" },
  { kind: "cycle", name: "FY2026 (published)" },
  { kind: "cycle", name: "FY2025 (unpublished)" },
];

const same = (a: ProtectedRecord, b: ProtectedRecord) =>
  a.kind === b.kind && a.name.trim().toLowerCase() === b.name.trim().toLowerCase();

export default function ProtectedRecordsDialog({
  org,
  project,
  view,
  onClose,
}: {
  org: string;
  project: string;
  /** The project's settings as last read; `null` when it has none yet. */
  view: NavView | null;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [records, setRecords] = useState<ProtectedRecord[]>(view?.protected ?? []);
  const [problem, setProblem] = useState("");
  const headingId = useId();

  const change = (at: number, patch: Partial<ProtectedRecord>) =>
    setRecords(records.map((r, i) => (i === at ? { ...r, ...patch } : r)));

  const addUsual = () =>
    setRecords([...records, ...USUAL_PMS_RECORDS.filter((u) => !records.some((r) => same(r, u)))]);

  const save = useMutation({
    mutationFn: async (list: ProtectedRecord[]) => {
      const res = await commands.autoRunSetProtected(org, project, list);
      if (res.status === "error") throw new Error(res.error);
      return res.data;
    },
    onSuccess: (next) => {
      qc.setQueryData(["autorun-nav", org, project], next);
      toast.success("Protected records saved.");
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  return (
    <Modal onClose={onClose} labelledBy={headingId} className="flex w-full max-w-2xl flex-col gap-3 p-5">
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Protected records
      </h2>
      <p className="text-xs text-muted">
        Records the project&apos;s tests must never edit or delete. An export to Playwright writes them to
        docs/tcm-protected-baseline.md in the repository, and each case it writes points there.
      </p>
      {records.length === 0 ? (
        <p className="text-xs text-faint">None yet.</p>
      ) : (
        <ul aria-label="Protected records" className="max-h-72 space-y-2 overflow-y-auto">
          {records.map((r, i) => (
            <li key={i} className="flex items-center gap-2">
              <Select
                aria-label={`Kind of record ${i + 1}`}
                className="w-48 shrink-0"
                value={r.kind}
                onChange={(e) => change(i, { kind: e.target.value as ProtectedKind })}
              >
                {KINDS.map((k) => (
                  <option key={k.value} value={k.value}>
                    {k.label}
                  </option>
                ))}
              </Select>
              <Input
                aria-label={`Name of record ${i + 1}`}
                className="flex-1"
                placeholder="Name, as the application shows it"
                value={r.name}
                onChange={(e) => change(i, { name: e.target.value })}
              />
              <Button
                size="sm"
                variant="ghost"
                aria-label={`Remove record ${i + 1}`}
                onClick={() => setRecords(records.filter((_, j) => j !== i))}
              >
                <IconRemove aria-hidden />
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" onClick={() => setRecords([...records, { kind: "goal_group", name: "" }])}>
          <IconAdd aria-hidden />
          Add a record
        </Button>
        <Button size="sm" variant="outline" onClick={addUsual}>
          <IconAdd aria-hidden />
          Add the usual PMS records
        </Button>
      </div>
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          disabled={save.isPending}
          onClick={() => {
            setProblem("");
            save.mutate(records);
          }}
        >
          <IconConfirm aria-hidden />
          {save.isPending ? "Saving" : "Save"}
        </Button>
      </div>
    </Modal>
  );
}
