// A short confetti burst over the whole window, for the optional extras'
// unlock on the Settings screen (screens/settingsExtras.ts). Our own small
// canvas animation - no package. Nothing under prefers-reduced-motion.
import { reducedMotion } from "./exitGhost";

/** How long a burst lasts. */
export const CONFETTI_MS = 2800;

/** Theme tokens the pieces take their colours from, read at burst time so
 * the burst matches whatever theme and accent are on. */
const TOKENS = ["--color-accent", "--color-success", "--color-warning", "--color-danger", "--color-muted"];

const PIECES = 140;

type Piece = { x: number; y: number; vx: number; vy: number; size: number; rot: number; vr: number; color: string };

/**
 * Start a burst; returns a function that stops it early. Does nothing
 * under prefers-reduced-motion, or where there is no 2D canvas (jsdom).
 * The canvas is aria-hidden and takes no pointer events, and a timer - not
 * the frame loop - removes it, because a hidden window stops sending frames
 * and the canvas must not outlive the burst.
 */
export function burstConfetti(durationMs: number = CONFETTI_MS): () => void {
  if (reducedMotion()) return () => {};
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) return () => {};

  const root = getComputedStyle(document.documentElement);
  const tokens = TOKENS.map((t) => root.getPropertyValue(t).trim()).filter(Boolean);
  const palette = tokens.length > 0 ? tokens : [getComputedStyle(document.body).color];

  const w = window.innerWidth || 1;
  const h = window.innerHeight || 1;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(w * dpr));
  canvas.height = Math.max(1, Math.floor(h * dpr));
  canvas.setAttribute("aria-hidden", "true");
  canvas.setAttribute("data-confetti", "");
  Object.assign(canvas.style, {
    position: "fixed",
    top: "0",
    left: "0",
    width: "100vw",
    height: "100vh",
    pointerEvents: "none",
    zIndex: "60",
  });
  ctx.scale(dpr, dpr);
  document.body.appendChild(canvas);

  const pieces: Piece[] = Array.from({ length: PIECES }, (_, i) => ({
    x: w / 2 + (Math.random() - 0.5) * w * 0.25,
    y: h * 0.35,
    vx: (Math.random() - 0.5) * 14,
    vy: -6 - Math.random() * 10,
    size: 5 + Math.random() * 5,
    rot: Math.random() * Math.PI,
    vr: (Math.random() - 0.5) * 0.3,
    color: palette[i % palette.length],
  }));

  const raf =
    typeof window.requestAnimationFrame === "function"
      ? window.requestAnimationFrame.bind(window)
      : (cb: FrameRequestCallback) => window.setTimeout(() => cb(performance.now()), 16);
  const caf =
    typeof window.cancelAnimationFrame === "function"
      ? window.cancelAnimationFrame.bind(window)
      : (id: number) => window.clearTimeout(id);

  const start = performance.now();
  let last = start;
  let frame = 0;
  let stopped = false;
  let timer = 0;

  const stop = () => {
    if (stopped) return;
    stopped = true;
    caf(frame);
    window.clearTimeout(timer);
    canvas.remove();
  };

  const tick = (now: number) => {
    if (stopped) return;
    const dt = Math.min(3, Math.max(0, (now - last) / 16.7));
    last = now;
    ctx.clearRect(0, 0, w, h);
    ctx.globalAlpha = Math.max(0, 1 - (now - start) / durationMs);
    for (const p of pieces) {
      p.vy += 0.35 * dt;
      p.vx *= 0.99;
      p.x += p.vx * dt;
      p.y += p.vy * dt;
      p.rot += p.vr * dt;
      ctx.save();
      ctx.translate(p.x, p.y);
      ctx.rotate(p.rot);
      ctx.fillStyle = p.color;
      ctx.fillRect(-p.size / 2, -p.size / 4, p.size, p.size / 2);
      ctx.restore();
    }
    frame = raf(tick);
  };

  frame = raf(tick);
  timer = window.setTimeout(stop, durationMs);
  return stop;
}
