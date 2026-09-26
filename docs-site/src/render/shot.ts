// The centrepiece: a screenshot with a numbered marker on every documented
// control. Activating a control dims the rest of the shot, rings the
// control in the accent and shows its caption beside it.
//
// Positions are in the shot's own pixels (its capture size: the main
// window's SHOT_WIDTH x SHOT_HEIGHT, or a smaller window's own size) and are
// laid out as percentages, so they scale with the rendered image at any
// width. A shot narrower than the main window is framed as that narrow
// window, centred, never stretched to the column.

import { SHOT_HEIGHT, SHOT_WIDTH, shotSize, type Box, type Control, type Shot, type ShotPositions, type Size } from "../types";
import { h, plain, rich } from "./dom";
import { shotSrc, type Theme } from "./theme";

const MAIN: Size = { w: SHOT_WIDTH, h: SHOT_HEIGHT };

/** Controls this close (shot px) to the frame's left or top edge get their
 *  marker on the opposite corner, so it is never cut in half by the frame. */
const EDGE = 24;
/** Gap between a control and its caption, and the caption's minimum
 *  distance from the frame's edges (frame px). */
const CAPTION_GAP = 14;
const CAPTION_MARGIN = 8;

export type ShotView = {
  figure: HTMLElement;
  /** True when this control has a marker on the shot. */
  has(controlId: string): boolean;
  /** Transient activation (hover / focus); null clears it. */
  hover(controlId: string | null): void;
  /** Sticky activation (a click, a search jump, a deep link); null clears it. */
  pin(controlId: string | null): void;
};

const pct = (n: number, of: number) => `${+((n / of) * 100).toFixed(4)}%`;

export function renderShot(opts: {
  shot: Shot;
  controls: { control: Control; n: number }[];
  /** The shot's positions.json entry, if it has one. */
  placed: ShotPositions | undefined;
  available: boolean;
  theme: Theme;
  motion: boolean;
  onActive: (controlId: string | null) => void;
  onMarkerClick: (controlId: string) => void;
}): ShotView {
  const { shot, controls, placed, available, theme, motion } = opts;
  const size = shotSize(shot);
  // Boxes are in the pixels they were measured at.
  const at = placed?.size ?? size;
  const boxes = placed?.controls;

  const media = available
    ? h("img", {
        "data-shot": shot.id,
        src: shotSrc(theme, shot.id),
        alt: shot.alt,
        width: size.w,
        height: size.h,
        loading: "lazy",
        decoding: "async",
      })
    : placeholder(shot.alt);

  const spot = h("div", { class: "spot", "aria-hidden": "true" });
  const caption = h("div", { class: "caption", "aria-hidden": "true" });
  const layer = h("div", { class: "annotations" }, spot);

  const markers = new Map<string, { el: HTMLElement; box: Box; control: Control; n: number }>();
  for (const { control, n } of controls) {
    const box = boxes?.[control.id];
    if (!box) continue; // no position yet: the list row still documents it
    // Anchored on the control's top-left corner, or the corner away from
    // the frame edge it touches (the nav rail, the top bar). CSS nudges it
    // outward from that corner and clamps it inside the frame.
    const right = box.x < EDGE;
    const below = box.y < EDGE;
    const el = h(
      "span",
      {
        class: motion ? "marker pulse" : "marker",
        "data-control": control.id,
        "data-anchor": `${below ? "b" : "t"}${right ? "r" : "l"}`,
        "aria-hidden": "true",
        title: plain(control.name),
      },
      String(n),
    );
    el.style.setProperty("--x", pct(right ? box.x + box.w : box.x, at.w));
    el.style.setProperty("--y", pct(below ? box.y + box.h : box.y, at.h));
    el.style.setProperty("--i", String(n));
    el.addEventListener("mouseenter", () => hover(control.id));
    el.addEventListener("mouseleave", () => hover(null));
    el.addEventListener("click", () => opts.onMarkerClick(control.id));
    markers.set(control.id, { el, box, control, n });
    layer.appendChild(el);
  }
  layer.appendChild(caption);

  const narrow = size.w < SHOT_WIDTH;
  const frame = h("div", { class: narrow ? "frame is-narrow" : "frame" }, media, layer);
  // Read by the stylesheet: the frame's aspect ratio, and a narrow
  // window's width cap (never wider than it really is).
  frame.style.setProperty("--shot-w", String(size.w));
  frame.style.setProperty("--shot-h", String(size.h));
  const figure = h("figure", { class: available ? "shot" : "shot is-placeholder", "data-shot": shot.id }, frame);

  let hovered: string | null = null;
  let pinned: string | null = null;
  let shown: string | null = null;

  function show() {
    const id = hovered ?? pinned;
    const m = id ? markers.get(id) : undefined;
    const next = m ? id : null;
    if (next === shown) return;
    if (shown) markers.get(shown)?.el.removeAttribute("data-active");
    shown = next;
    if (!m || !next) {
      delete figure.dataset.active;
      opts.onActive(null);
      return;
    }
    m.el.setAttribute("data-active", "");
    figure.dataset.active = next;
    Object.assign(spot.style, spotStyle(m.box, at));
    fillCaption(caption, m.control, m.n);
    placeCaption();
    opts.onActive(next);
  }

  /** Measures the caption against the frame and keeps it inside. */
  function placeCaption() {
    const m = shown ? markers.get(shown) : undefined;
    if (!m) return;
    const p = captionPlacement(
      m.box,
      { w: frame.clientWidth, h: frame.clientHeight },
      { w: caption.offsetWidth, h: caption.offsetHeight },
      at,
    );
    caption.style.left = `${p.left}px`;
    caption.style.top = `${p.top}px`;
    caption.dataset.side = p.side;
  }
  if (typeof ResizeObserver !== "undefined") new ResizeObserver(placeCaption).observe(frame);

  function hover(id: string | null) {
    hovered = id;
    show();
  }

  return {
    figure,
    has: (id) => markers.has(id),
    hover,
    pin(id) {
      pinned = id;
      show();
    },
  };
}

