// The player a case's video or a kept recording opens in, over the app. The
// video carries step ticks on its bar, a red stretch from the step that
// failed, a list of the steps that opens into their actions, and keys. Every
// marker is a start time Rust recorded; none is worked out here. The run
// report has the same player in its own script
// (src-tauri/web/autorun-report-player.js) - keep the two in step.

import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import {
  IconCancel,
  IconExport,
  IconHideActions,
  IconNextStep,
  IconPauseVideo,
  IconPlayVideo,
  IconPreviousStep,
  IconShowActions,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { MODAL_LARGE } from "./modalWidths";
import {
  clockText,
  currentAt,
  failedFrom,
  SKIP_MS,
  SPEEDS,
  startsOf,
  stepTarget,
  timelinePercent,
  type Timeline,
  type TimelineMark,
} from "./videoTimeline";

/** Under the video of a case recorded before step markers were kept. */
export const NO_MARKERS = "No step markers: this video was recorded before they were kept.";
/** What the stage says when the video cannot be played. */
export const VIDEO_WONT_PLAY = "This video cannot be played here - save it instead.";
/** What the stage says while the video's bytes are on their way. */
export const VIDEO_LOADING = "Loading the video...";

const jumpClass =
  "flex min-w-0 flex-1 items-center gap-2 rounded px-2 py-1 text-left text-sm text-text hover:bg-border/40 focus-visible:outline-2 focus-visible:outline-accent";

function MarkList({
  marks,
  label,
  nowIndex,
  current,
  onJump,
}: {
  marks: TimelineMark[];
  label?: string;
  nowIndex: number;
  /** The aria-current value the playing action gets; none inside a step. */
  current: "true" | undefined;
  onJump: (ms: number) => void;
}) {
  return (
    <ol aria-label={label} className="space-y-0.5">
      {marks.map((m, i) => (
        <li key={`${m.at_ms}-${i}`}>
          <button
            type="button"
            aria-label={`${m.label} at ${clockText(m.at_ms)}`}
            aria-current={i === nowIndex ? current : undefined}
            className={cn(jumpClass, "w-full", i === nowIndex && current && "bg-accent/10")}
            onClick={() => onJump(m.at_ms)}
          >
            <span className="min-w-0 flex-1 truncate">{m.label}</span>
            <span className="text-xs text-faint">{clockText(m.at_ms)}</span>
          </button>
        </li>
      ))}
    </ol>
  );
}

export default function VideoPlayer({
  timeline,
  src,
  loading,
  error,
  saving,
  onSave,
  onClose,
}: {
  timeline: Timeline;
  src: string | null;
  loading: boolean;
  error: boolean;
  saving: boolean;
  onSave: () => void;
  onClose: () => void;
}) {
  const video = useRef<HTMLVideoElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const bar = useRef<HTMLDivElement>(null);
  const failedItem = useRef<HTMLLIElement | null>(null);
  const [now, setNow] = useState(0);
  const [duration, setDuration] = useState<number>(Number.NaN);
  const [playing, setPlaying] = useState(false);
  const [rate, setRate] = useState<number>(1);
  const [broken, setBroken] = useState(false);
  const failedIdx = timeline.steps.findIndex((s) => s.failed);
  const [open, setOpen] = useState<Set<number>>(() => new Set(failedIdx >= 0 ? [failedIdx] : []));

  const hasSteps = timeline.steps.length > 0;
  const hasMarks = timeline.marks.length > 0;
  const starts = useMemo(() => startsOf(timeline), [timeline]);
  // The video's own length once it is known, else the one recorded with it.
  const len =
    Number.isFinite(duration) && duration > 0
      ? duration * 1000
      : timeline.length_ms !== null && Number.isFinite(timeline.length_ms) && timeline.length_ms > 0
        ? timeline.length_ms
        : 0;
  const idx = currentAt(starts, now);
  const stepName = hasSteps && idx >= 0 ? timeline.steps[idx].name : "";
  const failAt = failedFrom(timeline);
  const cannotPlay = error || broken;

  const readNow = () => {
    const t = video.current?.currentTime ?? 0;
    return Number.isFinite(t) ? t * 1000 : 0;
  };
  const sync = () => {
    const v = video.current;
    if (!v) return;
    setNow(readNow());
    setDuration(v.duration);
  };
  const seek = (ms: number) => {
    const v = video.current;
    if (!v) return;
    const to = Math.min(len > 0 ? len : Number.POSITIVE_INFINITY, Math.max(0, ms));
    v.currentTime = to / 1000;
    setNow(to);
  };
  const toggle = () => {
    const v = video.current;
    if (!v || !src || cannotPlay) return;
    if (playing) {
      v.pause();
      setPlaying(false);
    } else {
      setPlaying(true);
      void v.play()?.catch(() => setPlaying(false));
    }
  };
  const moveStep = (dir: -1 | 1) => {
    const to = stepTarget(timeline, readNow(), dir);
    if (to !== null) seek(to);
    else if (dir < 0) seek(0);
  };

  // The keys live on the dialog itself, so they work wherever the focus is
  // inside it. A focused button keeps Space and Enter for itself.
  const latest = useRef({ toggle, seek, readNow, moveStep });
  latest.current = { toggle, seek, readNow, moveStep };
  useEffect(() => {
    const dialog = content.current?.closest<HTMLElement>('[role="dialog"]');
    if (!dialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const onButton = e.target instanceof HTMLElement && e.target.closest("button") !== null;
      const k = e.key;
      const h = latest.current;
      if ((k === " " || k === "Enter") && onButton) return;
      if (k === " ") {
        e.preventDefault();
        h.toggle();
      } else if (k === "ArrowRight") {
        e.preventDefault();
        h.seek(h.readNow() + SKIP_MS);
      } else if (k === "ArrowLeft") {
        e.preventDefault();
        h.seek(h.readNow() - SKIP_MS);
      } else if (k === "]" || k === "[") {
        e.preventDefault();
        h.moveStep(k === "]" ? 1 : -1);
      }
    };
    dialog.addEventListener("keydown", onKey);
    return () => dialog.removeEventListener("keydown", onKey);
  }, []);

  // The failed step is on screen when the player opens.
  useEffect(() => {
    failedItem.current?.scrollIntoView?.({ block: "nearest" });
  }, []);

  const toggleOpen = (i: number) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(i)) next.add(i);
      return next;
    });

  const seekFromClick = (clientX: number) => {
    const r = bar.current?.getBoundingClientRect();
    if (!r || !r.width || !len) return;
    seek(((clientX - r.left) / r.width) * len);
  };

  const ticks = hasSteps
    ? timeline.steps.map((s) => ({ title: s.name, at: s.at_ms }))
    : timeline.marks.map((m) => ({ title: m.label, at: m.at_ms }));

  return (
    <Modal onClose={onClose} label={timeline.title} className={`${MODAL_LARGE} flex flex-col gap-3 p-4`}>
      <div ref={content} className="flex min-h-0 flex-1 flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <h2 className="min-w-0 truncate text-base font-semibold text-text">{timeline.title}</h2>
          <Button size="sm" variant="outline" aria-label="Close player" onClick={onClose}>
            <IconCancel aria-hidden />
          </Button>
        </div>
        <div className="grid min-h-0 flex-1 gap-4 overflow-auto lg:grid-cols-[minmax(0,1fr)_20rem]">
          <div className="min-w-0 space-y-3">
            <div className="flex items-center justify-center rounded bg-surface">
              <video
                ref={video}
                aria-label={timeline.title}
                src={src ?? undefined}
                preload="metadata"
                hidden={loading || cannotPlay}
                className="max-h-[60vh] max-w-full rounded border border-border"
                onTimeUpdate={sync}
                onSeeked={sync}
                onLoadedMetadata={sync}
                onDurationChange={sync}
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onEnded={() => setPlaying(false)}
                onError={() => setBroken(true)}
              />
              {loading && !cannotPlay && <p className="p-8 text-sm text-muted">{VIDEO_LOADING}</p>}
              {cannotPlay && <p className="p-8 text-sm text-muted">{VIDEO_WONT_PLAY}</p>}
            </div>
            <div
              ref={bar}
              role="slider"
              tabIndex={0}
              aria-label="Position"
              aria-valuemin={0}
              aria-valuemax={len / 1000}
              aria-valuenow={now / 1000}
              aria-valuetext={`${clockText(now)} of ${clockText(len)}${stepName ? `, ${stepName}` : ""}`}
              className="relative h-4 cursor-pointer rounded bg-border focus-visible:outline-2 focus-visible:outline-accent"
              onClick={(e) => seekFromClick(e.clientX)}
            >
              {failAt !== null && (
                <span
                  data-testid="failed-stretch"
                  className="absolute inset-y-0 bg-danger/40"
                  style={{ left: `${timelinePercent(failAt, len)}%`, right: 0 }}
                />
              )}
              {ticks.map((t, i) => (
                <span
                  key={`${t.at}-${i}`}
                  title={t.title}
                  className="absolute inset-y-0 w-0.5 bg-muted"
                  style={{ left: `${timelinePercent(t.at, len)}%` }}
                />
              ))}
              <span
                className="absolute inset-y-0 w-1 bg-accent"
                style={{ left: `${timelinePercent(Math.min(now, len || now), len)}%` }}
              />
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="outline" disabled={!src || cannotPlay} onClick={toggle}>
                {playing ? <IconPauseVideo aria-hidden /> : <IconPlayVideo aria-hidden />}
                {playing ? "Pause" : "Play"}
              </Button>
              <Button size="sm" variant="outline" aria-label="Previous step" title="Previous step" onClick={() => moveStep(-1)}>
                <IconPreviousStep aria-hidden />
              </Button>
              <Button size="sm" variant="outline" aria-label="Next step" title="Next step" onClick={() => moveStep(1)}>
                <IconNextStep aria-hidden />
              </Button>
              <span className="text-xs text-muted">
                {clockText(now)} / {clockText(len)}
              </span>
              <div role="group" aria-label="Speed" className="flex items-center gap-1">
                {SPEEDS.map((r) => (
                  <Button
                    key={r}
                    size="sm"
                    variant="outline"
                    aria-pressed={rate === r}
                    className={cn(rate === r && "bg-accent/10")}
                    onClick={() => {
                      if (video.current) video.current.playbackRate = r;
                      setRate(r);
                    }}
                  >
                    {r}x
                  </Button>
                ))}
              </div>
              <Button size="sm" variant="outline" disabled={saving} className="ml-auto" onClick={onSave}>
                <IconExport aria-hidden />
                Save video
              </Button>
            </div>
            {!hasSteps && !hasMarks && <p className="text-xs text-faint">{NO_MARKERS}</p>}
          </div>
          {(hasSteps || hasMarks) && (
            <div className="min-h-0 min-w-0 space-y-2 overflow-auto">
              <h3 className="text-sm font-semibold text-text">{hasSteps ? "Steps" : "Actions"}</h3>
              {hasSteps ? (
                <ol aria-label="Steps" className="space-y-1">
                  {timeline.steps.map((s, i) => {
                    const isOpen = open.has(i);
                    return (
                      <li
                        key={`${s.at_ms}-${i}`}
                        ref={i === failedIdx ? failedItem : undefined}
                        className={cn("rounded", i === idx && "bg-accent/10")}
                      >
                        <div className="flex items-center gap-1">
                          <button
                            type="button"
                            aria-label={`${s.name} at ${clockText(s.at_ms)}${s.failed ? ", failed" : ""}`}
                            aria-current={i === idx ? "step" : undefined}
                            className={jumpClass}
                            onClick={() => seek(s.at_ms)}
                          >
                            <span className="min-w-0 flex-1 truncate">{s.name}</span>
                            <span className="text-xs text-faint">{clockText(s.at_ms)}</span>
                            {s.failed && <span className="text-xs font-medium text-danger">Failed</span>}
                          </button>
                          {s.actions.length > 0 && (
                            <Button
                              size="sm"
                              variant="ghost"
                              aria-label={`Actions of ${s.name}`}
                              aria-expanded={isOpen}
                              onClick={() => toggleOpen(i)}
                            >
                              {isOpen ? <IconHideActions aria-hidden /> : <IconShowActions aria-hidden />}
                            </Button>
                          )}
                        </div>
                        {isOpen && s.actions.length > 0 && (
                          <div className="pl-4">
                            <MarkList marks={s.actions} nowIndex={-1} current={undefined} onJump={seek} />
                          </div>
                        )}
                      </li>
                    );
                  })}
                </ol>
              ) : (
                <MarkList marks={timeline.marks} label="Actions" nowIndex={idx} current="true" onJump={seek} />
              )}
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
