// Auto Test's own settings: the page every browser shows, and how many
// browsers can be open together. Both live in Rust's app settings; the rows
// are the same ones Settings used to hold (the page size) and a new one.

import BrowserLimitSettings from "../../components/BrowserLimitSettings";
import PageSizeSettings from "../../components/PageSizeSettings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel } from "../../lib/actionIcons";

export default function AutoRunSettingsDialog({ onClose }: { onClose: () => void }) {
  return (
    <Modal onClose={onClose} className="flex w-full max-w-2xl flex-col gap-1 p-5">
      <h2 className="text-sm font-semibold text-text">Auto Test settings</h2>
      <div className="divide-y divide-border">
        <PageSizeSettings />
        <BrowserLimitSettings />
      </div>
      <div className="flex justify-end pt-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
