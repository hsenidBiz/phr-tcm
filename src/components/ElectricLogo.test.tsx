import { render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ElectricLogo, { place } from "./ElectricLogo";

afterEach(() => {
  vi.restoreAllMocks();
});

// The Modules board: an 800x750 picture, rasterised by the trace at 560x525
// (its longer side is 560), drawn into an 800x750 container.
const BOARD = { w: 800, h: 750 };
const RASTER = { w: 560, h: 525 };
const k = RASTER.w / BOARD.w;

/** A traced shape as traceShape builds it: the part of the raster that holds
 * the drawing (from `left`,`top`, `w` by `h`), padded by a quarter of its
 * longer side plus 2. */
function traced(left: number, top: number, w: number, h: number) {
  return {
    left,
    top,
    logoWidth: w,
    logoHeight: h,
    pad: Math.ceil(Math.max(w, h) * 0.25) + 2,
    rasterWidth: RASTER.w,
    rasterHeight: RASTER.h,
  };
}

/** Where a board point lands on screen: board -> raster -> the traced
 * shape's field (texel centres sit at +0.5, the same on both sides) -> px. */
function onScreen(shape: ReturnType<typeof traced>, fitTo: "shape" | "image", bx: number, by: number) {
  const p = place(shape, { scale: 1, fitTo }, BOARD.w, BOARD.h);
  const fx = bx * k - shape.left + shape.pad;
  const fy = by * k - shape.top + shape.pad;
  return { x: p.ox + fx * p.fit, y: p.oy + fy * p.fit };
}

const POINTS = [
  [0, 0],
  [400, 166],
  [230, 375],
  [150, 565],
  [800, 750],
];

test("fitTo image maps every point of the picture onto the same board point, whatever part is drawn", () => {
  for (const shape of [traced(0, 0, 560, 525), traced(161, 116, 92, 21), traced(105, 262, 60, 134), traced(300, 200, 7, 180)]) {
    for (const [bx, by] of POINTS) {
      const at = onScreen(shape, "image", bx, by);
      expect(at.x).toBeCloseTo(bx, 6);
      expect(at.y).toBeCloseTo(by, 6);
    }
  }
});

test("fitTo image keeps the strands' scale fixed as the drawing changes", () => {
  const a = place(traced(0, 0, 560, 525), { scale: 1, fitTo: "image" }, BOARD.w, BOARD.h);
  const b = place(traced(161, 116, 92, 21), { scale: 1, fitTo: "image" }, BOARD.w, BOARD.h);
  expect(a.fit).toBeCloseTo(BOARD.w / RASTER.w, 9);
  expect(b.fit).toBeCloseTo(a.fit, 9);
  expect(b.unit).toBeCloseTo(a.unit, 9);
  expect(a.unit).toBeCloseTo(8, 9); // the board's longer side over 100
});

test("fitTo image centres a picture whose shape differs from the container's", () => {
  // An 800x750 picture in an 800x800 container: fitted by width, centred down.
  const p = place(traced(0, 0, 560, 525), { scale: 1, fitTo: "image" }, 800, 800);
  const pad = traced(0, 0, 560, 525).pad;
  expect(p.fit).toBeCloseTo(800 / 560, 9);
  expect(p.ox + pad * p.fit).toBeCloseTo(0, 6);
  expect(p.oy + pad * p.fit).toBeCloseTo(25, 6);
});

test("the original fitTo shape fills the box with the drawing, which is why the board cannot use it", () => {
  const part = traced(161, 116, 92, 21);
  const at = onScreen(part, "shape", 230 + 0, 166 + 0);
  expect(Math.abs(at.x - 230) + Math.abs(at.y - 166)).toBeGreaterThan(50);
  // Unchanged from the original: the drawing is centred at `scale` of the box.
  const p = place(part, { scale: 0.7, fitTo: "shape" }, 400, 300);
  expect(p.fit).toBeCloseTo(Math.min((400 * 0.7) / 92, (300 * 0.7) / 21), 9);
  expect(p.ox + (part.pad + 46) * p.fit).toBeCloseTo(200, 6);
});

test("without WebGL it renders an empty container and never throws", () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const { container, unmount } = render(
    <ElectricLogo src="data:image/svg+xml;charset=utf-8,%3Csvg%2F%3E" color="" glowColor="" />,
  );
  const box = container.querySelector(".electric-logo")!;
  expect(box).toBeInTheDocument();
  expect(box.querySelector("canvas")).toBeNull();
  expect(() => unmount()).not.toThrow();
});
