import { useEffect, useRef } from "react";

/**
 * Electric border (reactbits-style): a canvas overlay tracing the host's
 * rounded-rect border with two noise-displaced "lightning" filaments plus a
 * blurred glow pass, animated per frame. Mount it inside a relatively
 * positioned element (it fills the parent, spilling `inset` px outward so
 * the crackle can wander outside the edge). Decorative: pointer-events
 * none, aria-hidden, and it renders a calm static ring under
 * prefers-reduced-motion.
 */

/** Deterministic value noise, octaved - smooth 1D crackle. */
const rand = (i: number) => {
  const s = Math.sin(i * 127.1 + 311.7) * 43758.5453;
  return s - Math.floor(s);
};
const vnoise = (x: number) => {
  const i = Math.floor(x);
  const f = x - i;
  const u = f * f * (3 - 2 * f);
  return rand(i) * (1 - u) + rand(i + 1) * u;
};
const octaved = (x: number) =>
  vnoise(x) * 0.55 + vnoise(x * 2.3 + 7.1) * 0.3 + vnoise(x * 5.7 + 13.9) * 0.15;

/** Point + outward normal at arc-length s along a rounded rectangle. */
function pointAt(s: number, w: number, h: number, r: number) {
  const sw = w - 2 * r;
  const sh = h - 2 * r;
  const arc = (Math.PI / 2) * r;
  // Segments clockwise from the top-left corner's end: top, TR arc, right,
  // BR arc, bottom, BL arc, left, TL arc.
  const segs = [sw, arc, sh, arc, sw, arc, sh, arc];
  let d = s;
  for (let i = 0; i < 8; i++) {
    if (d > segs[i]) {
      d -= segs[i];
      continue;
    }
    const t = d / segs[i];
    switch (i) {
      case 0:
        return { x: r + d, y: 0, nx: 0, ny: -1 };
      case 1: {
        const a = -Math.PI / 2 + (t * Math.PI) / 2;
        return { x: w - r + r * Math.cos(a), y: r + r * Math.sin(a), nx: Math.cos(a), ny: Math.sin(a) };
      }
      case 2:
        return { x: w, y: r + d, nx: 1, ny: 0 };
      case 3: {
        const a = (t * Math.PI) / 2;
        return { x: w - r + r * Math.cos(a), y: h - r + r * Math.sin(a), nx: Math.cos(a), ny: Math.sin(a) };
      }
      case 4:
        return { x: w - r - d, y: h, nx: 0, ny: 1 };
      case 5: {
        const a = Math.PI / 2 + (t * Math.PI) / 2;
        return { x: r + r * Math.cos(a), y: h - r + r * Math.sin(a), nx: Math.cos(a), ny: Math.sin(a) };
      }
      case 6:
        return { x: 0, y: h - r - d, nx: -1, ny: 0 };
      default: {
        const a = Math.PI + (t * Math.PI) / 2;
        return { x: r + r * Math.cos(a), y: r + r * Math.sin(a), nx: Math.cos(a), ny: Math.sin(a) };
      }
    }
  }
  return { x: r, y: 0, nx: 0, ny: -1 };
}

export default function ElectricBorder({
  radius = 9,
  inset = 10,
  chaos = 3.2,
  speed = 1.6,
}: {
  /** Corner radius of the traced border, px. */
  radius?: number;
  /** How far the canvas extends past the host, px (room for the crackle). */
  inset?: number;
  /** Max displacement of the filaments, px. */
  chaos?: number;
  /** Noise scroll speed. */
  speed?: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    if (!canvas || !host) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    const color =
      getComputedStyle(host).getPropertyValue("--color-accent").trim() || "#8b5cf6";

    let raf = 0;
    let w = 0;
    let h = 0;
    const dpr = window.devicePixelRatio || 1;
    const resize = () => {
      const rect = host.getBoundingClientRect();
      w = rect.width + inset * 2;
      h = rect.height + inset * 2;
      canvas.width = Math.max(1, Math.round(w * dpr));
      canvas.height = Math.max(1, Math.round(h * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    const ro = new ResizeObserver(resize);
    ro.observe(host);
    resize();

    /** One filament: the border path with noise-displaced points. */
    const trace = (time: number, seed: number, amp: number) => {
      const bw = w - inset * 2;
      const bh = h - inset * 2;
      const r = Math.min(radius, bw / 2, bh / 2);
      const perimeter = 2 * (bw - 2 * r) + 2 * (bh - 2 * r) + 2 * Math.PI * r;
      const step = 3; // px between samples
      ctx.beginPath();
      for (let s = 0; s <= perimeter; s += step) {
        const p = pointAt(s, bw, bh, r);
        const n = octaved(s * 0.055 + time * speed + seed) - 0.5;
        const x = inset + p.x + p.nx * n * 2 * amp;
        const y = inset + p.y + p.ny * n * 2 * amp;
        if (s === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.closePath();
      ctx.stroke();
    };

    const draw = (now: number) => {
      const time = now / 1000;
      ctx.clearRect(0, 0, w, h);
      ctx.strokeStyle = color;
      ctx.shadowColor = color;

      // Glow pass: one soft wide halo under the filaments.
      ctx.lineWidth = 2.5;
      ctx.shadowBlur = 14;
      ctx.globalAlpha = 0.45;
      trace(time, 0, chaos);

      // Two crisp filaments with independent noise = the electric crackle.
      ctx.shadowBlur = 6;
      ctx.lineWidth = 1.2;
      ctx.globalAlpha = 0.95;
      trace(time, 0, chaos);
      ctx.globalAlpha = 0.6;
      trace(time * 1.13, 42.7, chaos * 1.35);

      raf = requestAnimationFrame(draw);
    };

    if (reduced) {
      // A single calm static ring - no animation loop at all.
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.shadowColor = color;
      ctx.shadowBlur = 10;
      trace(0, 0, 0);
    } else {
      raf = requestAnimationFrame(draw);
    }

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, [radius, inset, chaos, speed]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      data-testid="electric-border"
      className="pointer-events-none absolute z-10"
      style={{
        left: -inset,
        top: -inset,
        width: `calc(100% + ${inset * 2}px)`,
        height: `calc(100% + ${inset * 2}px)`,
      }}
    />
  );
}
