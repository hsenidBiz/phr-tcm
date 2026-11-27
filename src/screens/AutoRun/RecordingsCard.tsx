// The Recordings card: every recording an assistant kept (a discovery it
// recorded for the person to watch), newest first as Rust lists them. Play
// shows one in the app, Save copies it where the person chooses, Remove
// deletes it after one question. It draws nothing while loading, after an
// error, or with no recordings, and it is shown in Basic mode too.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { commands, type RecordingEntry } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconExport, IconPlayVideo, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { VIDEO_GONE } from "./CaseVideo";
import { useVideoFile } from "./useVideoFile";
import VideoPlayer from "./VideoPlayer";
import { clockText, recordingTimeline } from "./videoTimeline";

/** The query the list is read through (a local file list, like Past runs). */
export const RECORDINGS_KEY = ["autorun-recordings"] as const;

/** What the inline question says before a recording is deleted. */
export const REMOVE_CONFIRM = "Remove this recording? It cannot be undone.";

/** Names Windows reserves for devices, with or without an extension. */
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$/i;

/** A title made safe as a file name: characters Windows refuses (including
 * both slashes) are dropped, and a reserved device name falls back. */
export function fileNameFor(title: string): string {
  const clean = title
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[. ]+$/, "");
  if (!clean || RESERVED.test(clean)) return "recording.mp4";
  return `${clean}.mp4`;
}

function RecordingRow({ entry }: { entry: RecordingEntry }) {
  const qc = useQueryClient();
  const { info, src, close, play, save } = useVideoFile(entry.file, fileNameFor(entry.title));
  const [asking, setAsking] = useState(false);
  const [open, setOpen] = useState(false);
  // The bytes can land after the player was closed: let that URL go at once.
  useEffect(() => {
    if (!open && src) close();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `close` is a fresh closure each render
  }, [open, src]);
  const remove = useMutation({
    mutationFn: async () => unwrapStr(commands.autoRunRemoveRecording(entry.id)),
    onSuccess: () => {
      setAsking(false);
      void qc.invalidateQueries({ queryKey: RECORDINGS_KEY });
    },
    onError: (e) => toast.error(`Could not remove the recording: ${e.message}`),
  });
  const gone = !info.isLoading && !info.isError && !info.data;
  const playable = !gone && info.data?.playable !== false;

  return (
    <li className="space-y-2 py-2 first:pt-0 last:pb-0">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0 space-y-0.5 text-xs">
          <p className="truncate font-medium text-text">{entry.title}</p>
          <p className="text-faint">
            <span>{new Date(entry.created).toLocaleString()}</span>
            {" · "}
            <span>{clockText(entry.duration_ms)}</span>
          </p>
          {gone && <p className="text-faint">{VIDEO_GONE}</p>}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            aria-label={`Play ${entry.title}`}
            disabled={!playable}
            title={playable ? "Watch the recording with its actions" : "This recording is too large to play here - save it instead"}
            onClick={() => {
              if (play.isPending) return;
              setOpen(true);
              play.mutate();
            }}
          >
            <IconPlayVideo aria-hidden />
            Play
          </Button>
          <Button size="sm" variant="outline" aria-label={`Save ${entry.title}`} disabled={gone || save.isPending} onClick={() => save.mutate()}>
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
      {open && (
        <VideoPlayer
          timeline={recordingTimeline(entry)}
          src={src}
          loading={play.isPending}
          error={play.isError}
          saving={save.isPending}
          onSave={() => save.mutate()}
          onClose={() => {
            setOpen(false);
            close();
          }}
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
