// A Toaster whose toasts leave when the pointer reaches them.
//
// XiodUI's toaster, like sonner's before it, PAUSES a toast on hover so it
// can be read - right for a message worth reading and wrong for the runner,
// where the toast is a two-word confirmation ("Pasted screenshot") that
// lands on top of the outcome buttons. There, a hand moving toward the
// button is the signal that the toast has been seen: it goes, and the
// button under it is live again without waiting or dragging.

import { toast } from "../lib/toast";
import { Toaster } from "./ui/toaster";

export default function HoverDismissToaster() {
  return (
    // React synthesises mouseenter along the React tree, portals included,
    // so entering a toast (portalled to <body>) fires this even though the
    // wrapper itself has no size.
    <div onMouseEnter={() => toast.dismiss()}>
      <Toaster />
    </div>
  );
}
