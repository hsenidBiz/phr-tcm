// Find and replace on the open report. Find lists where the text is, in the
// report and in its subreports. Replace all first asks the report what it
// would change (a dry run), shows the count, and only saves when the person
// says yes: a saved replace is an ordinary edit with no undo of its own.

import { useState } from "react";
import { commands, type FindHit, type ReplaceOutcome } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { IconSearch } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

function skippedNote(n: number): string {
  if (n === 0) return "";
  return n === 1
    ? " 1 skipped: it is a field or text that cannot be edited."
    : ` ${n} skipped: they are fields or text that cannot be edited.`;
}

export function FindReplace({ onChanged }: { onChanged: () => void }) {
  const [find, setFind] = useState("");
  const [replace, setReplace] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const [hits, setHits] = useState<FindHit[] | null>(null);
  // What the dry run showed and the exact terms it was run with: "Yes" applies
  // these, and any change to the inputs below clears it.
  const [pending, setPending] = useState<{
    out: ReplaceOutcome;
    find: string;
    replace: string;
    matchCase: boolean;
  } | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = () => {
    setError(null);
    setDone(null);
    setPending(null);
    setBusy(true);
  };

  const runFind = async () => {
    start();
    try {
      setHits(await unwrapStr(commands.crystalFind(find, matchCase)));
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  // The dry run: what would change, and nothing saved.
  const review = async () => {
    start();
    try {
      const out = await unwrapStr(commands.crystalReplace(find, replace, matchCase, true, null));
      if (out.replaced === 0) {
        setDone(`Nothing to replace.${skippedNote(out.skipped_locked)}`);
      } else {
        setPending({ out, find, replace, matchCase });
      }
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!pending) return;
    setError(null);
    setBusy(true);
    try {
      const out = await unwrapStr(
        commands.crystalReplace(pending.find, pending.replace, pending.matchCase, false, pending.out.replaced),
      );
      setPending(null);
      setHits(null);
      setDone(`Replaced ${out.replaced}.${skippedNote(out.skipped_locked)}`);
      onChanged();
    } catch (e) {
      setPending(null);
      setError(message(e));
      // A refusal reads the file again, so what is on screen may be out of date.
      onChanged();
    } finally {
      setBusy(false);
    }
  };

  const edited = (set: (v: string) => void) => (e: { target: { value: string } }) => {
    set(e.target.value);
    setPending(null);
  };

  const empty = find === "";
  return (
    <section aria-label="Find and replace" className="shrink-0 rounded-lg border border-border bg-surface px-4 py-3">
      <h2 className="mb-2 text-sm font-semibold text-text">Find and replace</h2>
      <div className="grid gap-2 sm:grid-cols-2">
        <Input aria-label="Find" className="py-1.5" value={find} placeholder="Find" onChange={edited(setFind)} />
        <Input
          aria-label="Replace with"
          className="py-1.5"
          value={replace}
          placeholder="Replace with"
          onChange={edited(setReplace)}
        />
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-xs text-text">
          <Checkbox
            checked={matchCase}
            onCheckedChange={(v) => {
              setMatchCase(v);
              setPending(null);
            }}
            ariaLabel="Match case"
          />
          Match case
        </label>
        <Button size="sm" variant="outline" disabled={empty || busy} onClick={() => void runFind()}>
          <IconSearch aria-hidden />
          Find
        </Button>
        <Button size="sm" variant="outline" disabled={empty || busy} onClick={() => void review()}>
          Replace all
        </Button>
      </div>
      <div role="status" className="empty:hidden mt-2 flex flex-col gap-1 text-xs text-text">
        {pending && (
          <p>
            Replace {plural(pending.out.replaced, "text object", "text objects")}?
            {pending.out.changes.some((c) => c.subreport) && " Some are in subreports."} This is saved at once.
            {skippedNote(pending.out.skipped_locked)}
          </p>
        )}
        {done && <p>{done}</p>}
        {hits && (
          <p className="text-muted">{hits.length === 0 ? "No matches." : plural(hits.length, "match", "matches")}</p>
        )}
      </div>
      {pending && (
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={busy} onClick={() => void apply()}>
            Yes, replace
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => setPending(null)}>
            Cancel
          </Button>
        </div>
      )}
      {error && (
        <p role="alert" className="mt-2 text-xs text-danger">
          {error}
        </p>
      )}
      {hits && (
        <div className="mt-2">
          <ul className="mt-1 flex max-h-48 flex-col gap-1 overflow-y-auto">
            {hits.map((h) => (
              <li
                key={`${h.subreport ?? ""}/${h.object_id}`}
                className="flex min-w-0 flex-wrap items-center gap-x-2 text-xs text-text"
              >
                <span className="font-medium">{h.name || h.kind}</span>
                <span className="text-muted">{h.section}</span>
                {h.subreport && <span className="text-muted">Subreport: {h.subreport}</span>}
                <span className="min-w-0 truncate">{h.text}</span>
                {!h.replaceable && (
                  <Badge>{h.matched === "field" ? "A field, cannot be replaced" : "Text cannot be edited"}</Badge>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
