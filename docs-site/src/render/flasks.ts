// A row of flasks bouncing gently in the space above the page's footer. Purely
// decoration: the strip is aria-hidden and takes no pointer or focus.
//
// Two halves. `step` is the physics, a pure function of the state, the time
// and a random source, so it is tested with plain numbers. `startFlasks` is
// the DOM driver: it owns the animation frame loop, runs only while the strip
// is on screen, the page is visible and motion is allowed, and writes
// transforms only (no layout reads inside the loop).
//
// Coordinates: x is the flask's left edge in the strip, y its height above
// the ground (0 is resting on the bottom edge, up is positive).

import { h, reducedMotion, svg } from "./dom";

// ---- tuning -----------------------------------------------------------

/** Heights in px of the flasks along the strip, left to right. Six marks. */
export const FLASK_HEIGHTS = [22, 26, 20, 28, 24, 18] as const;
/** The strip's own height in px (styles.css .flask-strip agrees). */
export const STRIP_HEIGHT = 96;
/** The drawing's shape: its viewBox is 16 wide by 21 tall. */
const ASPECT = 16 / 21;

/** Downward acceleration, px/s^2. Low, so a bounce is slow and gentle
 * (the owner, 2026-10-03: "it needs to be a gentle bounce"). */
export const GRAVITY = 450;
/** The share of its speed a flask keeps off the ground: a soft landing. */
export const RESTITUTION = 0.45;
/** A random extra upward speed on every landing, 0 to this, px/s. It is what
 * stops the bounces settling into one rhythm. */
export const KICK_MAX = 120;
/** The highest a bounce ever goes, as a share of the room above the flask. */
export const PEAK_SHARE = 0.45;
/** A landing changes the sideways speed by up to this either way, px/s. */
export const DRIFT_KICK = 60;
/** The sideways speed never exceeds this, px/s. */
export const DRIFT_MAX = 70;
/** A landing sets the tilt spinning by up to this either way, degrees/s. */
export const WOBBLE_KICK = 160;
/** Pulls the tilt back upright (per s^2), and damps it (per s). */
const TILT_SPRING = 90;
const TILT_DAMP = 6;
/** The most a flask ever leans, degrees. */
export const TILT_MAX = 24;
/** The longest step ever simulated, s. A background tab hands back a huge
 * gap; clamping it keeps a flask from tunnelling through the ground. */
export const MAX_DT = 0.05;

export type Flask = {
  x: number;
  y: number;
  vx: number;
  vy: number;
  /** Lean in degrees, and how fast it is changing. */
  rot: number;
  vr: number;
  w: number;
  h: number;
};
export type FlaskState = { width: number; height: number; flasks: Flask[] };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The highest a flask's bottom may go: its top stays inside the strip. */
const ceiling = (height: number, h: number) => Math.max(0, height - h);

/** Fast enough to reach, at most, PEAK_SHARE of the room above the flask. */
const maxLaunch = (height: number, h: number) => Math.sqrt(2 * GRAVITY * ceiling(height, h) * PEAK_SHARE);

function stepFlask(f: Flask, dt: number, width: number, height: number, rand: () => number): Flask {
  let { vx, vy, vr } = f;
  vy -= GRAVITY * dt;
  let y = f.y + vy * dt;
  let x = f.x + vx * dt;
  let rot = f.rot + vr * dt;
  vr += (-TILT_SPRING * rot - TILT_DAMP * vr) * dt;

  if (y <= 0 && vy < 0) {
    // A landing: back up with less speed plus a random kick, a little
    // sideways drift, a little tilt. Always three draws, in this order.
    y = 0;
    vy = Math.min(-vy * RESTITUTION + rand() * KICK_MAX, maxLaunch(height, f.h));
    vx = clamp(vx + (rand() * 2 - 1) * DRIFT_KICK, -DRIFT_MAX, DRIFT_MAX);
    vr += (rand() * 2 - 1) * WOBBLE_KICK;
  }
  const top = ceiling(height, f.h);
  if (y > top) {
    y = top;
    vy = Math.min(vy, 0);
  }

  const maxX = Math.max(0, width - f.w);
  if (x < 0) {
    x = 0;
    vx = Math.abs(vx);
  } else if (x > maxX) {
    x = maxX;
    vx = -Math.abs(vx);
  }
  if (Math.abs(rot) > TILT_MAX) {
    rot = clamp(rot, -TILT_MAX, TILT_MAX);
    vr = 0;
  }
  return { ...f, x, y, vx, vy, rot, vr };
}

