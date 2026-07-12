import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type WorkComment } from "../bindings";
import { unwrap } from "../lib/ipc";
import { Button } from "./ui/button";
import { Textarea } from "./ui/input";
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
  const [text, setText] = useState("");

  const comments = useQuery({
    queryKey: ["wi-comments", org, project, itemId],
    queryFn: () => unwrap(commands.workItemComments(org, project, itemId)),
    retry: false,
  });

  const add = useMutation({
    mutationFn: () => unwrap(commands.addComment(org, project, itemId, text.trim())),
    onSuccess: () => {
      setText("");
      qc.invalidateQueries({ queryKey: ["wi-comments", org, project, itemId] });
      toast.success("Comment added.");
    },
    onError: (e) => toast.error(`Comment failed: ${e.message}`),
  });

  return (
    <section className="space-y-2 border-t border-border pt-3">
      <h3 className="text-xs font-semibold uppercase tracking-wide text-muted">Comments</h3>

      <div className="flex gap-2">
        <Textarea
          aria-label="New comment"
          className="h-14 flex-1 text-sm"
          placeholder="Write a comment"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <Button
          size="sm"
          variant="outline"
          disabled={!text.trim() || add.isPending}
          onClick={() => add.mutate()}
        >
          {add.isPending ? "Posting" : "Post"}
        </Button>
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
              <div className="text-xs">
                <span className="font-medium text-text">{c.created_by}</span>{" "}
                <span className="text-faint">commented {friendlyWhen(c.created_date)}</span>
              </div>
              <div className="whitespace-pre-wrap text-sm text-text">{c.text}</div>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
