// The Setup panel follows the scroll, eased, inside its own column. jsdom
// does no layout, so the geometry is tested as the pure function the hook
// calls each frame; the component is tested for which way it goes.

import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { clampOffset, FOLLOW_EASE, FOLLOW_GAP, followOffset, followRoom, stepToward } from "./followScroll";
import SetupColumn from "./SetupColumn";

afterEach(() => {
  vi.restoreAllMocks();
});

const view = { viewTop: 100, viewBottom: 900 };

test("at the top of the page the panel stays in its own place", () => {
  // The column starts below the top of the scrolling area: no offset.
  expect(followOffset({ ...view, columnTop: 300, columnHeight: 2000, panelHeight: 400 })).toBe(0);
});

test("scrolled down, it rests a gap below the top of the scrolling area", () => {
  // The column's top has scrolled 500px above the area's top.
  expect(followOffset({ ...view, columnTop: -400, columnHeight: 3000, panelHeight: 400 })).toBe(
    100 + FOLLOW_GAP + 400,
  );
});

test("it never goes past the bottom of its column", () => {
  expect(followOffset({ ...view, columnTop: -5000, columnHeight: 3000, panelHeight: 400 })).toBe(2600);
});

test("it never goes above its column, so never over the header", () => {
  expect(followOffset({ ...view, columnTop: 5000, columnHeight: 3000, panelHeight: 400 })).toBe(0);
  expect(clampOffset(-50, 1000, 200)).toBe(0);
});

test("a column no taller than the panel (the panel above the list) never moves", () => {
  expect(followOffset({ ...view, columnTop: -400, columnHeight: 400, panelHeight: 400 })).toBe(0);
  expect(clampOffset(300, 300, 400)).toBe(0);
});

test("a panel taller than the scrolling area keeps its bottom in view", () => {
  const g = { ...view, columnTop: -1000, columnHeight: 4000, panelHeight: 1200 };
  const offset = followOffset(g);
  // The panel's bottom sits a gap above the area's bottom.
  expect(g.columnTop + offset + g.panelHeight).toBe(view.viewBottom - FOLLOW_GAP);
});

test("the room a panel may take is the scrolling area less the gap above and below", () => {
  expect(followRoom(view)).toBe(800 - 2 * FOLLOW_GAP);
  // A window shorter than the two gaps leaves no room, never a negative one.
  expect(followRoom({ viewTop: 0, viewBottom: FOLLOW_GAP })).toBe(0);
});

test("a panel held to the room is never the too-tall case: its top stays in view", () => {
  const g = { ...view, columnTop: -1000, columnHeight: 4000, panelHeight: followRoom(view) };
  const offset = followOffset(g);
  // It rests a gap below the area's top, so its header row is in view.
  expect(g.columnTop + offset).toBe(view.viewTop + FOLLOW_GAP);
});

test("each frame covers a share of the distance, then settles exactly", () => {
  expect(stepToward(0, 100)).toBeCloseTo(100 * FOLLOW_EASE);
  expect(stepToward(100, 0)).toBeCloseTo(100 - 100 * FOLLOW_EASE);
  expect(stepToward(99.8, 100)).toBe(100);
  let at = 0;
  let frames = 0;
  while (at !== 500 && frames < 200) {
    at = stepToward(at, 500);
    frames += 1;
  }
  expect(at).toBe(500);
  // Seen catching up, but there within about a second at 60 frames.
  expect(frames).toBeGreaterThan(10);
  expect(frames).toBeLessThan(70);
});

function motion(reduce: boolean) {
  vi.spyOn(window, "matchMedia").mockImplementation(
    (q: string) =>
      ({
        matches: reduce && q.includes("prefers-reduced-motion"),
        media: q,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList,
  );
}

test("under reduced motion the panel is sticky and never animated", () => {
  motion(true);
  const raf = vi.spyOn(window, "requestAnimationFrame");
  const { container } = render(
    <SetupColumn>
      <p>Setup</p>
    </SetupColumn>,
  );
  const column = container.firstElementChild as HTMLElement;
  expect(column.dataset.follow).toBe("sticky");
  const box = screen.getByText("Setup").parentElement as HTMLElement;
  expect(box.className).toContain("sticky");
  expect(box.style.transform).toBe("");
  window.dispatchEvent(new Event("scroll"));
  expect(raf).not.toHaveBeenCalled();
});

test("with motion the panel follows by a transform, never sticky", () => {
  motion(false);
  const { container } = render(
    <SetupColumn>
      <p>Setup</p>
    </SetupColumn>,
  );
  const column = container.firstElementChild as HTMLElement;
  expect(column.dataset.follow).toBe("eased");
  const box = screen.getByText("Setup").parentElement as HTMLElement;
  expect(box.className).not.toContain("sticky");
});

test("the panel is told how much room it has, so its card can be held to it", () => {
  for (const reduce of [false, true]) {
    motion(reduce);
    window.innerHeight = 700;
    const { unmount } = render(
      <SetupColumn>
        <p>Setup</p>
      </SetupColumn>,
    );
    const box = screen.getByText("Setup").parentElement as HTMLElement;
    // No scrolling ancestor in jsdom: the window is the area.
    expect(box.style.getPropertyValue("--follow-room")).toBe(`${700 - 2 * FOLLOW_GAP}px`);
    unmount();
  }
});
