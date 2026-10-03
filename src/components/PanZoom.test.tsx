// The flow maps' pan and zoom viewport, the Test map's mechanism: drag to
// pan, a plain wheel scrolls, Ctrl/Cmd + wheel zooms about the cursor, and
// Zoom out / Zoom in / Reset view with a live percentage.
//
// jsdom does no layout, so the viewport's size is mocked (clientWidth /
// clientHeight) and every position is read back from the layer's transform.

import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
  KEEP,
  MAX_SCALE,
  MIN_SCALE,
  PAD,
  PanZoomControls,
  PanZoomViewport,
  clampPan,
  fitView,
  usePanZoom,
  wheelPan,
  zoomAbout,
  type Size,
} from "./PanZoom";

// ---------------------------------------------------------------------------
// The pure maths.

describe("fitView", () => {
  test("fits the whole content, centred", () => {
    const v = fitView({ w: 1000, h: 400 }, { w: 500, h: 300 });
    expect(v.s).toBe(0.5);
    expect(v.tx).toBe(0);
    expect(v.ty).toBe((300 - 400 * 0.5) / 2);
  });

  test("never enlarges past 100%", () => {
    const v = fitView({ w: 200, h: 100 }, { w: 800, h: 400 });
    expect(v.s).toBe(1);
    expect(v.tx).toBe(300);
    expect(v.ty).toBe(150);
  });

  test("leaves the padding around the content", () => {
    const v = fitView({ w: 1136, h: 200 }, { w: 600, h: 300 }, 16);
    expect(v.s).toBe(0.5);
    expect(v.tx).toBe(16);
    expect(v.ty).toBe(100);
  });

  test("an unmeasured viewport leaves the content at 100%, top-left", () => {
    expect(fitView({ w: 500, h: 200 }, { w: 0, h: 0 })).toEqual({ s: 1, tx: 0, ty: 0 });
  });
});

describe("zoomAbout", () => {
  test("the point under the cursor stays under the cursor", () => {
    const before = { s: 0.8, tx: 30, ty: -20 };
    const [px, py] = [250, 120];
    const contentX = (px - before.tx) / before.s;
    const contentY = (py - before.ty) / before.s;
    const after = zoomAbout(before, before.s * 1.1, px, py);
    expect(after.s).toBeCloseTo(0.88);
    expect(after.tx + contentX * after.s).toBeCloseTo(px);
    expect(after.ty + contentY * after.s).toBeCloseTo(py);
  });

  test("clamps the scale to 20%-400%", () => {
    expect(zoomAbout({ s: 1, tx: 0, ty: 0 }, 99, 0, 0).s).toBe(MAX_SCALE);
    expect(zoomAbout({ s: 1, tx: 0, ty: 0 }, 0.01, 0, 0).s).toBe(MIN_SCALE);
    expect(MIN_SCALE).toBe(0.2);
    expect(MAX_SCALE).toBe(4);
  });
});

describe("clampPan", () => {
  const content: Size = { w: 1000, h: 400 };
  const viewport: Size = { w: 500, h: 300 };

  test("keeps at least 40px of the content in view", () => {
    expect(KEEP).toBe(40);
    const far = clampPan({ s: 1, tx: -5000, ty: -5000 }, content, viewport);
    expect(far.tx + 1000).toBe(KEEP);
    expect(far.ty + 400).toBe(KEEP);
    const near = clampPan({ s: 1, tx: 5000, ty: 5000 }, content, viewport);
    expect(near.tx).toBe(500 - KEEP);
    expect(near.ty).toBe(300 - KEEP);
  });

  test("leaves a position already in view alone", () => {
    expect(clampPan({ s: 1, tx: -100, ty: 20 }, content, viewport)).toEqual({ s: 1, tx: -100, ty: 20 });
  });
});

describe("wheelPan", () => {
  test("scrolls content larger than the viewport up to its edge, no further", () => {
    const v = { s: 1, tx: 0, ty: 0 };
    expect(wheelPan(v, 30, 50, { w: 1000, h: 400 }, { w: 500, h: 300 })).toEqual({ s: 1, tx: -30, ty: -50 });
    expect(wheelPan(v, 0, 900, { w: 1000, h: 400 }, { w: 500, h: 300 }).ty).toBe(300 - 400);
    // Already at the top: scrolling up again changes nothing.
    expect(wheelPan(v, 0, -50, { w: 1000, h: 400 }, { w: 500, h: 300 })).toEqual(v);
  });

  test("an axis that already fits does not scroll, so the page can", () => {
    const v = { s: 0.5, tx: 0, ty: 50 };
    expect(wheelPan(v, 0, 60, { w: 1000, h: 400 }, { w: 500, h: 300 })).toEqual(v);
  });
});