/** One step of `dt` seconds (clamped to MAX_DT). `rand` is a source of
 * numbers in [0, 1). Returns a new state; the one given is not touched. */
export function step(state: FlaskState, dt: number, rand: () => number): FlaskState {
  const d = clamp(dt, 0, MAX_DT);
  return { ...state, flasks: state.flasks.map((f) => stepFlask(f, d, state.width, state.height, rand)) };
}

function evenly(width: number, sizes: readonly number[]): { x: number; w: number; h: number }[] {
  const dims = sizes.map((hh) => ({ w: hh * ASPECT, h: hh }));
  const room = Math.max(0, width - dims.reduce((sum, d) => sum + d.w, 0));
  const gap = room / (dims.length + 1);
  let x = gap;
  return dims.map((d) => {
    const at = { x: Math.min(x, Math.max(0, width - d.w)), ...d };
    x += d.w + gap;
    return at;
  });
}

/** Every flask upright and still on the ground, evenly spread. */
export function restState(width: number, height: number, sizes: readonly number[]): FlaskState {
  return {
    width,
    height,
    flasks: evenly(width, sizes).map((d) => ({ ...d, y: 0, vx: 0, vy: 0, rot: 0, vr: 0 })),
  };
}

/** The start of a run: spread along the strip and dropped from random heights. */
export function createState(width: number, height: number, sizes: readonly number[], rand: () => number): FlaskState {
  const rest = restState(width, height, sizes);
  return {
    ...rest,
    flasks: rest.flasks.map((f) => ({
      ...f,
      y: rand() * ceiling(height, f.h) * 0.85,
      vx: (rand() * 2 - 1) * DRIFT_MAX * 0.5,
    })),
  };
}

/** The same flasks in a strip of another size: spread kept, none outside. */
export function resizeState(state: FlaskState, width: number, height: number): FlaskState {
  const oldRoom = Math.max(0, state.width - Math.max(0, ...state.flasks.map((f) => f.w)));
  if (oldRoom === 0) {
    // Nothing to scale from (the strip had no width yet): lay them out afresh.
    return {
      width,
      height,
      flasks: evenly(
        width,
        state.flasks.map((f) => f.h),
      ).map((d, i) => ({
        ...state.flasks[i],
        ...d,
        y: Math.min(state.flasks[i].y, ceiling(height, state.flasks[i].h)),
      })),
    };
  }
  return {
    width,
    height,
    flasks: state.flasks.map((f) => {
      const oldMax = Math.max(0, state.width - f.w);
      const newMax = Math.max(0, width - f.w);
      const x = oldMax > 0 ? (f.x / oldMax) * newMax : 0;
      return { ...f, x: clamp(x, 0, newMax), y: Math.min(f.y, ceiling(height, f.h)) };
    }),
  };
}

// ---- the strip --------------------------------------------------------

/** The flask mark from the app (src/components/FlaskLogo.tsx), cropped to
 * the drawing so its base is the bottom edge of the box. */
const FLASK_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="4 2 16 21" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M9 3h6"/><path d="M10 3v6l-5 9a2 2 0 0 0 1.8 3h10.4a2 2 0 0 0 1.8-3l-5-9V3"/><path d="M7.5 14h9"/></svg>';

