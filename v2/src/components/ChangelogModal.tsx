// Post-update "What's new" dialog: shown once after an update installs,
// listing every version between the last one seen and the current one.
// Same layout rules as the app's other dialogs: pinned header/footer,
// only the body scrolls, so long changelogs never push it off-screen.
import { Sparkles } from "lucide-react";
import type { ChangelogEntry } from "../lib/changelog";
import { Button } from "./ui/button";

export default function ChangelogModal({
  entries,
  onClose,
}: {
  entries: ChangelogEntry[];
  onClose: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        className="flex max-h-[85vh] w-full max-w-md flex-col gap-4 rounded-lg border border-border bg-surface p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-center gap-2">
          <Sparkles size={16} className="text-accent" />
          <h2 className="text-sm font-semibold text-text">What's new</h2>
        </div>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-1">
          {entries.map((e) => (
            <section key={e.version} className="space-y-1.5">
              <h3 className="text-xs font-semibold text-text">
                Version {e.version}
                <span className="ml-2 font-normal text-faint">{e.date}</span>
              </h3>
              <ul className="list-disc space-y-1 pl-4 text-xs text-muted">
                {e.items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            </section>
          ))}
        </div>

        <div className="flex shrink-0 justify-end">
          <Button size="sm" onClick={onClose}>
            Got it
          </Button>
        </div>
      </div>
    </div>
  );
}
