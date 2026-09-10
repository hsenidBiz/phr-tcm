import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { marked } from "marked";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type WorkComment } from "../bindings";
import { IconPost } from "../lib/actionIcons";
import { unwrap } from "../lib/ipc";
import { renderMarkdown } from "../lib/markdown";
import { CACHE, persistentQuery } from "../lib/persistentQuery";
import { htmlToMd } from "../lib/richText";
import MarkdownField from "./MarkdownField";
import { Button } from "./ui/button";
import { Skeleton } from "./ui/skeleton";

// Deterministic avatar colours for initials discs (v1 _AVATAR_COLORS).
const AVATAR_COLORS = [
  "#e15b64", "#d99e2b", "#2aa5e0", "#9a74d8",
  "#e0873c", "#4caf7d", "#c65b9a", "#5b8ad9",
];

function initials(name: string): string {
  const parts = name.replace(",", " ").split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

function colorFor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % AVATAR_COLORS.length;
  return AVATAR_COLORS[h];
}

function Avatar({ comment }: { comment: WorkComment }) {
  const img = useQuery({
    queryKey: ["avatar", comment.avatar_url],
    queryFn: () => commands.avatarB64(comment.avatar_url),
    enabled: Boolean(comment.avatar_url),
    staleTime: Infinity,
    retry: false,
  });

  if (img.data) {
    return (
      <img
        alt={comment.created_by}
        className="h-7 w-7 rounded-full object-cover"
        src={`data:image/png;base64,${img.data}`}
      />
    );
  }
  return (
    <span
      aria-label={comment.created_by}
      className="flex h-7 w-7 items-center justify-center rounded-full text-[10px] font-bold text-white"
      style={{ backgroundColor: colorFor(comment.created_by) }}
    >
      {initials(comment.created_by)}
    </span>
  );
}