/** The spotlight ring: the control's box plus a small margin, in
 *  percentages of the shot (of size `shot`). */
export function spotStyle(b: Box, shot: Size = MAIN): { left: string; top: string; width: string; height: string } {
  const pad = 6;
  return {
    left: `calc(${pct(b.x, shot.w)} - ${pad}px)`,
    top: `calc(${pct(b.y, shot.h)} - ${pad}px)`,
    width: `calc(${pct(b.w, shot.w)} + ${pad * 2}px)`,
    height: `calc(${pct(b.h, shot.h)} + ${pad * 2}px)`,
  };
}

function fillCaption(caption: HTMLElement, c: Control, n: number) {
  caption.replaceChildren(
    h("span", { class: "caption-num" }, String(n)),
    h(
      "span",
      { class: "caption-body" },
      h("strong", { class: "caption-name" }, rich(c.name)),
      h("span", { class: "caption-does" }, rich(c.does)),
    ),
  );
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(v, Math.max(lo, hi)));

/**
 * Where a caption of size `cap` goes for control box `b` (px of a shot of
 * size `shot`) in a frame of size `frame` (frame px): centred on the control, below it when it
 * fits (else above, else whichever side has more room), always shifted to
 * stay inside the frame.
 */
export function captionPlacement(
  b: Box,
  frame: Size,
  cap: Size,
  shot: Size = MAIN,
): { left: number; top: number; side: "above" | "below" } {
  const sx = frame.w / shot.w;
  const sy = frame.h / shot.h;
  const top = b.y * sy;
  const bottom = (b.y + b.h) * sy;
  const centre = (b.x + b.w / 2) * sx;

  const roomBelow = frame.h - bottom - CAPTION_GAP - CAPTION_MARGIN;
  const roomAbove = top - CAPTION_GAP - CAPTION_MARGIN;
  const side = cap.h <= roomBelow || (cap.h > roomAbove && roomBelow >= roomAbove) ? "below" : "above";
  const y = side === "below" ? bottom + CAPTION_GAP : top - CAPTION_GAP - cap.h;

  return {
    left: clamp(centre - cap.w / 2, CAPTION_MARGIN, frame.w - cap.w - CAPTION_MARGIN),
    top: clamp(y, CAPTION_MARGIN, frame.h - cap.h - CAPTION_MARGIN),
    side,
  };
}

/** A quiet wireframe of the app window that holds the place of a shot. */
export function placeholder(label: string): HTMLElement {
  const bar = (w: number) => h("i", { class: "sk-line", style: `--w:${w}%` });
  const card = (lines: number[]) => h("div", { class: "sk-card" }, ...lines.map(bar));
  return h(
    "div",
    { class: "placeholder", role: "img", "aria-label": label },
    h(
      "div",
      { class: "sk-rail" },
      h("i", { class: "sk-logo" }),
      ...[0, 1, 2, 3, 4, 5].map((i) => h("i", { class: i === 1 ? "sk-nav is-on" : "sk-nav" })),
    ),
    h(
      "div",
      { class: "sk-main" },
      h("div", { class: "sk-top" }, bar(18), bar(12), h("i", { class: "sk-pill" })),
      h("div", { class: "sk-head" }, h("i", { class: "sk-title" }), h("i", { class: "sk-btn" })),
      h(
        "div",
        { class: "sk-grid" },
        card([70, 45, 60]),
        card([55, 80, 35]),
        card([65, 40, 75]),
      ),
      h(
        "div",
        { class: "sk-table" },
        ...[88, 72, 80, 64, 76].map((w, i) =>
          h("div", { class: "sk-row" }, h("i", { class: i === 0 ? "sk-check is-on" : "sk-check" }), bar(w), h("i", { class: "sk-chip" })),
        ),
      ),
    ),
  );
}