// ---------------------------------------------------------------------------
// The component, with a mocked viewport size.

const VIEWPORT = { w: 600, h: 300 };
// With 16px padding each side the room is 568 x 268, so this fits at 50%.
const CONTENT = { w: 1136, h: 200 };

function Harness({ onBox = () => {}, onButton = () => {} }: { onBox?: () => void; onButton?: () => void }) {
  const pz = usePanZoom(CONTENT);
  return (
    <div>
      <PanZoomControls pz={pz} />
      <PanZoomViewport pz={pz} label="Cycle map" testId="viewport">
        <div data-testid="box" style={{ position: "absolute", left: 0, top: 0, width: 200, height: 100 }} onClick={onBox}>
          <button onClick={onButton}>Open a template</button>
        </div>
      </PanZoomViewport>
    </div>
  );
}

function readView(): { s: number; tx: number; ty: number } {
  const layer = screen.getByTestId("viewport").firstElementChild as HTMLElement;
  const m = layer.style.transform.match(/translate\((-?[\d.e+-]+)px, (-?[\d.e+-]+)px\) scale\((-?[\d.e+-]+)\)/);
  if (!m) throw new Error(`unexpected transform: ${layer.style.transform}`);
  return { tx: Number(m[1]), ty: Number(m[2]), s: Number(m[3]) };
}

