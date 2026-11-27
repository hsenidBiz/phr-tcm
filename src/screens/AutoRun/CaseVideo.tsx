// The video kept of a failed unattended case ("Keep a video of failed
// cases"), under the case in Past runs and the review: Play shows it in the
// app, Download saves the file where the person chooses. Rust reads it from
// the run's own folder by name (a name it accepts only), refuses to hand a
// very large one to the page (it can still be saved), and says whether one
// exists at all - a case without a video, or whose file has been pruned,
// gets no controls.

import { Button } from "../../components/ui/button";
import { IconCancel, IconExport, IconPlayVideo } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { toast } from "../../lib/toast";
import { fileSize } from "./TestFilesDialog";
import { useVideoFile, videoInfoKey } from "./useVideoFile";

/** What the row says when the file is gone. */
export const VIDEO_GONE = "The video of this case is no longer on this machine.";

export { videoInfoKey };

/** Said once for a run that was meant to keep videos of failed cases but ran in
 * a browser that cannot record. Quiet text, never a badge on a case: nothing
 * about the cases' checks was skipped. */
export const VIDEO_UNAVAILABLE = "Video was not recorded: this browser version cannot record.";

export function VideoUnavailableNote({ run }: { run: { video_unavailable?: boolean } }) {
  if (!run.video_unavailable) return null;
  return <p className="text-xs text-faint">{VIDEO_UNAVAILABLE}</p>;
}

export default function CaseVideo({ name, className }: { name?: string | null; className?: string }) {
  const { info, src, close, play, save } = useVideoFile(name);

  if (!name || info.isLoading || info.isError) return null;
  if (!info.data) return <p className={cn("text-xs text-faint", className)}>{VIDEO_GONE}</p>;
  const { size, playable } = info.data;

  return (
    <div className={cn("space-y-1", className)}>
      <div className="flex flex-wrap items-center gap-2">
        <p className="text-xs font-medium text-muted">Video of the failure</p>
        <span className="text-xs text-faint">{fileSize(size)}</span>
        {src ? (
          <Button size="sm" variant="outline" onClick={close}>
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
