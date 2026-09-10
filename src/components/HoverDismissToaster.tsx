// A Toaster whose toasts leave when the pointer reaches them.
//
// Sonner's default is the opposite - hovering PAUSES a toast so it can be
// read - which is right for a message worth reading and wrong for the
// runner, where the toast is a two-word confirmation ("Pasted screenshot")
// that lands on top of the outcome buttons. There, a hand moving toward
// the button is the signal that the toast has been seen: it goes, and the
// button under it is live again without waiting or dragging.

import { Toaster, toast, type ToasterProps } from "sonner";

export default function HoverDismissToaster(props: ToasterProps) {
  return (
    // React synthesises mouseenter for the subtree, so entering the toast
    // (a fixed-position descendant) fires this even though the wrapper
    // itself has no size.
    <div onMouseEnter={() => toast.dismiss()}>
      <Toaster {...props} />
    </div>
  );
}
