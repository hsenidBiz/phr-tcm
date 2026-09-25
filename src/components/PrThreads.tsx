// The review conversation on one pull request, and the one control in the
// Work Manager's PR panel that WRITES: resolving a thread, or putting it
// back to active.
//
// Everything else in that panel is deliberately read-only - voting,
// completing, abandoning and replying all stay in Azure DevOps. A thread's
// status is the exception because it is the bookkeeping half of reviewing
// rather than the judgement half: it says nothing new, it is reversible
// from right here, and it is the thing you otherwise leave the app to do.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, MessageSquare, RotateCcw } from "lucide-react";
import { useEffect, useMemo } from "react";
import { toast } from "../lib/toast";
import { Markdown } from "@astryxdesign/core/Markdown";
import { commands, type PrThread } from "../bindings";
import AstryxIsland from "./AstryxIsland";
import { Skeleton } from "./ui/skeleton";
import { cn } from "../lib/cn";
import { relativeTime } from "../lib/history";
import { unwrap } from "../lib/ipc";
import { isResolved } from "../lib/threadStatus";
import { attachmentUrls, markUnavailableImages, swapInlineImages, toBlobImages } from "../lib/inlineImages";

/** What Azure DevOps calls it, in words a reader recognises. */
function statusLabel(status: string): string {
  switch (status.trim().toLowerCase()) {
    case "fixed":
      return "Resolved";
    case "wontfix":
      return "Won't fix";
    case "closed":
      return "Closed";
    case "bydesign":
      return "By design";
    case "pending":
      return "Pending";
    default:
      return "Active";
  }
}

function Thread({
  thread,
  onSetStatus,
  busy,
  withImages,
}: {
  thread: PrThread;
  onSetStatus: (status: string) => void;
  busy: boolean;
  /** Swaps attachment URLs in a comment's markdown for the downloaded
   * image, or an "unavailable" note once the fetch has settled. */
  withImages: (md: string) => string;
}) {
  const resolved = isResolved(thread.status);
  const where = thread.file_path
    ? `${thread.file_path}${thread.line > 0 ? `:${thread.line}` : ""}`
    : "General comment";
  return (
    <div
      className={cn(
        "rounded-md border p-2",
        resolved ? "border-border/60 bg-surface/40" : "border-border bg-surface",
      )}
    >
      <div className="flex items-center gap-2">
        {/* The file is the first thing a reviewer looks for, and it is the
            part most likely to be long - so it truncates from the LEFT,
            keeping the filename rather than the repository root.

            `dir="rtl"` is what moves the ellipsis to the front, but it also
            reorders the neutral characters at the edges: every path came out
            as "src/lib/thing.ts/" with its leading slash at the END. The
            inner LTR isolate fixes the ordering while the outer direction
            still decides which end overflows. */}
        <span className="min-w-0 flex-1 truncate text-left text-[11px] text-muted" dir="rtl">
          <bdi dir="ltr">{where}</bdi>
        </span>
        <span
          className={cn(
            "pill-label shrink-0 rounded-full px-2 text-[10px] font-medium",
            resolved ? "bg-surface-2 text-muted" : "bg-warning/15 text-warning",
          )}
        >
          {statusLabel(thread.status)}
        </span>
      </div>

      <div className="mt-1.5 space-y-1.5">
        {thread.comments.map((c) => (
          <div key={c.id}>
            <p className="flex flex-wrap items-baseline gap-x-2 text-[11px]">
              <span className="font-medium text-text">{c.author}</span>
              {c.published && <span className="text-faint">{relativeTime(c.published)}</span>}
              {c.edited && <span className="text-faint">(edited)</span>}
            </p>
            {/* Remote-authored text, so it goes through the Markdown island
                like the PR description does - never dangerouslySetInnerHTML.
                Links leave for the system browser. */}
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
                {withImages(c.content)}
              </Markdown>
            </AstryxIsland>
          </div>
        ))}
      </div>

      <div className="mt-1.5 flex justify-end">
        {resolved ? (
          <button
            className="flex items-center gap-1 rounded border border-border px-2 py-0.5 text-[11px] text-muted transition-colors hover:border-border-strong hover:text-text disabled:opacity-50"
            disabled={busy}
            onClick={() => onSetStatus("active")}
          >
            <RotateCcw size={11} />
            Reactivate
          </button>
        ) : (
          <button
            className="flex items-center gap-1 rounded border border-accent/60 px-2 py-0.5 text-[11px] text-accent transition-colors hover:bg-accent-soft disabled:opacity-50"
            disabled={busy}
            onClick={() => onSetStatus("fixed")}
          >
            <Check size={11} />
            Resolve
          </button>
        )}
      </div>
    </div>
  );
}

