// The video kept of a failed unattended case ("Keep a video of failed
// cases"), under the case in Past runs and the review: Play shows it in the
// app, Download saves the file where the person chooses. Rust reads it from
// the run's own folder by name (a name it accepts only), refuses to hand a
// very large one to the page (it can still be saved), and says whether one
// exists at all - a case without a video, or whose file has been pruned,
// gets no controls.

import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconCancel, IconExport, IconPlayVideo } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { pickSavePath } from "../../lib/runnerFiles";
import { toast } from "../../lib/toast";
import { fileSize } from "./TestFilesDialog";

/** What the row says when the file is gone. */
export const VIDEO_GONE = "The video of this case is no longer on this machine.";

/** The query one video's facts are read through. */
export const videoInfoKey = (name: string) => ["autorun-video-info", name] as const;

/** The bytes of a base64 string, for a Blob. */
function bytesOf(b64: string): Uint8Array<ArrayBuffer> {
  const raw = atob(b64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

/** Said once for a run that was meant to keep videos of failed cases but ran in
 * a browser that cannot record. Quiet text, never a badge on a case: nothing
 * about the cases' checks was skipped. */
export const VIDEO_UNAVAILABLE = "Video was not recorded: this browser version cannot record.";

export function VideoUnavailableNote({ run }: { run: { video_unavailable?: boolean } }) {
  if (!run.video_unavailable) return null;
  return <p className="text-xs text-faint">{VIDEO_UNAVAILABLE}</p>;
}

export default function CaseVideo({ name, className }: { name?: string | null; className?: string }) {
  const info = useQuery({
    queryKey: videoInfoKey(name ?? ""),
    queryFn: () => commands.autoRunVideoInfo(name as string),
    enabled: Boolean(name),
    // Read again whenever a list mounts: a file pruned since the last look
    // must show as gone.
    staleTime: 0,
    refetchOnMount: "always",
    retry: false,
  });
  /** The object URL of the video being watched, if it is. */
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => () => {
    if (src) URL.revokeObjectURL(src);
  }, [src]);

  const play = useMutation({
    mutationFn: async () => {
      const b64 = await unwrapStr(commands.autoRunVideo(name as string));
      return URL.createObjectURL(new Blob([bytesOf(b64)], { type: "video/mp4" }));
    },
    onSuccess: setSrc,
    onError: (e) => toast.error(`Could not play the video: ${e.message}`),
  });
  const save = useMutation({
    mutationFn: async () => {
      const path = await pickSavePath(name as string);
      if (!path) return false;
      await unwrapStr(commands.autoRunSaveVideo(name as string, path));
      return true;
    },
    onSuccess: (saved) => {
      if (saved) toast.success("Saved the video");
    },
    onError: (e) => toast.error(`Could not save the video: ${e.message}`),
  });

  if (!name || info.isLoading || info.isError) return null;
  if (!info.data) return <p className={cn("text-xs text-faint", className)}>{VIDEO_GONE}</p>;
  const { size, playable } = info.data;

  return (
    <div className={cn("space-y-1", className)}>
      <div className="flex flex-wrap items-center gap-2">
        <p className="text-xs font-medium text-muted">Video of the failure</p>
        <span className="text-xs text-faint">{fileSize(size)}</span>
        {src ? (
          <Button size="sm" variant="outline" onClick={() => setSrc(null)}>
            <IconCancel aria-hidden />
            Close video
          </Button>
        ) : (
          <Button
            size="sm"
            variant="outline"
            disabled={!playable || play.isPending}
            title={playable ? "Watch the video here" : "This video is too large to play here - download it instead"}
            onClick={() => play.mutate()}
          >
            <IconPlayVideo aria-hidden />
            Play video
          </Button>
        )}
        <Button size="sm" variant="outline" disabled={save.isPending} onClick={() => save.mutate()}>
          <IconExport aria-hidden />
          Download video
        </Button>
      </div>
      {src && <video aria-label="Video of the failed case" src={src} controls autoPlay onError={() => toast.error("This video cannot be played here - download it instead.")} className="max-h-96 max-w-full rounded border border-border" />}
    </div>
  );
}
