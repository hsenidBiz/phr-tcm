// The video kept of a failed unattended case ("Keep a video of failed
// cases"), under the case in Past runs and the review: Watch opens it in the
// player over the app (with the case's steps marked on it), Download saves
// the file where the person chooses. Rust reads it from the run's own folder
// by name (a name it accepts only), refuses to hand a very large one to the
// page (it can still be saved), and says whether one exists at all - a case
// without a video, or whose file has been pruned, gets no controls.

import { useEffect, useState } from "react";
import { Button } from "../../components/ui/button";
import { IconDownload, IconExport, IconPlayVideo } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { fileSize } from "./TestFilesDialog";
import { useVideoFile, videoInfoKey } from "./useVideoFile";
import VideoPlayer from "./VideoPlayer";
import type { Timeline } from "./videoTimeline";

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

/** The tooltip of a video that is too large to play in the page. */
const TOO_LARGE = "This video is too large to play here - download it instead";

/**
 * `compact` is the variant for a case's one-line row (Past runs): two icon
 * buttons, Watch and Download, that sit in the row beside its result. Watch
 * opens the player over the app, so the row never grows a second line. A
 * video that is gone leaves the same note, short, and both buttons off.
 */
export default function CaseVideo({
  name,
  className,
  compact = false,
  timeline,
}: {
  name?: string | null;
  className?: string;
  compact?: boolean;
  timeline: Timeline;
}) {
  const { info, src, close, play, save } = useVideoFile(name);
  const [open, setOpen] = useState(false);
  // The bytes can land after the player was closed: let that URL go at once,
  // and never reopen the player for it.
  useEffect(() => {
    if (!open && src) close();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `close` is a fresh closure each render
  }, [open, src]);

  if (!name || info.isLoading || info.isError) return null;
  const watch = () => {
    if (play.isPending) return;
    setOpen(true);
    play.mutate();
  };
  const player = open ? (
    <VideoPlayer
      timeline={timeline}
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
  ) : null;

  if (compact) {
    const iconButton = "px-2";
    if (!info.data) {
      return (
        <>
          <span className="max-w-48 truncate text-xs text-faint" title={VIDEO_GONE}>
            {VIDEO_GONE}
          </span>
          <Button size="sm" variant="outline" className={iconButton} disabled aria-label="Watch the failure video" title={VIDEO_GONE}>
            <IconPlayVideo aria-hidden />
          </Button>
          <Button size="sm" variant="outline" className={iconButton} disabled aria-label="Download the failure video" title={VIDEO_GONE}>
            <IconDownload aria-hidden />
          </Button>
        </>
      );
    }
    const { size, playable } = info.data;
    const sized = fileSize(size);
    return (
      <>
        <Button
          size="sm"
          variant="outline"
          className={iconButton}
          aria-label="Watch the failure video"
          disabled={!playable}
          title={playable ? `Watch the failure video (${sized})` : `${TOO_LARGE} (${sized})`}
          onClick={watch}
        >
          <IconPlayVideo aria-hidden />
        </Button>
        <Button
          size="sm"
          variant="outline"
          className={iconButton}
          aria-label="Download the failure video"
          disabled={save.isPending}
          title={`Download the failure video (${sized})`}
          onClick={() => save.mutate()}
        >
          <IconDownload aria-hidden />
        </Button>
        {player}
      </>
    );
  }
  if (!info.data) return <p className={cn("text-xs text-faint", className)}>{VIDEO_GONE}</p>;
  const { size, playable } = info.data;

  return (
    <div className={cn("space-y-1", className)}>
      <div className="flex flex-wrap items-center gap-2">
        <p className="text-xs font-medium text-muted">Video of the failure</p>
        <span className="text-xs text-faint">{fileSize(size)}</span>
        <Button
          size="sm"
          variant="outline"
          disabled={!playable}
          title={playable ? "Watch the video with its steps" : TOO_LARGE}
          onClick={watch}
        >
          <IconPlayVideo aria-hidden />
          Watch video
        </Button>
        <Button size="sm" variant="outline" disabled={save.isPending} onClick={() => save.mutate()}>
          <IconExport aria-hidden />
          Download video
        </Button>
      </div>
      {player}
    </div>
  );
}
