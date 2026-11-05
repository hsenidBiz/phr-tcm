// The cases that both passed and failed in their recent runs on one environment, read from the run files on
// this machine. Quiet: nothing at all when no case is flaky. Each row shows how the case went run by run
// (oldest first) and the last failure; the person can mark it Flaky, which makes Auto Run count its failures
// apart from the ones that make a run read as failed. Nothing sent to Azure DevOps changes.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type FlakyCase } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconHideDetails, IconMarkFlaky, IconShowDetails } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { FLAKY_CASES_KEY, useSetFlaky } from "./flaky";

/** The run-by-run dots, oldest first: success for a pass, danger for a fail. */
function Pattern({ pattern }: { pattern: boolean[] }) {
  const words = pattern.map((p) => (p ? "passed" : "failed")).join(", ");
  return (
    <span role="img" aria-label={`Recent runs, oldest first: ${words}`} className="inline-flex shrink-0 items-center gap-0.5">
      {pattern.map((passed, i) => (
        <span key={i} aria-hidden className={cn("size-2 rounded-full", passed ? "bg-success" : "bg-danger")} />
      ))}
    </span>
  );
}

export default function FlakyCases() {
  const [open, setOpen] = useState(true);
  const setFlaky = useSetFlaky();
  const flaky = useQuery({
    queryKey: FLAKY_CASES_KEY,
    queryFn: () => commands.autoRunFlakyCases(),
    retry: false,
  });
  const cases: FlakyCase[] = flaky.data ?? [];
  if (cases.length === 0) return null;
  const marked = cases.filter((c) => c.marked).length;

  return (
    <section aria-label="Flaky cases" className="space-y-1 rounded-md border border-border bg-surface p-2">
      <Button
        size="sm"
        variant="ghost"
        aria-expanded={open}
        className="w-full justify-start px-1 text-sm font-semibold text-text"
        onClick={() => setOpen((o) => !o)}
      >
        {open ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
        <span>Flaky cases</span>
        <span className="text-xs font-normal text-muted">
          {cases.length} passed and failed recently{marked > 0 ? `, ${marked} marked` : ""}
        </span>
      </Button>
      {open && (
        <>
          <p className="text-xs text-faint">
            These cases passed in some recent runs on an environment and failed in others. Marking one as
            flaky counts its failures apart from real failures in Auto Run. A failure is still sent to Azure
            DevOps as failed.
          </p>
          <ul className="space-y-1">
            {cases.map((c) => (
              <li
                key={`${c.environment ?? ""}-${c.case_id}`}
                aria-label={`Flaky case ${c.title}`}
                className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md border border-border/60 px-3 py-2 text-sm"
              >
                <span className="id-mono text-faint">#{c.case_id}</span>
                <span className="min-w-0 flex-1 basis-48 truncate text-text" title={c.title}>
                  {c.title}
                </span>
                {c.environment && <span className="text-xs text-muted">{c.environment}</span>}
                <Pattern pattern={c.pattern} />
                {c.last_failure && (
                  <span className="min-w-0 max-w-xs truncate text-xs text-muted" title={c.last_failure}>
                    {c.last_failure}
                  </span>
                )}
                <Button
                  size="sm"
                  variant={c.marked ? "default" : "outline"}
                  aria-label={`${c.marked ? "Unmark" : "Mark"} case ${c.case_id} as flaky`}
                  disabled={setFlaky.isPending}
                  onClick={() => setFlaky.mutate({ caseId: c.case_id, flaky: !c.marked })}
                >
                  <IconMarkFlaky aria-hidden />
                  {c.marked ? "Flaky" : "Mark flaky"}
                </Button>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