function friendlyWhen(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const mins = Math.floor((Date.now() - d.getTime()) / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toLocaleDateString();
}

/** Markdown -> the HTML ADO stores. Same conversion the drawer uses for
 * the description, so a comment written here round-trips the way a
 * description does. */
function toHtml(md: string): string {
  return `<div>${marked.parse(md, { async: false, breaks: true })}</div>`;
}

/** ADO stamps modifiedDate on creation too; only a later stamp is an edit. */
function wasEdited(c: WorkComment): boolean {
  return Boolean(c.modified_date && c.created_date && c.modified_date !== c.created_date);
}

export default function CommentsPanel({
  org,
  project,
  itemId,
}: {
  org: string;
  project: string;
  itemId: number;
}) {
  const qc = useQueryClient();
  const [draft, setDraft] = useState("");
  // One comment under edit at a time: its id, the markdown being typed,
  // and what it started as (Update stays off until something changed).
  const [edit, setEdit] = useState<{ id: number; md: string; original: string } | null>(null);

  const comments = useQuery({
    queryKey: ["wi-comments", org, project, itemId],
    ...persistentQuery({
      key: `wi-comments:${org}/${project}/${itemId}`,
      fetcher: () => unwrap(commands.workItemComments(org, project, itemId)),
      ...CACHE.outcomes,
    }),
    retry: false,
  });

  // Who is signed in, by identity id. Edit shows only on one's own
  // comments, the way ADO's form does - ADO would refuse the others
  // anyway, so offering it would only be a button that fails.
  const me = useQuery({
    queryKey: ["connected-user", org],
    queryFn: () => unwrap(commands.connectedUser(org)),
    staleTime: Infinity,
    retry: false,
  });
  const mine = (c: WorkComment) => Boolean(me.data?.id) && c.created_by_id === me.data!.id;

  const invalidate = () => qc.invalidateQueries({ queryKey: ["wi-comments", org, project, itemId] });

  const add = useMutation({
    mutationFn: () => unwrap(commands.addComment(org, project, itemId, toHtml(draft.trim()))),
    onSuccess: () => {
      setDraft("");
      invalidate();
      toast.success("Comment added.");
    },
    onError: (e) => toast.error(`Comment failed: ${e.message}`),
  });

  const update = useMutation({
    mutationFn: (v: { id: number; md: string }) =>
      unwrap(commands.updateComment(org, project, itemId, v.id, toHtml(v.md.trim()))),
    onSuccess: () => {
      setEdit(null);
      invalidate();
      toast.success("Comment updated.");
    },
    onError: (e) => toast.error(`Update failed: ${e.message}`),
  });

  return (
    <section className="space-y-2 border-t border-border pt-3">
      <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">Comments</h3>

      {/* The composer is the same markdown editor the description uses -
          toolbar, live preview - so a comment can carry lists, links and
          code, and ADO renders it the way it renders its own. */}
      <div className="space-y-1">
        <MarkdownField
          label="New comment"
          value={draft}
          onChange={setDraft}
          editing
          onStartEditing={() => {}}
          renderHtml={renderMarkdown}
          rows="h-20"
        />
        <div className="flex justify-end">
          <Button
            size="sm"
            variant="outline"
            disabled={!draft.trim() || add.isPending}
            onClick={() => add.mutate()}
          >
            <IconPost aria-hidden />
            {add.isPending ? "Posting" : "Post"}
          </Button>
        </div>
      </div>

      {comments.isLoading && <Skeleton className="h-16" />}
      {comments.isError && <p className="text-sm text-danger">{comments.error.message}</p>}
      {comments.data && comments.data.length === 0 && (
        <p className="text-sm text-muted">No comments yet.</p>
      )}

      <ul className="space-y-3">
        {(comments.data ?? []).map((c) => (
          <li key={c.id} className="flex gap-2">
            <Avatar comment={c} />
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline gap-2 text-xs">
                <span className="font-medium text-text">{c.created_by}</span>
                <span className="text-faint">
                  commented {friendlyWhen(c.created_date)}
                  {wasEdited(c) && " · edited"}
                </span>
                {mine(c) && edit?.id !== c.id && (
                  <button
                    className="ml-auto text-[11px] text-faint underline-offset-2 hover:text-accent hover:underline"
                    onClick={() => {
                      const md = htmlToMd(c.text_html || c.text);
                      setEdit({ id: c.id, md, original: md });
                    }}
                  >
                    Edit
                  </button>
                )}
              </div>
              {edit?.id === c.id ? (
                <div className="space-y-1">
                  <MarkdownField
                    label="Comment"
                    value={edit.md}
                    onChange={(md) => setEdit({ ...edit, md })}
                    editing
                    onStartEditing={() => {}}
                    renderHtml={renderMarkdown}
                    rows="h-24"
                  />
                  {/* ADO's own pair, in ADO's own words. */}
                  <div className="flex justify-end gap-2">
                    <Button size="sm" variant="ghost" onClick={() => setEdit(null)}>
                      Cancel
                    </Button>
                    <Button
                      size="sm"
                      disabled={!edit.md.trim() || edit.md === edit.original || update.isPending}
                      onClick={() => update.mutate({ id: edit.id, md: edit.md })}
                    >
                      {update.isPending ? "Updating" : "Update"}
                    </Button>
                  </div>
                </div>
              ) : c.text_html ? (
                // Stored HTML -> markdown -> sanitised HTML: the same route
                // the description takes, so what renders here is what the
                // editor would show, and `renderMarkdown` strips anything
                // that is not markdown's own tags before it reaches a
                // webview with IPC.
                <div
                  className="md-preview text-sm text-text"
                  dangerouslySetInnerHTML={{ __html: renderMarkdown(htmlToMd(c.text_html)) }}
                />
              ) : (
                // A row cached from before the HTML travelled: plain text.
                <div className="whitespace-pre-wrap text-sm text-text">{c.text}</div>
              )}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
