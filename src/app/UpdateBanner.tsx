import { formatByteProgress } from "../lib/bytes";
import { IconRefresh } from "../lib/actionIcons";
import { Button } from "../components/ui/button";
import { SpinHolder } from "../components/ui/spin-holder";

type Props = {
  /** The version on offer, from the release check. */
  available: string;
  /** The last click on the button did not finish. */
  failedAttempt: boolean;
  /** Download and install is running. */
  applying: boolean;
  /** Download progress; null until the first event. */
  dl: { percent: number; downloaded: number; total: number } | null;
  onApply: () => void;
};

/** The "a new version is available" bar under the title bar. */
export default function UpdateBanner({ available, failedAttempt, applying, dl, onApply }: Props) {
  return (
    <div className="border-b border-accent/40 bg-accent-soft px-6 py-2 text-sm">
      {/* One row either way: the version line makes the offer, and
                  once the button is clicked the progress bar takes its slot -
                  the banner never grows a second row mid-download. */}
      <div className="flex items-center justify-between gap-4">
        {applying ? (
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <div
              role="progressbar"
              aria-label={`Downloading version ${available}`}
              aria-valuemin={0}
              aria-valuemax={100}
              // Omitted, not zero, until the first event: an indeterminate
              // bar is what "we do not know yet" means to a screen reader,
              // and 0% would be a claim.
              aria-valuenow={dl ? dl.percent : undefined}
              aria-valuetext={dl ? formatByteProgress(dl.downloaded, dl.total) : undefined}
              className="h-1.5 flex-1 overflow-hidden rounded-full bg-accent/20"
            >
              <div
                className="h-full rounded-full bg-accent transition-[width] duration-300 ease-out"
                style={{ width: `${dl?.percent ?? 0}%` }}
              />
            </div>
            {/* Tabular figures: the numerator changes every few seconds
                        and proportional digits make the whole line jitter. */}
            <span className="shrink-0 tabular-nums text-xs text-muted">
              {!dl
                ? "Preparing…"
                : dl.percent >= 100
                  ? "Installing…"
                  : dl.total > 0
                    ? formatByteProgress(dl.downloaded, dl.total)
                    : `${dl.percent}%`}
            </span>
          </div>
        ) : (
          <span>
            {failedAttempt ? (
              // The last click on this button did NOT work: the app
              // restarted still on the old version because Update.exe
              // could not swap the install folder while another
              // program sat in it. Saying so beats the banner
              // silently reappearing and looking like it did nothing.
              <>
                The last update couldn't finish - another program was using the app's files
                (usually a browser window that was opened from this app). Close your browser
                windows and try again, or restart Windows.
              </>
            ) : (
              <>Version {available} is available.</>
            )}
          </span>
        )}
        <Button size="sm" disabled={applying} onClick={onApply}>
          <SpinHolder spinning={applying}>
            <IconRefresh aria-hidden />
          </SpinHolder>
          {applying ? "Updating" : "Restart to update"}
        </Button>
      </div>
    </div>
  );
}
