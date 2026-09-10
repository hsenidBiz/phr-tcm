// The Work Manager's Pull Requests panel: three read-only groups ordered
// by actionability - awaiting your review, yours, then everything active
// on a chosen repo. Rows open the PR in the browser; voting/completing
// stays in Azure DevOps (this panel never writes).

import {
  useInfiniteQuery,
  useIsFetching,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Bug,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  GitBranch,
  RefreshCw,
  Rocket,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { commands, type PrBuild, type PullRequest, type PrWorkItem } from "../bindings";
import PipelineDialog, { duration, failurePath, label, tone } from "../components/PipelineDialog";
import PrThreads, { isResolved } from "../components/PrThreads";
import MultiSelect from "../components/ui/multiselect";
import { Modal } from "../components/ui/modal";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import { cacheRead, cacheWrite } from "../lib/localCache";
import AstryxIsland from "../components/AstryxIsland";
import { Markdown } from "@astryxdesign/core/Markdown";

/** Mirrors AdoClient::PR_PAGE_SIZE - a full page implies a next page. */
const PR_PAGE = 25;

/** ADO reviewer votes: 10 approved, 5 approved w/ suggestions, 0 waiting,
 * -5 waiting for author, -10 rejected. */
function voteDot(vote: number): { cls: string; label: string } {
  if (vote >= 5) return { cls: "bg-success", label: "approved" };
  if (vote === -5) return { cls: "bg-warning", label: "waiting for author" };
  if (vote <= -10) return { cls: "bg-danger", label: "rejected" };
  return { cls: "bg-border-strong", label: "no vote yet" };
}

/** Work-item type colour, matching the board's swatches. */
const wiTypeColor: Record<string, string> = {
  Bug: "#e15b64",
  Task: "#d99e2b",
  "Product Backlog Item": "#2aa5e0",
  "User Story": "#2aa5e0",
  Feature: "#9a74d8",
  Epic: "#e0873c",
};

/** One linked work item as a DevOps-style chip: type-coloured icon, id,
 * title, and a state dot. Opens the work item in the browser. */
function WorkItemChip({ wi }: { wi: PrWorkItem }) {
  const color = wiTypeColor[wi.work_item_type] ?? "#9ca3af";
  return (
    // items-start, not center: the title wraps to as many lines as it
    // needs (the side column is 16rem, and "Participants - ..." told the
    // reader nothing), while the icon, id and state pin to the first line
    // like a DevOps chip. mt-px keeps the small marks on the text's
    // baseline once the row is taller than one line.
    <button
      className="flex w-full items-start gap-2 rounded-md border border-border bg-surface px-2.5 py-1.5 text-left transition-colors hover:border-border-strong hover:bg-surface-2"
      title={`Open ${wi.work_item_type} ${wi.id} in Azure DevOps`}
      onClick={() => openUrl(wi.url).catch(() => toast.error("Could not open the browser."))}
    >
      {wi.work_item_type === "Bug" ? (
        <Bug size={13} className="mt-px shrink-0" style={{ color }} />
      ) : (
        <span
          className="mt-1 inline-block h-2.5 w-2.5 shrink-0 rounded-sm"
          style={{ backgroundColor: color }}
        />
      )}
      <span className="id-mono shrink-0 text-faint">{wi.id}</span>
      <span className="min-w-0 flex-1 break-words text-text">{wi.title}</span>
      <span className="ml-auto flex shrink-0 items-center gap-1 text-muted">
        <span
          className="inline-block h-2 w-2 rounded-full"
          style={{ backgroundColor: wi.state_color ? `#${wi.state_color}` : "#9ca3af" }}
        />
        {wi.state}
      </span>
    </button>
  );
}

/** Azure DevOps truncates `description` in the pull request LIST response
 * around 400 characters, mid-word, with nothing marking it - so "was this
 * cut?" can't be answered from the API alone. 380 gives a small margin
 * below the documented-nowhere cutoff, catching anything close enough that
 * it was almost certainly cut mid-word. */
const DESCRIPTION_TRUNCATION_CAP = 380;

/** Same Markdown, same props, wherever a PR description renders - this is
 * the security-relevant bit (Astryx renders remote-authored text as React,
 * never dangerouslySetInnerHTML) as much as a style one. */
function DescriptionMarkdown({ text }: { text: string }) {
  return (
    <AstryxIsland>
      <Markdown
        density="compact"
        autolink="gfm"
        contentWidth="100%"
        className="text-text"
        onLinkClick={(href) => {
          openUrl(href).catch(() => toast.error("Could not open the browser."));
          return false;
        }}
      >
        {text}
      </Markdown>
    </AstryxIsland>
  );
}

/**
 * The row's description, clamped with a soft fade, plus "View more" when
 * there is more to see - either signal on its own is enough:
 *
 *  - the text is at the API's truncation cap, so it WAS cut server-side
 *    (this is the one that matters: it's what this feature exists to fix),
 *  - or it overflows the on-screen clamp, for a description short enough
 *    to dodge the cap but still too long to show in full.
 *
 * jsdom lays nothing out, so scrollHeight/clientHeight are both 0 there -
 * the overflow check can never fire in a test (or reveal a bug in it). The
 * char-length check has to stand on its own, which is also why it is
 * checked first.
 */
function PrDescription({ pr, org, project }: { pr: PullRequest; org: string; project: string }) {
  const [open, setOpen] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const clamped = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = clamped.current;
    if (!el) return;
    const measure = () => setOverflowing(el.scrollHeight > el.clientHeight + 1);
    measure();
    // A description that fits on first paint can still reflow past the
    // clamp later - the window narrows, a markdown image finishes loading -
    // and that reflow needs to be caught too, not just the first layout.
    // jsdom's ResizeObserver (see test-setup.ts) is a no-op, matching the
    // scrollHeight/clientHeight-are-both-0 reality noted above: it neither
    // fires in a test nor needs to.
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [pr.description]);

  // Fetched lazily, only while the modal is open - the row already has the
  // truncated text to show, so nothing blocks on this. A completed or
  // abandoned PR's description cannot change again, so once fetched it
  // never needs re-asking - same reasoning as pipeline's staleTime below.
  const finalized = pr.status === "completed" || pr.status === "abandoned";
  const full = useQuery({
    queryKey: ["pr-description", org, project, pr.repo, pr.id],
    queryFn: () => unwrap(commands.prDescription(org, project, pr.repo, pr.id)),
    enabled: open && Boolean(org && project),
    staleTime: finalized ? Infinity : 60_000,
    retry: false,
  });

  if (!pr.description.trim()) return <p className="text-faint">No description.</p>;

  const wasTruncated = pr.description.length >= DESCRIPTION_TRUNCATION_CAP;
  const showMore = wasTruncated || overflowing;
  // ?? would keep a legitimately empty fetch result ("" - description was
  // cleared between the list call and this fetch) and render a blank
  // modal with no explanation; || falls back to the text already on hand.
  const body = full.data || pr.description;

  return (
    <>
      <div className="relative">
        <div ref={clamped} className="max-h-40 overflow-hidden">
          <DescriptionMarkdown text={pr.description} />
        </div>
        {/* The clamp is deliberate - without it a long description makes
            the row enormous. The fade signals there is more below it - but
            only when the clamp actually cut something: a server-truncated
            ~400-char description usually renders well inside max-h-40, and
            painting a fade over text that isn't clipped just washes out the
            last visible line. `showMore` (which also covers wasTruncated)
            still gates the button - the button means "there is more to
            read", true either way; the fade means "this is cut off here",
            true only when the clamp did the cutting. */}
        {overflowing && (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-8 bg-gradient-to-t from-surface to-transparent" />
        )}
      </div>
      {showMore && (
        <button
          className="text-[11px] font-medium text-accent hover:underline"
          onClick={() => setOpen(true)}
        >
          View more
        </button>
      )}
      {open && (
        <Modal
          onClose={() => setOpen(false)}
          className="flex max-h-[85vh] w-[min(46rem,92vw)] flex-col"
        >
          <header className="flex items-center gap-2 border-b border-border px-4 py-3">
            <h3 className="min-w-0 flex-1 truncate text-sm font-semibold text-text">
              <span className="id-mono text-faint">!{pr.id}</span> {pr.title}
            </h3>
            <button
              aria-label="Close description"
              className="shrink-0 rounded p-1 text-muted hover:text-text"
              onClick={() => setOpen(false)}
            >
              <X size={15} />
            </button>
          </header>
          <div className="min-h-0 flex-1 overflow-auto p-4 text-xs">
            <DescriptionMarkdown text={body} />
            {/* The truncated text shows immediately, but while the full
                fetch is in flight it visibly stops mid-word - say more is
                coming rather than leaving that unexplained. */}
            {full.isPending && <p className="mt-3 text-faint">Loading the full description…</p>}
            {/* Keep the modal, and the truncated text, on failure - losing
                both over a flaky fetch is worse than a short description. */}
            {full.isError && (
              <p className="mt-3 text-faint">The full description could not be loaded.</p>
            )}
          </div>
        </Modal>
      )}
    </>
  );
}

