// The markers on a screenshot take turns to pulse, in number order: one
// pulses, a short pause, then the next, and back to the first after the last.
// All of them pulsing on their own clocks read as a flicker; one at a time
// walks the eye through the screen the way the list reads.
//
// One timer serves every screenshot. A figure only steps while it is on
// screen, the page is visible and no marker on it is being pointed at (its
// figure has data-active), so the spotlight never competes with a pulse.

/** How long one pulse lasts (the CSS animation reads it as --pulse-ms). */
export const PULSE_MS = 900;
/** The pause after a pulse before the next marker's. */
export const PULSE_GAP_MS = 400;
export const PULSE_STEP_MS = PULSE_MS + PULSE_GAP_MS;

const NOW = "pulse-now";

type Sequence = { figure: HTMLElement; markers: HTMLElement[]; at: number; visible: boolean };

const sequences = new Set<Sequence>();
let timer: ReturnType<typeof setInterval> | null = null;
let io: IntersectionObserver | null = null;
const byFigure = new WeakMap<Element, Sequence>();

/** The marker after `at` among `count`, wrapping to the first. */
export function nextIndex(at: number, count: number): number {
  return count > 0 ? (at + 1) % count : -1;
}

function step(s: Sequence) {
  const prev = s.markers[s.at];
  prev?.classList.remove(NOW);
  s.at = nextIndex(s.at, s.markers.length);
  const next = s.markers[s.at];
  if (!next) return;
  // Re-adding the class restarts the animation; with one marker it is the
  // same element, so force a style flush between the two.
  if (next === prev) void next.offsetWidth;
  next.classList.add(NOW);
}

function tick() {
  if (typeof document !== "undefined" && document.hidden) return;
  for (const s of sequences) {
    if (!s.figure.isConnected) {
      sequences.delete(s);
      io?.unobserve(s.figure);
      continue;
    }
    if (!s.visible) continue;
    if (s.figure.dataset.active) {
      s.markers[s.at]?.classList.remove(NOW);
      continue;
    }
    step(s);
  }
  if (!sequences.size && timer) {
    clearInterval(timer);
    timer = null;
  }
}

/** Starts the turn-taking pulse for a figure's markers (in number order). */
export function registerPulse(figure: HTMLElement, markers: HTMLElement[]): void {
  if (!markers.length) return;
  const s: Sequence = { figure, markers, at: -1, visible: typeof IntersectionObserver === "undefined" };
  sequences.add(s);
  byFigure.set(figure, s);
  if (typeof IntersectionObserver !== "undefined") {
    io ??= new IntersectionObserver((entries) => {
      for (const e of entries) {
        const seq = byFigure.get(e.target);
        if (!seq) continue;
        if (e.isIntersecting === seq.visible) continue;
        seq.visible = e.isIntersecting;
        // Out of view it rests; back in view it starts again from 1.
        seq.markers[seq.at]?.classList.remove(NOW);
        seq.at = -1;
      }
    });
    io.observe(figure);
  }
  timer ??= setInterval(tick, PULSE_STEP_MS);
}

/** Test hook: forget every sequence and stop the timer. */
export function resetPulses(): void {
  for (const s of sequences) s.markers.forEach((m) => m.classList.remove(NOW));
  sequences.clear();
  io?.disconnect();
  io = null;
  if (timer) clearInterval(timer);
  timer = null;
}
