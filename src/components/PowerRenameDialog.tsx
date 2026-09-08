import { useMemo, useState } from "react";
import { toast } from "sonner";
import {
  canApply,
  EMPTY_RULE,
  previewRename,
  rowsToApply,
  type RenameRow,
  type RenameRule,
} from "../lib/powerRename";
import { IconCancel, IconConfirm, IconUndo } from "../lib/actionIcons";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Input } from "./ui/input";
import { Modal } from "./ui/modal";
import { Select } from "./ui/select";

/** What the caller has to be able to do with the renamed titles. Update Test
 * Cases writes them to Azure DevOps; the queue rewrites drafts in memory. The
 * dialog does not care which - it hands over the exact strings it displayed. */
export type RenameTarget = {
  /** Where these live, for the dialog's own wording. */
  label: string;
  cases: { id: number | null; title: string }[];
  /** Titles NOT being renamed that a result could collide with. */
  otherTitles?: string[];
  /** Apply the shown titles. Rejects with a readable message, or resolves
   * with the rows that did NOT get written (empty when all did). */
  apply: (rows: RenameRow[]) => Promise<RenameRow[]>;
  /** Whether a failed write leaves anything to put back. Drafts are in
   * memory, so their undo can never fail; Azure DevOps writes can. */
  undoable: boolean;
};

/**
 * Bulk title rename over the selected cases.
 *
 * The preview is not a prediction of what some other code will do - the
 * strings on screen ARE the payload. `previewRename` computes them once and
 * `apply` receives those rows unchanged, so the two cannot drift.
 */