/** The latest run only - a summary, not a history. The dialog behind
 * "View history" is where the older runs and the step detail live. */
function BuildCard({ b, total }: { b: PrBuild; total: number }) {
  const when = b.started ? new Date(b.started).toLocaleString() : "";
  const failed = failurePath(b);
  const took = duration(b.started, b.finished);
  return (
    <div className="space-y-1.5 rounded-md border border-border bg-bg p-2">
      <div className="flex items-center gap-2">
        <span className={cn("pill-label rounded-full px-1.5 text-[10px] font-medium", tone(b.status, b.result))}>
          {label(b.result || b.status)}
        </span>
        <span className="truncate text-text">{b.name}</span>
        <span className="id-mono shrink-0 text-faint">{b.number}</span>
        <span className="pill-label shrink-0 rounded bg-surface-2 px-1 text-[10px] text-muted">
          {b.is_validation ? "PR validation" : "CI"}
        </span>
        {b.web_url && (
          <span
            role="button"
            aria-label={`Open build ${b.number} in Azure DevOps`}
            title="Open build in Azure DevOps"
            className="ml-auto shrink-0 rounded p-0.5 text-muted hover:text-accent"
            onClick={(e) => {
              e.stopPropagation();
              openUrl(b.web_url).catch(() => toast.error("Could not open the browser."));
            }}
          >
            <ExternalLink size={12} />
          </span>
        )}
      </div>
      {b.stages.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {b.stages.map((s, i) => (
            <span
              key={i}
              className={cn("pill-label rounded px-1.5 text-[10px]", tone(s.state, s.result))}
              title={`Stage ${s.name}: ${label(s.result || s.state)}`}
            >
              {s.name}
            </span>
          ))}
        </div>
      )}
      {/* Environments are the whole point of the feature - which of these
          did this change actually reach, and how far did it get. */}
      {b.deployments.length > 0 && (
        <div className="flex flex-wrap items-center gap-1">
          <Rocket size={11} className="text-faint" />
          {b.deployments.map((d, i) => (
            <span
              key={i}
              className={cn("pill-label rounded-full px-1.5 text-[10px] font-medium", tone(d.status))}
              title={`${d.release} → ${d.environment}: ${label(d.status)}${
                d.on ? ` (${new Date(d.on).toLocaleString()})` : ""
              }`}
            >
              {d.environment}
            </span>
          ))}
        </div>
      )}
      {/* If it went red, say where without making the user open anything. */}
      {failed && <p className="text-danger">Failed at {failed}</p>}
      <p className="text-faint">
        {when}
        {took && ` · ${took}`}
        {total > 1 && ` · ${total - 1} earlier run${total === 2 ? "" : "s"}`}
      </p>
    </div>
  );
}