export default function PrThreads({
  org,
  project,
  repo,
  prId,
  enabled,
  finalized,
}: {
  org: string;
  project: string;
  repo: string;
  prId: number;
  /** Only fetched for the row the user actually opened. */
  enabled: boolean;
  /** A completed or abandoned PR's conversation cannot change. */
  finalized: boolean;
}) {
  const qc = useQueryClient();
  const key = ["pr-threads", org, project, repo, prId];
  const threads = useQuery({
    queryKey: key,
    queryFn: () => unwrap(commands.prThreads(org, project, repo, prId)),
    enabled: enabled && Boolean(org && project),
    staleTime: (finalized ? 60 : 2) * 60_000,
    retry: false,
  });

  const setStatus = useMutation({
    mutationFn: ({ threadId, status }: { threadId: number; status: string }) =>
      unwrap(commands.setPrThreadStatus(org, project, repo, prId, threadId, status)),
    // Refetch rather than patch the cache: the status Azure DevOps stored
    // is the one that counts, and it also brings back any comment added
    // while this row was open.
    onSuccess: (saved) => {
      void qc.invalidateQueries({ queryKey: key });
      toast.success(isResolved(saved) ? "Thread resolved." : "Thread reactivated.");
    },
    onError: (e) => toast.error(`Could not update the thread: ${e.message}`),
  });

  // Attachment images in a comment get 401 as a plain markdown image - the
  // WebView sends no bearer header. Held in memory for this open view only
  // (react-query, not the disk cache - the downloaded bytes are large).
  // The cache holds data: URIs, the same shape CommentsPanel's identically
  // keyed query holds - the two never disagree on what a cached entry
  // looks like because there is only one shape.
  const commentTexts = (threads.data ?? []).flatMap((t) => t.comments.map((c) => c.content));
  const imageUrls = attachmentUrls(commentTexts);
  const images = useQuery({
    queryKey: ["comment-images", org, imageUrls],
    queryFn: () => unwrap(commands.commentImages(org, commentTexts)),
    enabled: imageUrls.length > 0,
    staleTime: Infinity,
  });
  // Astryx's Markdown island refuses a data: image src outright, so - only
  // here, not in the shared cache above - each one becomes a blob: object
  // URL. Minted with useMemo (not in queryFn) so a re-render alone never
  // mints another batch, and revoked in the matching effect below the
  // moment this batch stops being the one in use: on the next data change,
  // and on unmount.
  const blobImages = useMemo(() => toBlobImages(images.data ?? []), [images.data]);
  useEffect(
    () => () => {
      for (const img of blobImages) URL.revokeObjectURL(img.data);
    },
    [blobImages],
  );
  /** Swap in what came back; anything still unswapped once the fetch has
   * settled becomes the unavailable note. While still loading, the text is
   * left unchanged - the image appears when it is ready. */
  const withImages = (md: string) => {
    const swapped = swapInlineImages(md, blobImages);
    return images.isPending ? swapped : markUnavailableImages(swapped, "md");
  };

  if (!enabled) return null;

  const all = threads.data ?? [];
  const open = all.filter((t) => !isResolved(t.status));
  const done = all.filter((t) => isResolved(t.status));

  return (
    <div className="space-y-1 pt-1">
      <div className="flex items-center gap-2">
        <MessageSquare size={12} className="text-faint" />
        <p className="font-semibold text-muted">Comments</p>
        {all.length > 0 && (
          <span className="text-faint">
            {open.length} unresolved of {all.length}
          </span>
        )}
      </div>

      {threads.isPending ? (
        <Skeleton className="h-10" />
      ) : threads.isError ? (
        // Say what Azure DevOps said. A bare "could not load" once hid a
        // 400 from a bad repositoryId for a whole release.
        <p className="text-danger">{threads.error.message}</p>
      ) : all.length === 0 ? (
        <p className="text-faint">No comments on this pull request.</p>
      ) : (
        <div className="space-y-1.5">
          {/* Unresolved first: the whole point of the list is what still
              needs an answer. Resolved ones stay visible underneath rather
              than being hidden - they are the record of what was already
              dealt with, and hiding them makes a reviewer re-read the PR
              in the browser to check. */}
          {[...open, ...done].map((t) => (
            <Thread
              key={t.id}
              thread={t}
              busy={setStatus.isPending}
              onSetStatus={(status) => setStatus.mutate({ threadId: t.id, status })}
              withImages={withImages}
            />
          ))}
        </div>
      )}
    </div>
  );
}
