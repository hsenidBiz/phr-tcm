// An overview's labels live outside the screenshot, never on it: a label
// drawn over the shot covered the very text it pointed at. Each one sits in
// a strip above the frame (or below it, for a part in the lower half),
// lined up with its part, and a thin leader runs from the label to the
// part's outline - the only thing that crosses the picture. Labels that
// would touch stack into a second row, the nearest row to the frame first.

/** Space between two labels in a row, and between rows, in CSS px. */
export const LABEL_GAP = 8;
/** How far a label may be pushed along its row to clear the one before it
 *  before it takes a new row instead, in CSS px. */
export const MAX_NUDGE = 140;
/** The room between the strip and the frame, where the leaders start. */
export const LEADER_ROOM = 16;

export type StripItem = { key: string; x: number; w: number };
export type StripSpot = { key: string; x: number; lane: number };

/** Places labels of width `w` along a strip `width` wide, each as near its
 *  wanted left edge `x` as it can be without touching another: along its
 *  row by up to MAX_NUDGE, otherwise in the next row. Returned in the order
 *  given. */
export function layoutStrip(items: StripItem[], width: number, gap = LABEL_GAP): StripSpot[] {
  const rows: { x: number; w: number }[][] = [];
  const spots = new Map<string, StripSpot>();
  for (const it of [...items].sort((a, b) => a.x - b.x)) {
    const w = Math.min(it.w, width);
    const want = Math.max(0, Math.min(it.x, width - w));
    for (let lane = 0; ; lane++) {
      const row = (rows[lane] ??= []);
      let x = want;
      for (const p of row) if (x < p.x + p.w + gap && p.x < x + w + gap) x = p.x + p.w + gap;
      if ((x + w <= width && x - want <= MAX_NUDGE) || row.length === 0) {
        row.push({ x, w });
        row.sort((a, b) => a.x - b.x);
        spots.set(it.key, { key: it.key, x, lane });
        break;
      }
    }
  }
  return items.map((it) => spots.get(it.key)!);
}

/** Where a leader meets its part: level with the label's middle, kept a
 *  little inside the part's ends so it never lands on a rounded corner. */
export function leaderX(label: { x: number; w: number }, part: { x: number; w: number }, inset = 10): { from: number; to: number } {
  const mid = label.x + label.w / 2;
  const to = Math.max(part.x + inset, Math.min(mid, part.x + part.w - inset));
  const from = Math.max(label.x + inset, Math.min(to, label.x + label.w - inset));
  return { from, to };
}

const SVG = "http://www.w3.org/2000/svg";

/** Lays out one overview (`nav.ov`) now and whenever its frame changes
 *  width. Without layout (no ResizeObserver) the labels stay as a wrapped
 *  row above the shot, which reads fine on its own. */
export function watchOverview(ov: HTMLElement): void {
  const frame = ov.querySelector<HTMLElement>(".frame");
  if (!frame || typeof ResizeObserver === "undefined") return;
  let lastW = -1;
  const run = () => {
    const w = frame.clientWidth;
    if (!w || w === lastW) return;
    lastW = w;
    layOut(ov, frame);
  };
  new ResizeObserver(run).observe(frame);
  // Label widths change once the web font arrives.
  document.fonts?.ready.then(() => {
    lastW = -1;
    run();
  });
}

function layOut(ov: HTMLElement, frame: HTMLElement) {
  const width = frame.clientWidth;
  const regionOf = (key: string) => frame.querySelector<HTMLElement>(`.region[data-g="${key}"]`);
  const f = frame.getBoundingClientRect();

  for (const strip of ov.querySelectorAll<HTMLElement>(".ov-strip")) {
    const labels = [...strip.querySelectorAll<HTMLElement>(".ov-label")];
    if (!labels.length) continue;
    const items = labels.map((l) => {
      const r = regionOf(l.dataset.g!)!.getBoundingClientRect();
      const w = l.offsetWidth;
      // A part on the right lines its label up with its right end, so the
      // label runs back over the shot's width rather than off it.
      const right = r.left + r.width / 2 - f.left > width * 0.55;
      return { key: l.dataset.g!, x: right ? r.right - f.left - w : r.left - f.left, w };
    });
    const spots = layoutStrip(items, width);
    const lanes = Math.max(...spots.map((s) => s.lane)) + 1;
    const lh = labels[0].offsetHeight;
    const below = strip.classList.contains("is-below");
    // The strip spans the column; the frame may be narrower, centred in it.
    const offset = f.left - strip.getBoundingClientRect().left;
    labels.forEach((l, i) => {
      const { x, lane } = spots[i];
      l.style.left = `${x + offset}px`;
      // Row 0 sits next to the frame.
      l.style.top = `${(below ? lane : lanes - 1 - lane) * (lh + LABEL_GAP) + (below ? LEADER_ROOM : 0)}px`;
    });
    strip.style.height = `${lanes * (lh + LABEL_GAP) - LABEL_GAP + LEADER_ROOM}px`;
  }
  ov.dataset.laid = "";
  drawLeaders(ov, frame);
}

function drawLeaders(ov: HTMLElement, frame: HTMLElement) {
  ov.querySelector(":scope > .ov-leaders")?.remove();
  const o = ov.getBoundingClientRect();
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("class", "ov-leaders");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("width", String(o.width));
  svg.setAttribute("height", String(o.height));
  for (const l of ov.querySelectorAll<HTMLElement>(".ov-label")) {
    const region = frame.querySelector<HTMLElement>(`.region[data-g="${l.dataset.g}"]`);
    if (!region) continue;
    const a = l.getBoundingClientRect();
    const r = region.getBoundingClientRect();
    const below = !!l.closest(".is-below");
    const { from, to } = leaderX({ x: a.left - o.left, w: a.width }, { x: r.left - o.left, w: r.width });
    const y1 = below ? a.top - o.top : a.bottom - o.top;
    const y2 = below ? r.bottom - o.top : r.top - o.top;
    const g = document.createElementNS(SVG, "g");
    g.setAttribute("class", "ov-leader");
    g.setAttribute("data-g", l.dataset.g!);
    const line = document.createElementNS(SVG, "path");
    line.setAttribute("d", `M${from} ${y1}L${to} ${y2}`);
    const dot = document.createElementNS(SVG, "circle");
    dot.setAttribute("cx", String(to));
    dot.setAttribute("cy", String(y2));
    dot.setAttribute("r", "3");
    g.append(line, dot);
    svg.appendChild(g);
  }
  ov.appendChild(svg);
}

/** Lights a label, its part and its leader together while either the label
 *  or the part is pointed at or focused. */
export function pairHover(ov: HTMLElement): void {
  const set = (key: string | undefined, on: boolean) => {
    if (!key) return;
    for (const el of ov.querySelectorAll(`[data-g="${key}"]`)) el.classList.toggle("is-hot", on);
  };
  for (const el of ov.querySelectorAll<HTMLElement>(".ov-label, .region")) {
    el.addEventListener("mouseenter", () => set(el.dataset.g, true));
    el.addEventListener("mouseleave", () => set(el.dataset.g, false));
    el.addEventListener("focus", () => set(el.dataset.g, true));
    el.addEventListener("blur", () => set(el.dataset.g, false));
  }
}
