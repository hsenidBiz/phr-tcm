// The video kept of a failed unattended case ("Keep a video of failed
// cases"), under the case in Past runs and the review: Play shows it in the
// app, Download saves the file where the person chooses. Rust reads it from
// the run's own folder by name (a name it accepts only), refuses to hand a
// very large one to the page (it can still be saved), and says whether one
// exists at all - a case without a video, or whose file has been pruned,
// gets no controls.

import { useEffect, useRef } from "react";
import { Button } from "../../components/ui/button";
import { IconCancel, IconDownload, IconExport, IconPlayVideo } from "../../lib/actionIcons";
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

/** The tooltip of a video that is too large to play in the page. */
const TOO_LARGE = "This video is too large to play here - download it instead";

const VIDEO_PLAYER =
  "max-h-96 max-w-full rounded border border-border";

/**
 * `compact` is the variant for a case's one-line row (Past runs): two icon
 * buttons, Watch and Download, that sit in the row beside its result, with
 * the video itself opening on a line of its own below everything else on the row (order-last), so Replay stays on the row. A
 * video that is gone leaves the same note, short, and both buttons off.
 */
export default function CaseVideo({
  name,
  className,
  compact = false,
}: {
  name?: string | null;
  className?: string;
  compact?: boolean;
}) {
  const { info, src, close, play, save } = useVideoFile(name);
  // The Watch and Close buttons swap places: the focus stays with the one
  // that was pressed.
  const watchBtn = useRef<HTMLButtonElement>(null);
  const closeBtn = useRef<HTMLButtonElement>(null);
  const wasPlaying = useRef(false);
  useEffect(() => {
    if (!compact) return;
    if (src) closeBtn.current?.focus();
    else if (wasPlaying.current) watchBtn.current?.focus();
    wasPlaying.current = Boolean(src);
  }, [src, compact]);

  if (!name || info.isLoading || info.isError) return null;
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
        {src ? (
          <Button ref={closeBtn} size="sm" variant="outline" className={iconButton} aria-label="Close the failure video" title="Close the video" onClick={close}>
            <IconCancel aria-hidden />
          </Button>
        ) : (
          <Button
            ref={watchBtn}
            size="sm"
            variant="outline"
            className={iconButton}
            aria-label="Watch the failure video"
            disabled={!playable || play.isPending}
            title={playable ? `Watch the failure video (${sized})` : `${TOO_LARGE} (${sized})`}
            onClick={() => play.mutate()}
          >
            <IconPlayVideo aria-hidden />
          </Button>
        )}
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
        {src && (
          <video
            aria-label="Video of the failed case"
            src={src}
            controls
            autoPlay
            onError={() => toast.error("This video cannot be played here - download it instead.")}
            className={cn("order-last basis-full", VIDEO_PLAYER)}
          />
        )}
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
            title={playable ? "Watch the video here" : TOO_LARGE}
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
      {src && <video aria-label="Video of the failed case" src={src} controls autoPlay onError={() => toast.error("This video cannot be played here - download it instead.")} className={VIDEO_PLAYER} />}
    </div>
  );
}
