/**
 * A CSS custom property holding a length, in pixels - for canvas-drawn
 * effects that need the same number the stylesheet uses (the PBI chip's
 * glow traces the chip's own `rounded-md` corners).
 *
 * Understands px and rem, which is all the theme defines; anything else,
 * or an unset property, gives `fallback`.
 */
export function cssLengthPx(name: string, fallback: number, root: HTMLElement = document.documentElement): number {
  const raw = getComputedStyle(root).getPropertyValue(name).trim();
  const m = /^(-?\d*\.?\d+)(px|rem)$/.exec(raw);
  if (!m) return fallback;
  const n = parseFloat(m[1]);
  if (m[2] === "px") return n;
  const base = parseFloat(getComputedStyle(root).fontSize);
  return n * (Number.isFinite(base) && base > 0 ? base : 16);
}
