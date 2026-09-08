import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { commands } from "../bindings";
import { fileName, type WatchedFile } from "../lib/fileSync";
import { cn } from "../lib/cn";

/** Autosave delay. Long enough that a sentence isn't written to disk a
 * character at a time, short enough that closing the app right after
 * typing doesn't lose it. Matches the report page's boxes. */
const DEBOUNCE = 600;

type SaveState = "idle" | "saving" | "saved" | "failed";

/**
 * Notes about a whole imported set, one box per JSON file.
 *
 * These live in the file's own top-level `comments`, not in the app, so
 * they travel with it: send the file on, or let an assistant read it, and
 * the reasoning is right there beside the cases. The same boxes appear in
 * the browser view - this is the copy for people who never open it.
 *
 * Collapsed by default. It is reference material, not something to be read
 * on the way past.
 */
export default function GeneralComments({
  watches,
  onSaved,
}: {
  watches: WatchedFile[];
  /** The file's new fingerprint, so the caller's watch entry can move
   * forward instead of re-reading a file it just wrote. */
  onSaved: (path: string, text: string, stamp: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [state, setState] = useState<Record<string, SaveState>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  // Local while typing: the parent's copy only moves on a successful save,
  // so a slow disk can't yank characters back out from under the cursor.
  const [draft, setDraft] = useState<Record<string, string>>({});
  const timers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const t of Object.values(pending)) clearTimeout(t);
    };
  }, []);

  if (watches.length === 0) return null;

  const save = (path: string, text: string) => {
    setDraft((d) => ({ ...d, [path]: text }));
    setState((s) => ({ ...s, [path]: "saving" }));
    clearTimeout(timers.current[path]);
    timers.current[path] = setTimeout(() => {
      const failed = (why: string) => {
        setState((s) => ({ ...s, [path]: "failed" }));
        setErrors((e) => ({ ...e, [path]: why }));
      };
      void commands
        .saveGeneralComment(path, text)
        .then((r) => {
          if (r.status === "error") return failed(r.error);
          setState((s) => ({ ...s, [path]: "saved" }));
          onSaved(path, text, r.data);
        })
        // The bridge itself can fail (the app shutting down mid-save).
        // Leaving the box on "Saving…" forever is the one outcome that
        // would let someone believe their note was written.
        .catch((e) => failed(e instanceof Error ? e.message : String(e)));
    }, DEBOUNCE);
  };

  const label = (path: string) => {
    switch (state[path]) {
      case "saving":
        return <span className="text-faint">Saving…</span>;
      case "saved":
        return <span className="text-accent">Saved ✓</span>;
      case "failed":
        return <span className="text-danger">Not saved — {errors[path]}</span>;
      default:
        return null;
    }
  };

  return (
    <div className="rounded-md border border-border/60 bg-surface-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={cn(
          "flex w-full items-center gap-1.5 px-2.5 py-1.5 text-xs font-semibold text-text",
          "hover:text-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
        )}
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0" aria-hidden />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden />
        )}
        General comments
        <span className="font-normal text-faint">
          — notes about the set, saved into the JSON
        </span>
      </button>
      {open && (
        <div className="space-y-3 px-2.5 pb-2.5">
          {watches.map((w) => (
            <div key={w.path} className="space-y-1">
              <div className="flex items-baseline gap-2 text-xs">
                <span className="id-mono min-w-0 flex-1 truncate text-muted" title={w.path}>
                  {fileName(w.path)}
                </span>
                {label(w.path)}
              </div>
              <textarea
                aria-label={`General comments for ${fileName(w.path)}`}
                rows={3}
                className={cn(
                  "w-full resize-y rounded-md border border-border bg-surface px-2 py-1.5 text-xs text-text",
                  "placeholder:text-faint focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
                )}
                placeholder="e.g. Spec 3.2 is ambiguous about the timeout — asked Dev, waiting"
                value={draft[w.path] ?? w.comment ?? ""}
                onChange={(e) => save(w.path, e.target.value)}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