/**
 * Validation state for every PR on screen, in one call per REPOSITORY.
 *
 * The expanded row already fetches a PR's builds in full, but that is
 * several calls and only worth it for the row someone opened. A pill on
 * every title needs the same answer for the whole list, so it is asked
 * per repo and sorted out server-side - see `pr_build_states`.
 *
 * A PR with no validation build is absent from the map, which is not the
 * same as passing: the caller shows nothing for either, but it must not
 * turn "no build" into a green claim.
 */
function usePrBuildStates(org: string, project: string, prs: PullRequest[]) {
  const byRepo = useMemo(() => {
    const m = new Map<string, number[]>();
    for (const pr of prs) m.set(pr.repo_id, [...(m.get(pr.repo_id) ?? []), pr.id]);
    // Sorted so the query key is stable across re-orders of the same set.
    return [...m.entries()].map(([repoId, ids]) => ({ repoId, ids: [...ids].sort((a, b) => a - b) }));
  }, [prs]);

  const results = useQueries({
    queries: byRepo.map(({ repoId, ids }) => ({
      queryKey: ["pr-build-states", org, project, repoId, ids],
      queryFn: () => unwrap(commands.prBuildStates(org, project, repoId, ids)),
      enabled: Boolean(org && project) && ids.length > 0,
      staleTime: 60_000,
      retry: false,
    })),
  });

  const states = new Map<number, string>();
  for (const r of results) for (const s of r.data ?? []) states.set(s.pr_id, s.state);
  return states;
}