/** The decorative strip, flasks at rest. startFlasks brings it to life. */
export function renderFlaskStrip(): HTMLElement {
  return h(
    "div",
    { class: "flask-strip", "aria-hidden": "true" },
    ...FLASK_HEIGHTS.map((hh, i) => {
      const el = h("div", { class: `flask flask-${i % 3}` });
      el.style.width = `${(hh * ASPECT).toFixed(1)}px`;
      el.style.height = `${hh}px`;
      el.appendChild(svg(FLASK_SVG));
      return el;
    }),
  );
}

const REDUCE = "(prefers-reduced-motion: reduce)";

/** Runs the flasks in a strip from renderFlaskStrip. Returns its cleanup. */
export function startFlasks(strip: HTMLElement): () => void {
  const els = [...strip.querySelectorAll<HTMLElement>(".flask")];
  if (!els.length) return () => {};
  const sizes = els.map((_, i) => FLASK_HEIGHTS[i % FLASK_HEIGHTS.length]);

  // The same rule as the rest of the site (render/layout.ts): reduced motion
  // is the browser's setting, read when the page is built and on each change.
  let animated = !reducedMotion();
  let inView = true; // the observer corrects this the moment it reports
  let raf = 0;
  let last: number | null = null;

  let width = strip.clientWidth;
  let height = strip.clientHeight || STRIP_HEIGHT;
  let state = animated ? createState(width, height, sizes, Math.random) : restState(width, height, sizes);

  const apply = () => {
    state.flasks.forEach((f, i) => {
      els[i].style.transform = `translate(${f.x.toFixed(1)}px, ${(-f.y).toFixed(1)}px) rotate(${f.rot.toFixed(2)}deg)`;
    });
  };

  const running = () => animated && inView && !document.hidden;

  const frame = (t: number) => {
    raf = 0;
    if (!running()) return;
    // The first frame after a start or a pause takes no time, so nothing jumps.
    const dt = last === null ? 0 : (t - last) / 1000;
    last = t;
    state = step(state, dt, Math.random);
    apply();
    raf = requestAnimationFrame(frame);
  };

  const sync = () => {
    if (running()) {
      if (!raf) {
        last = null;
        raf = requestAnimationFrame(frame);
      }
    } else if (raf) {
      cancelAnimationFrame(raf);
      raf = 0;
      last = null;
    }
  };

  const measure = () => {
    const w = strip.clientWidth;
    const hh = strip.clientHeight || STRIP_HEIGHT;
    if (w === width && hh === height) return;
    width = w;
    height = hh;
    state = resizeState(state, width, height);
    apply();
  };

  const onMotion = () => {
    const next = !reducedMotion();
    if (next === animated) return;
    animated = next;
    state = animated ? createState(width, height, sizes, Math.random) : restState(width, height, sizes);
    apply();
    sync();
  };

  apply();
  sync();

  const cleanups: (() => void)[] = [];
  const onVisibility = () => sync();
  document.addEventListener("visibilitychange", onVisibility);
  cleanups.push(() => document.removeEventListener("visibilitychange", onVisibility));

  if (typeof IntersectionObserver !== "undefined") {
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        if (e.target !== strip) continue;
        inView = e.isIntersecting;
        sync();
      }
    });
    io.observe(strip);
    cleanups.push(() => io.disconnect());
  }

  if (typeof ResizeObserver !== "undefined") {
    const ro = new ResizeObserver(measure);
    ro.observe(strip);
    cleanups.push(() => ro.disconnect());
  } else {
    window.addEventListener("resize", measure);
    cleanups.push(() => window.removeEventListener("resize", measure));
  }

  try {
    const mq = window.matchMedia(REDUCE);
    mq.addEventListener("change", onMotion);
    cleanups.push(() => mq.removeEventListener("change", onMotion));
  } catch {
    // No matchMedia: motion is whatever it was at the start.
  }

  return () => {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    cleanups.forEach((c) => c());
  };
}
