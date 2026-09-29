/**
 * The flow page's connector script (src-tauri/web/flow-page.js). The
 * browser lays the stages out, so the script reads where each box landed
 * and draws the curves between them. jsdom does no layout, so the boxes'
 * rectangles are given here.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, expect, test } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(resolve(here, "../../src-tauri/web/flow-page.js"), "utf8");

type Rect = { left: number; right: number; top: number; bottom: number };
type FlowPage = { wire: (a: Rect, b: Rect) => string; draw: () => void };
const page = () => (window as unknown as { FlowPage: FlowPage }).FlowPage;

beforeAll(() => {
  new Function(src)();
});

function placeAt(el: Element, r: Rect) {
  (el as HTMLElement).getBoundingClientRect = () =>
    ({ ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON: () => r }) as DOMRect;
}

test("a connector leaves a box's right edge and reaches the next box's left edge, flat at both ends", () => {
  const d = page().wire({ left: 0, right: 100, top: 0, bottom: 40 }, { left: 200, right: 300, top: 100, bottom: 140 });
  expect(d).toBe("M100 20 C150 20 150 120 200 120");
});

test("draw places every connector and its colour blend where the boxes landed", () => {
  document.body.innerHTML = `
    <div id="canvas">
      <svg><defs><linearGradient id="g0" x1="0" x2="1"></linearGradient></defs>
        <g class="edge" data-from="setup" data-to="rules" data-grad="g0"><path class="wire"></path><path class="spark"></path></g>
        <g class="edge" data-from="setup" data-to="gone" data-grad="g1"><path class="wire" d=""></path></g>
      </svg>
      <div class="stage" data-stage="setup"></div>
      <div class="stage" data-stage="rules"></div>
    </div>`;
  const canvas = document.getElementById("canvas")!;
  placeAt(canvas, { left: 10, right: 1010, top: 50, bottom: 450 });
  // A wrapped title made "rules" taller than "setup".
  placeAt(document.querySelector("[data-stage=setup]")!, { left: 10, right: 270, top: 150, bottom: 230 });
  placeAt(document.querySelector("[data-stage=rules]")!, { left: 366, right: 626, top: 130, bottom: 290 });

  page().draw();

  const [wire, spark] = Array.from(document.querySelectorAll("g[data-to=rules] path"));
  // Relative to the canvas: setup's right edge at 260, mid-height 140; rules' left at 356, mid-height 160.
  expect(wire.getAttribute("d")).toBe("M260 140 C308 140 308 160 356 160");
  expect(spark.getAttribute("d")).toBe(wire.getAttribute("d"));
  const grad = document.getElementById("g0")!;
  expect([grad.getAttribute("x1"), grad.getAttribute("x2")]).toEqual(["260", "356"]);
  // A connector to a stage that is not on the page is left undrawn.
  expect(document.querySelector("g[data-to=gone] path")!.getAttribute("d")).toBe("");
});
