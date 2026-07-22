// Post-update "What's new" dialog: shown once after an update installs,
// listing every version between the last one seen and the current one.
// Built on Astryx Dialog (focus trap, Escape, aria-modal) inside an
// AstryxIsland so it renders in the app's theme; the content and footer
// stay on our own primitives.
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import type { ChangelogEntry } from "../lib/changelog";
import AstryxIsland from "./AstryxIsland";
import { Button } from "./ui/button";

export default function ChangelogModal({
  entries,
  onClose,
}: {
  entries: ChangelogEntry[];
  onClose: () => void;
}) {
  return (
    <AstryxIsland>
      <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440}>
        <Layout
          header={<DialogHeader title="What's new" onOpenChange={(open) => !open && onClose()} />}
          content={
            <LayoutContent>
              <div className="space-y-4">
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
            </LayoutContent>
          }
          footer={
            <LayoutFooter hasDivider>
              <Button size="sm" onClick={onClose}>
                Got it
              </Button>
            </LayoutFooter>
          }
        />
      </Dialog>
    </AstryxIsland>
  );
}
