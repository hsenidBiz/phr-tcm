import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { toast } from "sonner";
import { expect, test } from "vitest";
import HoverDismissToaster from "./HoverDismissToaster";

/// Sonner pauses a toast on hover; this Toaster does the opposite. The
/// runner's "Pasted screenshot" lands on the outcome buttons, so the
/// pointer arriving is the cue to get out of the way.
test("a toast leaves when the pointer reaches it", async () => {
  render(<HoverDismissToaster position="bottom-right" />);
  toast.success("Pasted screenshot");
  const el = await screen.findByText("Pasted screenshot");

  fireEvent.mouseEnter(el);
  await waitFor(() => expect(screen.queryByText("Pasted screenshot")).not.toBeInTheDocument());
});
