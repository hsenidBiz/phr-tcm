import { useEffect, useMemo, useState } from "react";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconConfirm, IconImport, IconMoveDown, IconMoveUp } from "../../lib/actionIcons";
import { moveItem, orderFromFiles, type FileForOrder, type SuiteCase } from "../../lib/suiteOrder";

export type OrderFile = FileForOrder & { path: string };

/** Arrange several draft files into the order their blocks should take in
 * the suite. Each file is one block in the file's ROW order; blocks follow
 * this list; whatever is in no file trails behind. The dialog only
 * proposes: Apply arranges the list on screen, and Apply order saves. */
export default function FileOrderDialog({
  suiteCases,
  files,
  onAddFiles,
  onClose,
  onApply,
}: {
  suiteCases: SuiteCase[];
  files: OrderFile[];
  onAddFiles: () => void;
  onClose: () => void;
  onApply: (order: SuiteCase[], placedTotal: number) => void;
}) {
  // The parent appends when the picker adds files; this keeps the user's
  // arrangement of the ones already here and tacks the new ones on the end.
  const [arranged, setArranged] = useState<OrderFile[]>(files);
  useEffect(() => {
    setArranged((cur) => {
      const known = new Set(cur.map((f) => f.path));
      return [...cur.filter((f) => files.some((n) => n.path === f.path)), ...files.filter((f) => !known.has(f.path))];
    });
  }, [files]);

  const result = useMemo(() => orderFromFiles(suiteCases, arranged), [suiteCases, arranged]);
  const placedTotal = result.placed.reduce((a, b) => a + b, 0);

  return (
    <Modal onClose={onClose} className="w-[560px] max-w-full p-4">
      <h2 className="text-sm font-semibold text-text">Apply order from files</h2>
      <p className="mt-1 text-xs text-muted">
        Each file becomes a block of cases in the order they appear in that file. Put the files in the order
        the blocks should take; anything in no file stays after them in its current order.
      </p>
      <ol className="mt-3 divide-y divide-border rounded-md border border-border">
        {arranged.map((f, i) => {
          const n = result.placed[i];
          return (
            <li key={f.path} className="flex items-center gap-3 px-3 py-2 text-sm">
              <span className="id-mono w-6 shrink-0 text-right text-faint">{i + 1}</span>
              <span className="min-w-0 flex-1 truncate text-text" title={f.path}>
                {f.name}
              </span>
              <span className={n === 0 ? "text-xs text-warning" : "text-xs text-muted"}>
                {n === 0 ? "places nothing from this suite" : `places ${n} of ${suiteCases.length}`}
              </span>
              <span className="flex shrink-0 items-center gap-1">
                <button type="button" aria-label={`Move ${f.name} up`} title="Move up" disabled={i === 0}
                  className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                  onClick={() => setArranged((a) => moveItem(a, i, i - 1))}>
                  <IconMoveUp aria-hidden />
                </button>
                <button type="button" aria-label={`Move ${f.name} down`} title="Move down" disabled={i === arranged.length - 1}
                  className="rounded p-1 text-muted hover:text-accent disabled:opacity-30 [&_svg]:size-3.5"
                  onClick={() => setArranged((a) => moveItem(a, i, i + 1))}>
                  <IconMoveDown aria-hidden />
                </button>
              </span>
            </li>
          );
        })}
      </ol>
      {result.duplicates > 0 && (
        <p className="mt-2 text-xs text-warning">
          {result.duplicates === 1
            ? "1 test case is named in more than one file; the first file keeps it."
            : `${result.duplicates} test cases are named in more than one file; the first file keeps them.`}
        </p>
      )}
      <div className="mt-4 flex items-center gap-2">
        <Button size="sm" variant="ghost" onClick={onAddFiles}>
          <IconImport aria-hidden />
          Add more files
        </Button>
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={placedTotal === 0} onClick={() => onApply(result.order, placedTotal)}>
          <IconConfirm aria-hidden />
          Apply
        </Button>
      </div>
    </Modal>
  );
}