/** The pill beside a title. Nothing at all for a green run - a list where
 * most rows are fine should be quiet, and the pill is there to pick out
 * the ones that are not. */
function PipelinePill({ state }: { state?: string }) {
  if (state === "running") {
    // Amber, NOT the accent. The accent here is green, and a green pill
    // next to a title is read as "this one passed" - the opposite of what
    // a running build means. Amber is shared with Conflicts, which is
    // fine: both say "not settled yet", and the words tell them apart.
    return (
      <span className="pill-label rounded-full bg-warning/15 px-2 text-[10px] font-medium text-warning">
        Pipeline In Progress
      </span>
    );
  }
  if (state === "failed") {
    return (
      <span className="pill-label rounded-full bg-danger/15 px-2 text-[10px] font-medium text-danger">
        Pipeline Error
      </span>
    );
  }
  return null;
}

function PrRow({
  pr,
  org,
  project,
  buildState,
}: {
  pr: PullRequest;
  org: string;
  project: string;
  buildState?: string;
}) {
  const [open, setOpen] = useState(false);
  const [showPipeline, setShowPipeline] = useState(false);
  // Unresolved review threads, surfaced on the COLLAPSED row - the whole
  // point is knowing without opening anything. Same queryKey (and the same
  // staleTime) as PrThreads uses inside the expanded detail, so this is
  // one fetch per PR shared by both, not two. Active rows only: a
  // completed or abandoned PR has no comment anyone still needs to
  // resolve, and eagerly fetching threads for pages of closed PRs would
  // multiply the panel's ADO calls for nothing (the expanded view still
  // loads them on demand).
  const threadInfo = useQuery({
    queryKey: ["pr-threads", org, project, pr.repo, pr.id],
    queryFn: () => unwrap(commands.prThreads(org, project, pr.repo, pr.id)),
    enabled: pr.status === "active" && Boolean(org && project),
    staleTime: 2 * 60_000,
    retry: false,
  });
  const toResolve = (threadInfo.data ?? []).filter((t) => !isResolved(t.status)).length;
  const created = pr.created ? new Date(pr.created).toLocaleDateString() : "";
  const closed = pr.closed ? new Date(pr.closed).toLocaleDateString() : "";
  // Linked work items load lazily, only when the row is expanded.
  const workItems = useQuery({
    queryKey: ["pr-work-items", org, project, pr.repo, pr.id],
    queryFn: () => unwrap(commands.prWorkItems(org, project, pr.repo, pr.id)),
    enabled: open && Boolean(org && project),
    staleTime: (pr.status !== "active" ? 60 : 5) * 60_000,
    retry: false,
  });
  // Builds + deployments, also lazy: several ADO calls per PR, so only for
  // the row the user actually opened. A closed PR's builds/stages/logs are
  // immutable, so they come from the local cache - but a release can be
  // created against an old build LATER, so on every cache hit the
  // deployments (and only them: one cheap call instead of the full chain)
  // are re-asked and folded back into the cache. The cache stays, and it
  // can never show a deployment picture ADO has since moved past.
  const finalized = pr.status === "completed" || pr.status === "abandoned";
  const pipeline = useQuery({
    queryKey: ["pr-pipeline", org, project, pr.repo_id, pr.id, pr.merge_commit],
    queryFn: async () => {
      const key = `pipe:${org}/${project}:${pr.id}:${pr.merge_commit}`;
      if (finalized) {
        const hit = cacheRead<PrBuild[]>(key, 30 * 24 * 60 * 60_000);
        if (hit) {
          // Validation builds never deploy - only CI builds need re-asking.
          const ids = hit.filter((b) => !b.is_validation).map((b) => b.id);
          if (ids.length === 0) return hit;
          try {
            const fresh = await unwrap(commands.prDeployments(org, project, ids));
            const byId = new Map(fresh.map((f) => [f.build_id, f.deployments]));
            const merged = hit.map((b) =>
              byId.has(b.id) ? { ...b, deployments: byId.get(b.id)! } : b,
            );
            cacheWrite(key, merged);
            return merged;
          } catch {
            // Offline or throttled: the cached history is still the truth
            // about the builds themselves.
            return hit;
          }
        }
      }
      // repo_id, not repo: the Build API filters by repository GUID.
      const data = await unwrap(commands.prPipeline(org, project, pr.repo_id, pr.id, pr.merge_commit));
      if (finalized && data.length > 0 && data.every((b) => b.status === "completed")) {
        cacheWrite(key, data);
      }
      return data;
    },
    enabled: open && Boolean(org && project),
    staleTime: finalized ? Infinity : 60_000,
    retry: false,
  });
  return (
    <div className="rounded-md border border-border bg-surface transition-colors hover:border-border-strong">
      {/* Clicking the row expands the detail; the external-link button is
          the way out to Azure DevOps. */}
      <button
        className="flex w-full items-start gap-3 px-3 py-2 text-left"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {open ? (
          <ChevronDown size={14} className="mt-1 shrink-0 text-muted" />
        ) : (
          <ChevronRight size={14} className="mt-1 shrink-0 text-muted" />
        )}
        {/* No pull-request glyph here: every row on this tab is a pull
            request, so the icon repeated the heading rather than telling
            the rows apart. The chevron already carries the affordance. */}
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-center gap-2">
            <span className="truncate text-sm font-medium text-text">
              <span className="id-mono text-faint">!{pr.id}</span> {pr.title}
            </span>
            {pr.is_draft && (
              <span className="pill-label rounded-full bg-surface-2 px-2 text-[10px] font-medium text-muted">
                Draft
              </span>
            )}
            {pr.has_conflicts && (
              <span className="pill-label rounded-full bg-warning/15 px-2 text-[10px] font-medium text-warning">
                Conflicts
              </span>
            )}
            {/* Quiet when everything is settled, like the pipeline pill -
                the pill exists to pick out rows that still need someone. */}
            {toResolve > 0 && (
              <span className="pill-label rounded-full bg-warning/15 px-2 text-[10px] font-medium text-warning">
                {toResolve} comment{toResolve === 1 ? "" : "s"} to resolve
              </span>
            )}
            <PipelinePill state={buildState} />
          </span>
          <span className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted">
            {/* Same repo-pill treatment as the board's PR chips. */}
            <span className="pill-label rounded-full bg-accent-soft px-1.5 text-[10px] font-medium text-accent">
              {pr.repo}
            </span>
            <span className="flex items-center gap-1">
              <GitBranch size={11} />
              {pr.source_branch} → {pr.target_branch}
            </span>
            <span>{pr.author}</span>
          </span>
        </span>
        {/* Reviewer pips: one dot per reviewer, tinted by vote. */}
        <span className="mt-1 flex shrink-0 items-center gap-1">
          {pr.reviewers.map((r, i) => {
            const d = voteDot(r.vote);
            return (
              <span
                key={i}
                className={cn("inline-block h-2 w-2 rounded-full", d.cls)}
                title={`${r.display_name}: ${d.label}`}
              />
            );
          })}
        </span>
        <span
          role="button"
          aria-label={`Open !${pr.id} in Azure DevOps`}
          title="Open in Azure DevOps"
          className="mt-0.5 shrink-0 rounded p-1 text-muted hover:text-accent"
          onClick={(e) => {
            e.stopPropagation();
            openUrl(pr.web_url).catch(() => toast.error("Could not open the browser."));
          }}
        >
          <ExternalLink size={13} />
        </span>
      </button>

      {open && (
        <div className="border-t border-border/60 px-9 py-2 text-xs">
          {/* Two columns once there's room, Azure DevOps' own layout: the
              description/threads/reviewers/pipeline stack on the left, work
              items as a side panel on the right. Below `lg` there isn't
              room for both, so it goes back to one stack, work items last -
              a squeezed two-column layout is worse than a stack on a
              narrow window. */}
          <div className="flex flex-col gap-4 lg:flex-row lg:items-start">
            <div className="min-w-0 flex-1 space-y-2">
              <PrDescription pr={pr} org={org} project={project} />
              {/* The review conversation, and the only write this panel
                  makes (resolving a thread). Lazy like the work items - one
                  more ADO call per PR, only for the row that was opened. */}
              <PrThreads
                org={org}
                project={project}
                repo={pr.repo}
                prId={pr.id}
                enabled={open}
                finalized={finalized}
              />
              <div className="space-y-1">
                {pr.reviewers.length === 0 && <p className="text-faint">No reviewers assigned.</p>}
                {pr.reviewers.map((r, i) => {
                  const d = voteDot(r.vote);
                  return (
                    <p key={i} className="flex items-center gap-2 text-muted">
                      <span className={cn("inline-block h-2 w-2 rounded-full", d.cls)} />
                      {r.display_name}
                      <span className="text-faint">— {d.label}</span>
                    </p>
                  );
                })}
              </div>
              {/* Pipeline: which builds ran for this PR and where they got
                  deployed. Best-effort - a PR with no pipeline just says so. */}
              <div className="space-y-1 pt-1">
                <div className="flex items-center gap-2">
                  <p className="font-semibold text-muted">Last Run Pipeline</p>
                  {(pipeline.data?.length ?? 0) > 0 && (
                    <button
                      className="ml-auto rounded border border-border px-2 py-0.5 text-[11px] text-muted transition-colors hover:border-border-strong hover:text-text"
                      onClick={() => setShowPipeline(true)}
                    >
                      View history
                    </button>
                  )}
                </div>
                {pipeline.isPending ? (
                  <Skeleton className="h-10" />
                ) : pipeline.isError ? (
                  // Show what Azure DevOps actually said - a bare "could not
                  // read" hid a 400 from a bad repositoryId for a whole release.
                  <p className="text-danger">{pipeline.error.message}</p>
                ) : (pipeline.data?.length ?? 0) === 0 ? (
                  <p className="text-faint">No builds found for this pull request.</p>
                ) : (
                  <BuildCard b={pipeline.data![0]} total={pipeline.data!.length} />
                )}
              </div>
              <div className="flex flex-wrap gap-x-3 text-faint">
                {created && <p>Created {created}</p>}
                {closed && <p>Closed {closed}</p>}
              </div>
            </div>
            {/* Related work items, rendered as DevOps-style chips (icon + id
                + title + state) from the PR's real linkage, not the
                description. No add/remove affordances: this panel never
                writes, and those are Azure DevOps' own UI's, not ours. */}
            {(workItems.data?.length ?? 0) > 0 && (
              <div className="w-full shrink-0 space-y-1 lg:w-64">
                <p className="font-semibold text-muted">Work items</p>
                <div className="space-y-1">
                  {workItems.data!.map((wi) => (
                    <WorkItemChip key={wi.id} wi={wi} />
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {showPipeline && (
        <PipelineDialog
          prId={pr.id}
          prTitle={pr.title}
          repo={pr.repo}
          builds={pipeline.data ?? []}
          org={org}
          project={project}
          onClose={() => setShowPipeline(false)}
        />
      )}
    </div>
  );
}

function PrGroup({
  title,
  prs,
  emphasize = false,
  empty,
  org,
  project,
}: {
  title: string;
  prs: PullRequest[];
  emphasize?: boolean;
  empty: string;
  org: string;
  project: string;
}) {
  const buildStates = usePrBuildStates(org, project, prs);
  return (
    <section className="space-y-2">
      <h2
        className={cn(
          "text-sm font-semibold",
          emphasize && prs.length > 0 ? "text-accent" : "text-muted",
        )}
      >
        {title}
        {prs.length > 0 && <span className="ml-2 text-xs font-normal text-faint">{prs.length}</span>}
      </h2>
      {prs.length === 0 ? (
        <p className="rounded-md border border-dashed border-border px-3 py-2 text-xs text-faint">
          {empty}
        </p>
      ) : (
        <div className="space-y-1.5">
          {prs.map((pr) => (
            <PrRow
              key={`${pr.repo}-${pr.id}`}
              pr={pr}
              org={org}
              project={project}
              buildState={buildStates.get(pr.id)}
            />
          ))}
        </div>
      )}
    </section>
  );
}

/** The tracked repos for one org/project, with the pre-multi-select
 * single-repo key folded in so an existing choice keeps working. */
function readRepoIds(org: string, project: string): string[] {
  try {
    const raw = localStorage.getItem(`tcm-v2-pr-repos:${org}/${project}`);
    if (raw) {
      const arr = JSON.parse(raw);
      if (Array.isArray(arr)) return arr.filter((s) => typeof s === "string" && s);
    }
    const legacy = localStorage.getItem(`tcm-v2-pr-repo:${org}/${project}`);
    if (legacy) return [legacy];
  } catch {
    // session-only
  }
  return [];
}

/** The picker's first, non-repo row. A repo could in principle carry this
 * name too, but ADO repo names and this label colliding AND the user
 * needing both is not a case worth complicating the picker's shape for. */
const YOURS = "Your Pull Requests";

export default function PrPanel({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const repoKey = `tcm-v2-pr-repos:${org}/${project}`;
  // "Your Pull Requests" is ON unless the user deselected it - the panel's
  // default view is your own PRs, with repositories opted into alongside.
  const yoursKey = `tcm-v2-pr-yours:${org}/${project}`;
  const [showYours, setShowYoursRaw] = useState(() => localStorage.getItem(yoursKey) !== "off");
  const setShowYours = (on: boolean) => {
    setShowYoursRaw(on);
    try {
      localStorage.setItem(yoursKey, on ? "on" : "off");
    } catch {
      // session-only
    }
  };
  const [repoIds, setRepoIdsRaw] = useState<string[]>(() => readRepoIds(org, project));
  // The seed above runs once, but repoKey changes the moment the user
  // switches org or project - and the previous project's repos stayed
  // selected, so the panel asked Azure DevOps for pull requests on repos
  // that are not in this project and showed the error it got back. Re-seed
  // from the key that is current. Adjusting state during render (rather
  // than in an effect) is what React prescribes here: it re-renders before
  // painting, so the wrong repos are never on screen or in a request.
  const seededFor = useRef(repoKey);
  if (seededFor.current !== repoKey) {
    seededFor.current = repoKey;
    setRepoIdsRaw(readRepoIds(org, project));
    setShowYoursRaw(localStorage.getItem(yoursKey) !== "off");
  }
  const setRepoIds = (ids: string[]) => {
    setRepoIdsRaw(ids);
    try {
      localStorage.setItem(repoKey, JSON.stringify(ids));
    } catch {
      // session-only
    }
  };

  const overview = useQuery({
    queryKey: ["pr-overview", org, project],
    queryFn: () => unwrap(commands.prOverview(org, project)),
    enabled: Boolean(org && project),
    retry: false,
  });
  const repos = useQuery({
    queryKey: ["repos", org, project],
    queryFn: () => unwrap(commands.listRepos(org, project)),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
  });
  // Active vs completed on the chosen repo. Completed is a separate ADO
  // query (capped server-side), not a client filter.
  const statusKey = "tcm-v2-pr-status";
  const [prStatus, setPrStatusRaw] = useState<"active" | "completed">(() =>
    localStorage.getItem(statusKey) === "completed" ? "completed" : "active",
  );
  const setPrStatus = (s: "active" | "completed") => {
    setPrStatusRaw(s);
    try {
      localStorage.setItem(statusKey, s);
    } catch {
      // session-only
    }
  };
  // "Active on <repo>" drops any PR already shown in a group ABOVE it, so
  // a PR appears once. Only groups actually on screen count: with "Your
  // Pull Requests" deselected, your PRs must stay in the repo sections -
  // deduping against a hidden group would make them vanish entirely.
  const shownAbove = useMemo(() => {
    const ids = new Set<number>();
    for (const pr of overview.data?.awaiting ?? []) ids.add(pr.id);
    if (showYours) for (const pr of overview.data?.mine ?? []) ids.add(pr.id);
    return ids;
  }, [overview.data, showYours]);

  // Tracked repos render in the repo list's order, not click order - the
  // sections keep a stable arrangement however the selection was built.
  // Ids the project no longer has (a repo deleted, a stale import) simply
  // don't render; they stay in storage, harmless.
  const trackedRepos = (repos.data ?? []).filter((r) => repoIds.includes(r.id));

  // Repo NAMES face the MultiSelect (they are what a person recognises,
  // and ADO keeps them unique within a project); ids face storage and the
  // API, which survive a repo being renamed.
  const anyRepoFetching = useIsFetching({ queryKey: ["repo-prs", org, project] }) > 0;

  return (
    // Pull requests are a list, so width costs nothing: the cap steps up
    // with the window and comes off entirely on a very wide one, rather
    // than stopping at 1024px and leaving half the screen black.
    <div className="max-w-3xl space-y-6 xl:max-w-5xl 2xl:max-w-none">
      <div className="flex items-center gap-2">
        <MultiSelect
          ariaLabel="Repositories"
          className="w-64"
          allLabel="Pick pull requests…"
          options={[YOURS, ...(repos.data ?? []).map((r) => r.name)]}
          selected={[...(showYours ? [YOURS] : []), ...trackedRepos.map((r) => r.name)]}
          onChange={(names) => {
            setShowYours(names.includes(YOURS));
            setRepoIds(
              (repos.data ?? []).filter((r) => names.includes(r.name)).map((r) => r.id),
            );
          }}
        />
        {/* Which slice of the repo's PRs the bottom group shows. */}
        <div className="flex rounded-md border border-border p-0.5">
          {(["active", "completed"] as const).map((s) => (
            <button
              key={s}
              className={cn(
                "rounded px-2 py-1 text-xs capitalize transition-colors",
                prStatus === s ? "bg-accent-soft text-accent" : "text-muted hover:text-text",
              )}
              aria-pressed={prStatus === s}
              onClick={() => setPrStatus(s)}
            >
              {s}
            </button>
          ))}
        </div>
        <button
          aria-label="Refresh pull requests"
          title="Refresh pull requests"
          className="rounded p-1.5 text-muted transition-colors hover:text-accent"
          onClick={() => {
            qc.invalidateQueries({ queryKey: ["pr-overview", org, project] });
            qc.invalidateQueries({ queryKey: ["repo-prs", org, project] });
          }}
        >
          <RefreshCw
            size={14}
            className={overview.isFetching || anyRepoFetching ? "animate-spin" : undefined}
          />
        </button>
      </div>

      {overview.isError && <p className="text-sm text-danger">{overview.error.message}</p>}
      {overview.isLoading && (
        <div className="space-y-2">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-14" />
          <Skeleton className="h-14" />
        </div>
      )}

      {overview.data && (
        <>
          <PrGroup
            title="Awaiting your review"
            prs={overview.data.awaiting}
            emphasize
            empty="Nothing waiting on you."
            org={org}
            project={project}
          />
          {showYours && (
            <PrGroup
              title="Your pull requests"
              prs={overview.data.mine}
              empty="You have no active pull requests."
              org={org}
              project={project}
            />
          )}
        </>
      )}

      {trackedRepos.map((r) => (
        <RepoPrSection
          key={r.id}
          org={org}
          project={project}
          repoId={r.id}
          repoName={r.name}
          prStatus={prStatus}
          shownAbove={shownAbove}
        />
      ))}
    </div>
  );
}

/** One tracked repo's slice of the panel: its titled PR group, its own
 * error line, and its own pagination. A section per repo (rather than one
 * merged feed) keeps "Load more" honest - each repo pages independently
 * against its own skip count, and one repo's 400 does not blank the rest. */
function RepoPrSection({
  org,
  project,
  repoId,
  repoName,
  prStatus,
  shownAbove,
}: {
  org: string;
  project: string;
  repoId: string;
  repoName: string;
  prStatus: "active" | "completed";
  shownAbove: Set<number>;
}) {
  // One page at a time (PAGE mirrors Rust's PR_PAGE_SIZE); a full page
  // means there may be another behind it.
  const active = useInfiniteQuery({
    queryKey: ["repo-prs", org, project, repoId, prStatus],
    queryFn: ({ pageParam }) =>
      unwrap(commands.repoPullRequests(org, project, repoId, prStatus, pageParam)),
    initialPageParam: 0,
    getNextPageParam: (last, all) =>
      last.length === PR_PAGE ? all.reduce((n, p) => n + p.length, 0) : undefined,
    enabled: Boolean(org && project && repoId),
    // Completed history only ever gains newer entries - no need to refetch
    // the pages themselves for 10 minutes.
    staleTime: prStatus === "completed" ? 10 * 60_000 : 60_000,
    retry: false,
  });
  const fetched = useMemo(() => active.data?.pages.flat() ?? [], [active.data]);
  // Only the active list can collide with the groups above; completed PRs
  // are never shown there, so they must not be de-duplicated away.
  const repoPrs =
    prStatus === "active" ? fetched.filter((pr) => !shownAbove.has(pr.id)) : fetched;

  if (active.isError) {
    return <p className="text-sm text-danger">{active.error.message}</p>;
  }
  return (
    <>
      <PrGroup
        title={`${prStatus === "active" ? "Active" : "Completed"} on ${repoName}`}
        prs={repoPrs}
        empty={
          active.isLoading
            ? "Loading…"
            : prStatus === "active"
              ? "No other active pull requests on this repository."
              : "No completed pull requests on this repository."
        }
        org={org}
        project={project}
      />
      {active.hasNextPage && (
        <div className="flex justify-center">
          <button
            className="rounded-md border border-border px-3 py-1.5 text-xs text-muted transition-colors hover:border-border-strong hover:text-text disabled:opacity-50"
            disabled={active.isFetchingNextPage}
            onClick={() => active.fetchNextPage()}
          >
            {active.isFetchingNextPage ? "Loading…" : "Load more"}
          </button>
        </div>
      )}
    </>
  );
}
