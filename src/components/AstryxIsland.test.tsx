import { act, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import AstryxIsland from "./AstryxIsland";

afterEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.classList.remove("dark");
});

/** Regression: Astryx's root Theme syncs data-theme onto <html> (sets
 * light/dark at mount, REMOVES it at unmount) - our theme attribute. An
 * island mounting/unmounting must never knock the app back to Slate. */
test("an island preserves the app's data-theme through mount AND unmount", async () => {
  localStorage.setItem("tcm-v2-theme-id", "oled");
  document.documentElement.setAttribute("data-theme", "oled");

  const { unmount } = render(
    <AstryxIsland>
      <span>island content</span>
    </AstryxIsland>,
  );
  // Mount: their layout effect set data-theme="dark"; ours re-asserted after.
  expect(document.documentElement.getAttribute("data-theme")).toBe("oled");

  unmount();
  // Unmount: their cleanup removed the attribute; our microtask restores it.
  await act(async () => {
    await Promise.resolve();
  });
  expect(document.documentElement.getAttribute("data-theme")).toBe("oled");
});
