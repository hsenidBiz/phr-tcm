import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, test } from "vitest";
import { Switch } from "../../../src/components/ui/switch";

// A switch whose value comes back from a save: the click only asks, and the
// parent changes `checked` later (or never, when the save fails).
function SavedSwitch({ answer }: { answer: { resolve?: (on: boolean) => void } }) {
  const [on, setOn] = useState(false);
  return (
    <Switch
      ariaLabel="Saved"
      checked={on}
      onCheckedChange={(next) => {
        answer.resolve = (ok) => setOn(ok ? next : on);
      }}
    />
  );
}

describe("Switch bounce", () => {
  test("does not bounce as it appears", () => {
    render(<Switch ariaLabel="Plain" checked onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Plain" }).className).not.toContain("is-init");
  });

  test("a saved switch waits for the new value before it bounces", () => {
    const answer: { resolve?: (on: boolean) => void } = {};
    render(<SavedSwitch answer={answer} />);
    const sw = screen.getByRole("switch", { name: "Saved" });
    fireEvent.click(sw);
    // Still off while the save runs: no bounce toward the old value.
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(sw.className).not.toContain("is-init");
    act(() => answer.resolve?.(true));
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(sw.className).toContain("is-init");
  });

  test("a save that fails never bounces", () => {
    const answer: { resolve?: (on: boolean) => void } = {};
    render(<SavedSwitch answer={answer} />);
    const sw = screen.getByRole("switch", { name: "Saved" });
    fireEvent.click(sw);
    act(() => answer.resolve?.(false));
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(sw.className).not.toContain("is-init");
  });

  test("a switch held in local state bounces on the click", () => {
    function Local() {
      const [on, setOn] = useState(false);
      return <Switch ariaLabel="Local" checked={on} onCheckedChange={setOn} />;
    }
    render(<Local />);
    const sw = screen.getByRole("switch", { name: "Local" });
    fireEvent.click(sw);
    expect(sw.className).toContain("is-init");
  });

  test("a change nobody clicked for does not bounce", () => {
    const { rerender } = render(<Switch ariaLabel="Pushed" checked={false} onCheckedChange={() => {}} />);
    rerender(<Switch ariaLabel="Pushed" checked onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Pushed" }).className).not.toContain("is-init");
  });
});
