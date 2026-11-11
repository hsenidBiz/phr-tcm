// Waiting for you: what the assistants left for a person to decide, at the
// top of Auto Run. A proposed account or sign-in, or a divergence that needs
// a ruling. Each row opens the place the decision is made. Nothing is drawn
// while nothing waits.

import { useQuery } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { commands, type WaitingItem } from "../../bindings";
import { IconAccounts, IconGoTo, IconSignIn, IconWarning } from "../../lib/actionIcons";
import { Button } from "../../components/ui/button";
import { clearAutoRunFocus, focusOf, requestAutoRunFocus, useAutoRunFocus } from "../../lib/autoRunFocus";
import { unwrapStr } from "../../lib/ipc";
import { timeAgo } from "../../lib/timeAgo";
import { useWaitingItems } from "../../lib/useWaitingItems";
import AccountsDialog from "./AccountsDialog";
import SignInProposalNotice from "./SignInProposalNotice";

const rowKey = (item: WaitingItem) => `${item.target.kind}:${item.key}`;

const ICONS = {
  accounts: IconAccounts,
  sign_in: IconSignIn,
  divergence: IconWarning,
} as const;

export default function WaitingForYou({
  org,
  project,
  pbiId,
  cases,
  onOpenPbi,
}: {
  org: string;
  /** The project Auto Run's files belong to. */
  project: string;
  /** The PBI on screen, if any. */
  pbiId: number | null;
  /** The ids of the cases of the PBI on screen. */
  cases: readonly number[];
  /** Switch to another PBI, for a divergence that belongs to it. */
  onOpenPbi?: (pbiId: number) => void;
}) {
  const headingId = useId();
  const waiting = useWaitingItems(org, project, Boolean(org && project));
  const [accountsOpen, setAccountsOpen] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);

  const divergences = useQuery({
    queryKey: ["autorun-divergences", org, project],
    queryFn: () => unwrapStr(commands.autoRunLoadDivergences(org, project)),
    enabled: Boolean(org && project && expanded?.startsWith("divergence:")),
    retry: false,
  });

  // A bell notice for the accounts or the sign-in lands here: show it.
  const focus = useAutoRunFocus();
  useEffect(() => {
    if (!focus) return;
    if (focus.kind === "accounts") {
      setAccountsOpen(true);
      clearAutoRunFocus();
    } else if (focus.kind === "sign-in") {
      const row = waiting?.items.find((i) => i.target.kind === "sign_in");
      setExpanded(row ? rowKey(row) : null);
      clearAutoRunFocus();
    }
  }, [focus, waiting]);

  const items = waiting?.items ?? [];
  if (items.length === 0 && !accountsOpen) return null;

  const open = (item: WaitingItem) => {
    const t = item.target;
    if (t.kind === "accounts") {
      setAccountsOpen(true);
    } else if (t.kind === "sign_in") {
      setExpanded((cur) => (cur === rowKey(item) ? null : rowKey(item)));
    } else {
      if (t.pbi != null && t.pbi !== pbiId) onOpenPbi?.(t.pbi);
      if (t.pbi != null || cases.includes(t.case_id)) {
        requestAutoRunFocus(focusOf(t));
      } else {
        setExpanded((cur) => (cur === rowKey(item) ? null : rowKey(item)));
      }
    }
  };

  return (
    <>
      {items.length > 0 && (
        <section aria-labelledby={headingId} className="space-y-2 rounded-md border border-border bg-surface p-3">
          <h2 id={headingId} className="text-sm font-semibold text-text">
            Waiting for you
          </h2>
          <ul className="space-y-2">
            {items.map((item) => {
              const Icon = ICONS[item.target.kind];
              const key = rowKey(item);
              const t = item.target;
              return (
                <li key={key} className="space-y-2">
                  <div className="flex items-center gap-2 text-sm">
                    <Icon aria-hidden className="size-4 shrink-0 text-muted" />
                    <span className="min-w-0 flex-1 text-text">{item.title}</span>
                    {item.proposed_ms ? (
                      <span className="shrink-0 text-xs text-muted">
                        {timeAgo(new Date(item.proposed_ms).toISOString())}
                      </span>
                    ) : null}
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Open: ${item.title}`}
                      aria-expanded={t.kind === "sign_in" ? expanded === key : undefined}
                      onClick={() => open(item)}
                    >
                      <IconGoTo aria-hidden />
                      Open
                    </Button>
                  </div>
                  {expanded === key && t.kind === "sign_in" && <SignInProposalNotice org={org} project={project} />}
                  {expanded === key && t.kind === "divergence" && (
                    <div className="space-y-1 rounded-md border border-border bg-surface-2 p-2 text-xs text-text">
                      <p>Step {t.step}</p>
                      {(() => {
                        const d = divergences.data?.find((x) => x.id === t.id);
                        return d ? (
                          <>
                            <p>
                              <span className="text-muted">The case says: </span>
                              {d.case_says}
                            </p>
                            <p>
                              <span className="text-muted">The app shows: </span>
                              {d.app_shows}
                            </p>
                          </>
                        ) : null;
                      })()}
                      <p className="text-muted">This case is not under the selected PBI.</p>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      )}
      {accountsOpen && (
        <AccountsDialog org={org} project={project} initialTab="proposed" onClose={() => setAccountsOpen(false)} />
      )}
    </>
  );
}
