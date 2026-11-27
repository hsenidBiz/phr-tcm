// The Recordings card: every recording an assistant kept (a discovery it
// recorded for the person to watch), newest first as Rust lists them. Play
// shows one in the app, Save copies it where the person chooses, Remove
// deletes it after one question. It draws nothing while loading, after an
// error, or with no recordings, and it is shown in Basic mode too.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type RecordingEntry } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconCancel, IconExport, IconPlayVideo, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { useVideoFile } from "./useVideoFile";

/** The query the list is read through (a local file list, like Past runs). */
export const RECORDINGS_KEY = ["autorun-recordings"] as const;

/** What the inline question says before a recording is deleted. */
export const REMOVE_CONFIRM = "Remove this recording? It cannot be undone.";

/** `83000` ms as `1:23`. */
function lengthText(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/** A title made safe as a file name: characters Windows refuses are dropped. */
function fileNameFor(title: string): string {
  const clean = title.replace(/[<>:"/\|?*\u0000-\u001f]/g, "").replace(/\s+/g, " ").trim();
  return `${clean || "recording"}.mp4`;
}

function RecordingRow({ entry }: { entry: RecordingEntry }) {
  const qc = useQueryClient();
  const { info, src, close, play, save } = useVideoFile(entry.file, fileNameFor(entry.title));
  const [asking, setAsking] = useState(false);
  const remove = useMutation({
    mutationFn: async () => unwrapStr(commands.autoRunRemoveRecording(entry.id)),
    onSuccess: () => {
      setAsking(false);
      void qc.invalidateQueries({ queryKey: RECORDINGS_KEY });
    },
    onError: (e) => toast.error(`Could not remove the recording: ${e.message}`),
  });
  const playable = info.data?.playable !== false;

  return (
    <li className="space-y-2 py-2 first:pt-0 last:pb-0">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0 space-y-0.5 text-xs">
          <p className="truncate font-medium text-text">{entry.title}</p>
          <p className="text-faint">
            <span>{new Date(entry.created).toLocaleString()}</span>
            {" · "}
            <span>{lengthText(entry.duration_ms)}</span>
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {src ? (
            <Button size="sm" variant="outline" aria-label={`Close ${entry.title}`} onClick={close}>
              <IconCancel aria-hidden />
              Close
            </Button>
          ) : (
            <Button
              size="sm"
              variant="outline"
              aria-label={`Play ${entry.title}`}
              disabled={!playable || play.isPending}
              title={playable ? "Watch the recording here" : "This recording is too large to play here - save it instead"}
              onClick={() => play.mutate()}
            >
              <IconPlayVideo aria-hidden />
              Play
            </Button>
          )}
          <Button size="sm" variant="outline" aria-label={`Save ${entry.title}`} disabled={save.isPending} onClick={() => save.mutate()}>
            <IconExport aria-hidden />
            Save
          </Button>
          <Button size="sm" variant="outline" aria-label={`Remove ${entry.title}`} disabled={remove.isPending} onClick={() => setAsking(true)}>
            <IconRemove aria-hidden />
            Remove
          </Button>
        </div>
      </div>
      {asking && (
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <p className="text-muted">{REMOVE_CONFIRM}</p>
          <Button size="sm" variant="outline" disabled={remove.isPending} onClick={() => remove.mutate()}>
            Remove recording
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setAsking(false)}>
            Keep
          </Button>
        </div>
      )}
      {src && (
        <video
          aria-label={`Recording: ${entry.title}`}
          src={src}
          controls
          autoPlay
          onError={() => toast.error("This recording cannot be played here - save it instead.")}
          className="max-h-96 max-w-full rounded border border-border"
        />
      )}
    </li>
  );
}

export default function RecordingsCard() {
  const headingId = useId();
  const list = useQuery({
    queryKey: RECORDINGS_KEY,
    queryFn: () => unwrapStr(commands.autoRunRecordings()),
    retry: false,
  });
  if (list.isLoading || list.isError || !list.data || list.data.length === 0) return null;
  return (
    <section aria-labelledby={headingId} className="space-y-2 rounded-md border border-border bg-surface p-3">
      <h3 id={headingId} className="text-sm font-semibold text-text">
        Recordings
      </h3>
      <ul className="divide-y divide-border">
        {list.data.map((r) => (
          <RecordingRow key={r.id} entry={r} />
        ))}
      </ul>
    </section>
  );
}
