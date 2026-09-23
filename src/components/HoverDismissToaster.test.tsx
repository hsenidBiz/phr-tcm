import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test } from "vitest";
import { toast } from "../lib/toast";
import HoverDismissToaster from "./HoverDismissToaster";

/// The app's toaster pauses a toast on hover; this one does the opposite.
/// The runner's "Pasted screenshot" lands on the outcome buttons, so the
/// pointer arriving is the cue to get out of the way.
test("a toast leaves when the pointer reaches it", async () => {
  render(<HoverDismissToaster />);
  act(() => {
    toast.success("Pasted screenshot");
  });
  const el = await screen.findByText("Pasted screenshot");

  fireEvent.mouseEnter(el);
  await waitFor(() => expect(screen.queryByText("Pasted screenshot")).not.toBeInTheDocument());
});