const percent = () => screen.getByText(/^\d+%$/);

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(VIEWPORT.w);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(VIEWPORT.h);
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("PanZoom", () => {
  test("starts fitted, with the content sized and transformed from its top-left", () => {
    render(<Harness />);
    const viewport = screen.getByRole("group", { name: "Cycle map" });
    expect(viewport).toHaveAttribute("tabindex", "0");
    expect(viewport.className).toContain("overflow-hidden");
    // min(content + padding, 60vh), never under 240px - min-height wins over max-height.
    expect(viewport.style.height).toBe(`${CONTENT.h + 2 * PAD}px`);
    expect(viewport.style.maxHeight).toBe("60vh");
    expect(viewport.style.minHeight).toBe("240px");

    const layer = viewport.firstElementChild as HTMLElement;
    expect(layer.style.width).toBe(`${CONTENT.w}px`);
    expect(layer.style.height).toBe(`${CONTENT.h}px`);
    expect(layer.style.transformOrigin).toBe("0 0");
    expect(readView()).toEqual({ s: 0.5, tx: 16, ty: 100 });

    expect(percent()).toHaveTextContent("50%");
    expect(percent()).toHaveAttribute("aria-live", "polite");
  });

  test("Zoom in and Zoom out step by 1.2 about the centre and clamp at 20% and 400%", () => {
    render(<Harness />);
    const zoomIn = screen.getByRole("button", { name: "Zoom in" });
    const zoomOut = screen.getByRole("button", { name: "Zoom out" });

    fireEvent.click(zoomIn);
    expect(percent()).toHaveTextContent("60%");
    // About the viewport's centre: the content point there stays there.
    const v = readView();
    expect((300 - v.tx) / v.s).toBeCloseTo((300 - 16) / 0.5);

    fireEvent.click(zoomOut);
    fireEvent.click(zoomOut);
    expect(percent()).toHaveTextContent("42%");

    for (let i = 0; i < 20; i++) fireEvent.click(zoomOut);
    expect(percent()).toHaveTextContent("20%");
    for (let i = 0; i < 30; i++) fireEvent.click(zoomIn);
    expect(percent()).toHaveTextContent("400%");
  });

  test("Reset view returns to the fit", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    fireEvent.keyDown(screen.getByTestId("viewport"), { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Reset view" }));
    expect(readView()).toEqual({ s: 0.5, tx: 16, ty: 100 });
    expect(percent()).toHaveTextContent("50%");
  });

  test("Ctrl+wheel zooms by 1.1 about the cursor, and holds the page still", () => {
    render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    const before = readView();
    const [cx, cy] = [100, 150];
    const notPrevented = fireEvent.wheel(viewport, { deltaY: -100, ctrlKey: true, clientX: cx, clientY: cy });
    expect(notPrevented).toBe(false);
    const after = readView();
    expect(after.s).toBeCloseTo(0.55);
    expect((cx - after.tx) / after.s).toBeCloseTo((cx - before.tx) / before.s);
    expect((cy - after.ty) / after.s).toBeCloseTo((cy - before.ty) / before.s);

    fireEvent.wheel(viewport, { deltaY: 100, metaKey: true, clientX: cx, clientY: cy });
    expect(readView().s).toBeCloseTo(0.5);
  });

  test("a plain wheel pans once the map is larger than its box, and lets the page scroll when it cannot", () => {
    render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    // Fitted, it already shows everything: the wheel is the page's.
    expect(fireEvent.wheel(viewport, { deltaY: 40 })).toBe(true);
    expect(readView()).toEqual({ s: 0.5, tx: 16, ty: 100 });

    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    const before = readView();
    expect(fireEvent.wheel(viewport, { deltaX: 30 })).toBe(false);
    expect(readView().tx).toBeCloseTo(before.tx - 30);
    expect(readView().s).toBe(before.s);
  });

  test("a press on a button does not pan, and its click still fires", () => {
    const onButton = vi.fn();
    render(<Harness onButton={onButton} />);
    const viewport = screen.getByTestId("viewport");
    const button = screen.getByRole("button", { name: "Open a template" });
    const before = readView();

    fireEvent.pointerDown(button, { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(viewport, { pointerId: 1, clientX: 60, clientY: 40 });
    fireEvent.pointerUp(viewport, { pointerId: 1, clientX: 60, clientY: 40 });
    fireEvent.click(button);

    expect(readView()).toEqual(before);
    expect(onButton).toHaveBeenCalledTimes(1);
  });

  test("a drag pans, shows the grabbing cursor, and swallows the click that ends it", () => {
    const onBox = vi.fn();
    render(<Harness onBox={onBox} />);
    const viewport = screen.getByTestId("viewport");
    const box = screen.getByTestId("box");
    const before = readView();
    expect(viewport.className).toContain("cursor-grab");

    fireEvent.pointerDown(box, { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(viewport, { pointerId: 1, clientX: 30, clientY: 25 });
    expect(viewport.className).toContain("cursor-grabbing");
    expect(readView()).toEqual({ ...before, tx: before.tx + 20, ty: before.ty + 15 });
    fireEvent.pointerUp(viewport, { pointerId: 1, clientX: 30, clientY: 25 });
    fireEvent.click(box);

    expect(onBox).not.toHaveBeenCalled();
    expect(viewport.className).not.toContain("cursor-grabbing");
  });

  test("a press that moves 3px or less is still a click", () => {
    const onBox = vi.fn();
    render(<Harness onBox={onBox} />);
    const viewport = screen.getByTestId("viewport");
    const box = screen.getByTestId("box");
    const before = readView();

    fireEvent.pointerDown(box, { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(viewport, { pointerId: 1, clientX: 12, clientY: 12 });
    fireEvent.pointerUp(viewport, { pointerId: 1, clientX: 12, clientY: 12 });
    fireEvent.click(box);

    expect(readView()).toEqual(before);
    expect(onBox).toHaveBeenCalledTimes(1);
  });

  test("dragging cannot carry the content out of view", () => {
    render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    fireEvent.pointerDown(viewport, { button: 0, pointerId: 1, clientX: 300, clientY: 150 });
    fireEvent.pointerMove(viewport, { pointerId: 1, clientX: -5000, clientY: -5000 });
    const v = readView();
    expect(v.tx + CONTENT.w * v.s).toBe(KEEP);
    expect(v.ty + CONTENT.h * v.s).toBe(KEEP);

    fireEvent.pointerMove(viewport, { pointerId: 1, clientX: 5000, clientY: 5000 });
    expect(readView()).toEqual({ s: 0.5, tx: VIEWPORT.w - KEEP, ty: VIEWPORT.h - KEEP });
    fireEvent.pointerUp(viewport, { pointerId: 1 });
  });

  test("the keys pan and zoom when the viewport has focus", () => {
    render(<Harness />);
    const viewport = screen.getByTestId("viewport");
    viewport.focus();

    fireEvent.keyDown(viewport, { key: "ArrowRight" });
    fireEvent.keyDown(viewport, { key: "ArrowDown" });
    expect(readView()).toEqual({ s: 0.5, tx: 16 - 40, ty: 100 - 40 });
    fireEvent.keyDown(viewport, { key: "ArrowLeft" });
    fireEvent.keyDown(viewport, { key: "ArrowUp" });
    expect(readView()).toEqual({ s: 0.5, tx: 16, ty: 100 });

    fireEvent.keyDown(viewport, { key: "+" });
    expect(percent()).toHaveTextContent("60%");
    fireEvent.keyDown(viewport, { key: "=" });
    expect(percent()).toHaveTextContent("72%");
    fireEvent.keyDown(viewport, { key: "-" });
    expect(percent()).toHaveTextContent("60%");
    fireEvent.keyDown(viewport, { key: "0" });
    expect(readView()).toEqual({ s: 0.5, tx: 16, ty: 100 });
  });

  test("keys pressed on a button inside the map are the button's, not the map's", () => {
    render(<Harness />);
    const before = readView();
    fireEvent.keyDown(screen.getByRole("button", { name: "Open a template" }), { key: "ArrowRight" });
    expect(readView()).toEqual(before);
  });
});