export default function PowerRenameDialog({
  target,
  onClose,
  onDone,
}: {
  target: RenameTarget;
  onClose: () => void;
  onDone: () => void;
}) {
  const [rule, setRule] = useState<RenameRule>(EMPTY_RULE);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  /** Set after a successful apply, so it can be put straight back. */
  const [undo, setUndo] = useState<RenameRow[] | null>(null);

  const set = <K extends keyof RenameRule>(key: K, value: RenameRule[K]) =>
    setRule((r) => ({ ...r, [key]: value }));

  const preview = useMemo(
    () => previewRename(target.cases, rule, target.otherTitles ?? []),
    [target.cases, target.otherTitles, rule],
  );
  const ready = canApply(preview) && !busy;

  const run = async (rows: RenameRow[], verb: string, keepUndo: boolean) => {
    setBusy(true);
    setProgress({ done: 0, total: rows.length });
    try {
      const failed = await target.apply(rows);
      const done = rows.length - failed.length;
      if (failed.length === 0) {
        toast.success(`${verb} ${done} title${done === 1 ? "" : "s"}.`);
      } else {
        // Named, not counted - the same rule the rest of the app now follows.
        toast.warning(
          `${verb} ${done} of ${rows.length}. Not changed: ${failed
            .map((f) => f.before)
            .join(", ")}`,
          { duration: 20000 },
        );
      }
      // Only what actually got written can be put back.
      const written = rows.filter((r) => !failed.includes(r));
      setUndo(keepUndo && target.undoable && written.length > 0 ? written : null);
      onDone();
      if (!keepUndo) onClose();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const apply = () => run(rowsToApply(preview), "Renamed", true);

  /** Undo is the same write in the other direction - this app issues no
   *  DELETE and cannot roll back a revision, so there is nothing else it
   *  could be. Swapping before and after is the whole implementation. */
  const revert = () => {
    if (!undo) return;
    const back = undo.map((r) => ({ ...r, before: r.after, after: r.before }));
    setUndo(null);
    void run(back, "Put back", false);
  };

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-4 p-5">
      <div className="shrink-0">
        <h2 className="text-sm font-semibold text-text">
          Power Rename {target.cases.length} test case{target.cases.length === 1 ? "" : "s"}
        </h2>
        <p className="mt-1 text-xs text-muted">
          Only titles change. The preview below is exactly what gets saved to {target.label}.
        </p>
      </div>

      {/* ------------------------------------------------------------ rule */}
      <div className="grid shrink-0 gap-3 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-xs text-muted">
          Find
          <Input
            aria-label="Find"
            autoFocus
            value={rule.find}
            onChange={(e) => set("find", e.target.value)}
            placeholder={rule.useRegex ? "^TC-(\\d+) - (.+)$" : "Text to find"}
            className={preview.error ? "border-danger" : undefined}
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted">
          Replace with
          <Input
            aria-label="Replace with"
            value={rule.replace}
            onChange={(e) => set("replace", e.target.value)}
            placeholder={rule.useRegex ? "$2 [$1]" : "Replacement"}
          />
        </label>
      </div>

      {preview.error && (
        <p role="alert" className="shrink-0 text-xs text-danger">
          That pattern is not valid: {preview.error}
        </p>
      )}

      <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted">
        <label className="flex cursor-pointer items-center gap-2">
          <Checkbox checked={rule.useRegex} onCheckedChange={(v) => set("useRegex", v)} />
          Regular expression
        </label>
        <label className="flex cursor-pointer items-center gap-2">
          <Checkbox checked={rule.matchCase} onCheckedChange={(v) => set("matchCase", v)} />
          Match case
        </label>
        <label className="flex cursor-pointer items-center gap-2">
          <Checkbox checked={rule.firstOnly} onCheckedChange={(v) => set("firstOnly", v)} />
          First match only
        </label>
      </div>

      <div className="grid shrink-0 gap-3 sm:grid-cols-4">
        <label className="flex flex-col gap-1 text-xs text-muted">
          Prefix
          <Input
            aria-label="Prefix"
            value={rule.prefix}
            onChange={(e) => set("prefix", e.target.value)}
            placeholder="Smoke: "
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted">
          Suffix
          <Input
            aria-label="Suffix"
            value={rule.suffix}
            onChange={(e) => set("suffix", e.target.value)}
            placeholder=" (v2)"
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-muted">
          Capitalisation
          <Select
            aria-label="Capitalisation"
            value={rule.casing}
            onChange={(e) => set("casing", e.target.value as RenameRule["casing"])}
          >
            <option value="keep">Leave as-is</option>
            <option value="title">Title Case</option>
            <option value="upper">UPPERCASE</option>
            <option value="lower">lowercase</option>
          </Select>
        </label>
        <div className="flex gap-2">
          <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-muted">
            Number from
            <Input
              aria-label="Number from"
              type="number"
              value={rule.numberFrom}
              onChange={(e) => set("numberFrom", Number(e.target.value) || 0)}
            />
          </label>
          <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-muted">
            Digits
            <Input
              aria-label="Number digits"
              type="number"
              min={1}
              max={12}
              value={rule.numberPad}
              onChange={(e) => set("numberPad", Number(e.target.value) || 1)}
            />
          </label>
        </div>
      </div>

      <p className="shrink-0 text-xs text-faint">
        Put <code className="id-mono">{"${n}"}</code> in the replacement, prefix or suffix to number
        the cases in the order shown.
      </p>

      {/* --------------------------------------------------------- preview */}
      <div className="min-h-0 flex-1 overflow-auto rounded-md border border-border">
        <table className="w-full text-left text-xs">
          <thead className="sticky top-0 bg-surface-2 text-faint">
            <tr>
              <th scope="col" className="w-16 px-2 py-1.5 font-medium">
                Case
              </th>
              <th scope="col" className="px-2 py-1.5 font-medium">
                Now
              </th>
              <th scope="col" className="px-2 py-1.5 font-medium">
                After
              </th>
            </tr>
          </thead>
          <tbody>
            {preview.rows.map((row, i) => {
              const s = row.status;
              return (
                <tr
                  key={row.id ?? `draft-${i}`}
                  className="border-t border-border align-top transition-colors hover:bg-surface-2"
                >
                  <td className="id-mono px-2 py-1.5 text-faint">
                    {row.id != null ? `#${row.id}` : "draft"}
                  </td>
                  <td
                    className={`px-2 py-1.5 ${s.kind === "unchanged" ? "text-faint" : "text-muted"}`}
                  >
                    {row.before}
                  </td>
                  <td className="px-2 py-1.5">
                    {s.kind === "unchanged" ? (
                      <span className="text-faint">unchanged</span>
                    ) : (
                      <>
                        <span
                          className={
                            s.kind === "blocked"
                              ? "text-danger line-through"
                              : "font-medium text-text"
                          }
                        >
                          {row.after}
                        </span>
                        {(s.kind === "blocked" || s.kind === "warned") && (
                          <span
                            className={`ml-2 ${s.kind === "blocked" ? "text-danger" : "text-warning"}`}
                          >
                            {s.reason}
                          </span>
                        )}
                      </>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {/* ---------------------------------------------------------- footer */}
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <p className="text-xs text-muted" aria-live="polite">
          {progress
            ? `Saving ${progress.done} of ${progress.total}`
            : preview.blocked > 0
              ? `${preview.blocked} cannot be saved - fix the rule to continue.`
              : `${preview.renamed} will change` +
                (preview.warned > 0 ? `, ${preview.warned} would duplicate another title` : "")}
        </p>
        <div className="ml-auto flex gap-2">
          {undo && (
            <Button variant="ghost" size="sm" disabled={busy} onClick={revert}>
              <IconUndo aria-hidden />
              Undo rename
            </Button>
          )}
          <Button variant="ghost" size="sm" onClick={onClose}>
            <IconCancel aria-hidden />
            {undo ? "Done" : "Cancel"}
          </Button>
          <Button size="sm" disabled={!ready} onClick={apply}>
            <IconConfirm aria-hidden />
            {busy ? "Saving" : `Rename ${preview.renamed}`}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
