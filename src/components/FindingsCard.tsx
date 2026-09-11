// AI Findings: the problems an assistant recorded while reading - a test
// case against its spec, a spec against itself, code against both. Local
// to this app. The developer reads them here, resolves them when acted
// on, dismisses the noise.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "../bindings";
import { IconConfirm, IconFinding, IconRemove } from "../lib/actionIcons";
import { cn } from "../lib/cn";
import { renderMarkdown } from "../lib/markdown";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Switch } from "./ui/switch";

const KIND_LABEL: Record<string, string> = { test_case: "Test case", spec: "Spec", code: "Code" };

export default function FindingsCard({ org = "", project = "" }: { org?: string; project?: string }) {
  const qc = useQueryClient();
  const [showResolved, setShowResolved] = useState(false);
  const findings = useQuery({
    queryKey: ["findings", org, project],
    queryFn: () => commands.listFindings(org, project),
    enabled: Boolean(org && project),
  });
  const refresh = () => qc.invalidateQueries({ queryKey: ["findings", org, project] });
  const setStatus = useMutation({
    mutationFn: async ({ id, status }: { id: string; status: string }) => {
      const r = await commands.setFindingStatus(id, status);
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: refresh,
    onError: (e) => toast.error(`Could not update the finding: ${e.message}`),
  });
  const remove = useMutation({
    mutationFn: async (id: string) => {
      const r = await commands.removeFinding(id);
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: refresh,
    onError: (e) => toast.error(`Could not dismiss the finding: ${e.message}`),
  });

  const all = findings.data ?? [];
  const shown = showResolved ? all : all.filter((f) => f.status === "open");
  const openCount = all.filter((f) => f.status === "open").length;

  return (
    <section data-tour="ai-findings" className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center gap-2">
        <IconFinding aria-hidden className="size-3.5 shrink-0 text-muted" />
        <h2 className="text-sm font-semibold text-text">AI Findings</h2>
        {openCount > 0 && <Badge className="bg-accent-soft text-accent">{openCount} open</Badge>}
        <label className="ml-auto flex items-center gap-2 text-xs text-muted">
          <Switch checked={showResolved} onCheckedChange={setShowResolved} ariaLabel="Show resolved" />
          Show resolved
        </label>
      </div>
      <p className="text-xs text-muted">
        Problems an assistant found while reading a test case, a spec or the code. Kept in this
        app only. Resolve one when you have acted on it. Dismiss one that is wrong.
      </p>
      {shown.length === 0 ? (
        <p className="text-xs text-faint">
          {all.length === 0
            ? "No findings yet. An assistant records one when something it reads is wrong."
            : "Nothing open. Switch on Show resolved to see the rest."}
        </p>
      ) : (
        <ul className="space-y-2">
          {shown.map((f) => (
            <li
              key={f.id}
              className={cn("space-y-1 rounded-md border border-border p-3", f.status === "resolved" && "opacity-70")}
            >
              <div className="flex flex-wrap items-center gap-2">
                <Badge className="bg-surface-2 text-muted">{KIND_LABEL[f.kind] ?? f.kind}</Badge>
                {f.subject && <span className="id-mono text-xs text-muted">{f.subject}</span>}
                <span className="ml-auto text-[11px] text-faint">{f.created_at.slice(0, 10)}</span>
              </div>
              <p className="text-sm font-medium text-text">{f.title}</p>
              {f.detail && (
                <div className="text-xs text-muted" dangerouslySetInnerHTML={{ __html: renderMarkdown(f.detail) }} />
              )}
              <div className="flex gap-2 pt-1">
                {f.status === "open" ? (
                  <Button size="sm" variant="outline" onClick={() => setStatus.mutate({ id: f.id, status: "resolved" })}>
                    <IconConfirm aria-hidden />
                    Resolve
                  </Button>
                ) : (
                  <Button size="sm" variant="outline" onClick={() => setStatus.mutate({ id: f.id, status: "open" })}>
                    Reopen
                  </Button>
                )}
                <Button size="sm" variant="ghost" onClick={() => remove.mutate(f.id)}>
                  <IconRemove aria-hidden />
                  Dismiss
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
