import { useEffect, useRef, useState } from "react";
import { HitContext, useBriefState, useDecorPresses, useHit, useReducedMotion, useThemeIs } from "./decorShared";

/** The Halloween theme's scenery: pumpkins hanging under the PBI bar, bats
 * flying from point to point, spiders on threads, and (HalloweenWeb) a
 * cobweb with bugs in the content area's bottom-left corner. All colours
 * come from the theme through the .halloween-* classes in index.css, so it
 * follows the palette. It is decoration: aria-hidden, takes no height, and
 * takes no click itself (pointer-events none everywhere). A pumpkin or
 * spider answers a press on the page under it through one document
 * listener that leaves anything interactive alone. Rendered only while the
 * Halloween theme is on. */

const PUMPKINS = [
  { right: "48%", string: 6, delay: "-0.9s", grin: "smile" },
  { right: "40%", string: 16, delay: "-2.2s", grin: "teeth" },
  { right: "31%", string: 10, delay: "-1.6s", grin: "smile" },
  { right: "22%", string: 20, delay: "-0.4s", grin: "teeth" },
] as const;

// Pumpkins' right edges sit at 52%..78% from the left, each 32px wide; the
// middle spider at 40% stays clear of them down to a 760px column.
const SPIDERS = [
  { left: "10%", drop: 70, duration: "20s", delay: "-4s" },
  { left: "40%", drop: 45, duration: "16s", delay: "-9s" },
  { left: "86%", drop: 90, duration: "27s", delay: "-15s" },
] as const;

const BATS = [{ width: 26 }, { width: 22 }, { width: 24 }] as const;

const WHACK_MS = 1200;
const SCURRY_MS = 7000;
const EDGE = 40;

type Random = () => number;

/** True while the Halloween theme is on (and this is not a docs capture). */
function useHalloween(): boolean {
  return useThemeIs("halloween");
}

function Pumpkin({ right, string, delay, grin }: (typeof PUMPKINS)[number]) {
  const [whacked, whack] = useBriefState(WHACK_MS);
  const [everWhacked, setEverWhacked] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const body = useRef<SVGSVGElement>(null);
  useHit(body, () => {
    // Restart the swing on every press: drop the class, flush, add it back.
    const el = root.current;
    if (el) {
      el.classList.remove("is-whack");
      void el.offsetWidth;
      el.classList.add("is-whack");
    }
    setEverWhacked(true);
    whack();
  });
  // After a whack the sway starts at rest too, so the hand-off has no snap.
  const style = { right, "--sway-delay": everWhacked ? "0s" : delay } as React.CSSProperties;
  return (
    <div ref={root} className={`halloween-pumpkin${whacked ? " is-whack" : ""}`} style={style}>
      <div className="halloween-string" style={{ height: string }} />
      <svg ref={body} width="32" height="27" viewBox="0 0 28 24">
        <rect className="halloween-stem" x="12.5" y="0" width="3" height="5" rx="1" />
        <ellipse className="halloween-body" cx="14" cy="14" rx="13" ry="10" />
        <path className="halloween-rib" d="M9 5 Q6 14 9 23 M19 5 Q22 14 19 23" />
        {grin === "smile" ? (
          <g className="halloween-glow">
            <path d="M7 11 l3 -3 l3 3z M15 11 l3 -3 l3 3z" />
            <path d="M7 16 q7 6 14 0 l-3 1 l-2 -1 l-2 1 l-2 -1 l-2 1z" />
          </g>
        ) : (
          <g className="halloween-glow">
            <circle cx="9" cy="11" r="2" />
            <circle cx="19" cy="11" r="2" />
            <path d="M8 16 q6 5 12 0z" />
          </g>
        )}
      </svg>
    </div>
  );
}

function Spider({ left, drop, duration, delay }: (typeof SPIDERS)[number]) {
  const [scurrying, scurry, live] = useBriefState(SCURRY_MS);
  const thread = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [from, setFrom] = useState(0);
  useHit(body, () => {
    if (live.current) return;
    // Start the climb from wherever the thread is right now.
    setFrom(thread.current?.getBoundingClientRect().height ?? 0);
    scurry();
  });
  const style = {
    left,
    animationDuration: duration,
    animationDelay: delay,
    "--drop": `${drop}px`,
    "--drop-n": drop,
    "--from": `${from}px`,
    "--from-n": from,
  } as React.CSSProperties;
  return (
    <div className={`halloween-spider${scurrying ? " is-scurry" : ""}`} style={style}>
      <div ref={thread} className="halloween-spider-thread" />
      <div ref={body} className="halloween-spider-body">
        <svg width="16" height="12" viewBox="0 0 16 12">
          <ellipse cx="8" cy="6" rx="4" ry="4" />
          <path d="M4 5 L0 2 M4 7 L0 9 M12 5 L16 2 M12 7 L16 9" />
        </svg>
      </div>
    </div>
  );
}

/** One bat: flies to a random point in the top of the window, pauses
 * sometimes, then picks the next. The CSS transition does the flying; the
 * timer is only a fallback for a transitionend that never arrives. */
function Bat({ width, random }: { width: number; random: Random }) {
  const bat = useRef<HTMLDivElement>(null);
  const face = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();

  useEffect(() => {
    const el = bat.current;
    const faceEl = face.current;
    if (!el || !faceEl || reduced) return;
    const height = Math.round(width * 0.55);
    let x = 0;
    let y = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const maxX = () => Math.max(EDGE, window.innerWidth - EDGE - width);
    const maxY = () => Math.max(EDGE, window.innerHeight * 0.6 - height);
    const point = () => ({
      x: EDGE + random() * Math.max(0, window.innerWidth - 2 * EDGE - width),
      y: EDGE + random() * Math.max(0, window.innerHeight * 0.6 - EDGE - height),
    });
    const fly = () => {
      const to = point();
      const dist = Math.hypot(to.x - x, to.y - y);
      const seconds = Math.max(0.5, dist / (150 + random() * 100));
      faceEl.style.transform = to.x < x ? "scaleX(-1)" : "";
      el.style.transitionDuration = `${seconds}s`;
      el.style.transform = `translate(${to.x}px, ${to.y}px)`;
      x = to.x;
      y = to.y;
      clearTimeout(timer);
      timer = setTimeout(landed, seconds * 1000 + 250);
    };
    const landed = () => {
      clearTimeout(timer);
      timer = setTimeout(fly, random() < 0.4 ? random() * 1500 : 0);
    };
    const onEnd = (e: TransitionEvent) => {
      if (e.target === el) landed();
    };
    // A resize that leaves the target, or the bat itself, outside the
    // window sends it to a new point in view at once.
    const onResize = () => {
      const r = el.getBoundingClientRect();
      const lost = r.left > window.innerWidth || r.top > window.innerHeight;
      if (lost || x > maxX() || y > maxY()) fly();
    };

    // Start from a random spot with no transition, then fly.
    ({ x, y } = point());
    el.style.transitionDuration = "0s";
    el.style.transform = `translate(${x}px, ${y}px)`;
    void el.offsetWidth;
    el.addEventListener("transitionend", onEnd);
    window.addEventListener("resize", onResize);
    fly();
    return () => {
      clearTimeout(timer);
      el.removeEventListener("transitionend", onEnd);
      window.removeEventListener("resize", onResize);
    };
  }, [width, random, reduced]);

  return (
    <div ref={bat} className="halloween-bat">
      <div ref={face} className="halloween-bat-face">
        <svg width={width} height={Math.round(width * 0.55)} viewBox="0 0 22 12">
          <path d="M11 4 Q8 0 0 3 Q4 5 4 9 Q7 6 11 8 Q15 6 18 9 Q18 5 22 3 Q14 0 11 4z" />
        </svg>
      </div>
    </div>
  );
}

export default function HalloweenDecor({ random = Math.random }: { random?: Random }) {
  const on = useHalloween();
  const register = useDecorPresses(on);

  if (!on) return null;

  return (
    <HitContext.Provider value={register}>
    <div className="halloween-decor" aria-hidden="true">
      {PUMPKINS.map((p) => (
        <Pumpkin key={p.right} {...p} />
      ))}
      {BATS.map((b, i) => (
        <Bat key={i} width={b.width} random={random} />
      ))}
      {SPIDERS.map((s) => (
        <Spider key={s.left} {...s} />
      ))}
    </div>
    </HitContext.Provider>
  );
}

/** The cobweb in the bottom-left of the content column, with what it caught.
 * Mounted inside that (positioned) column, so it clears the left rail. */
export function HalloweenWeb() {
  if (!useHalloween()) return null;

  return (
    <svg className="halloween-web" width="90" height="90" viewBox="0 0 80 80" aria-hidden="true">
      <path d="M0 80 L0 0 M0 80 L31 6 M0 80 L57 23 M0 80 L74 49 M0 80 L80 80" />
      <path d="M0 56 Q4 59 9 58 Q12 63 17 63 Q17 68 22 71 Q20 76 24 80" />
      <path d="M0 36 Q8 43 17 39 Q21 48 31 49 Q32 60 41 63 Q37 73 44 80" />
      <path d="M0 16 Q11 26 25 21 Q31 34 46 34 Q46 49 59 55 Q54 69 64 80" />
      <g className="halloween-fly">
        <ellipse className="halloween-fly-wing" cx="36" cy="49" rx="3.4" ry="1.6" transform="rotate(-35 38 51)" />
        <ellipse className="halloween-fly-wing" cx="40" cy="48" rx="3.4" ry="1.6" transform="rotate(-70 38 51)" />
        <ellipse className="halloween-bug-body" cx="38" cy="52" rx="1.5" ry="2.4" />
        <circle className="halloween-bug-body" cx="38" cy="49.4" r="1.1" />
      </g>
      <g className="halloween-beetle">
        <path className="halloween-bug-leg" d="M9 31 L5 29 M9 33 L5 34 M15 31 L19 29 M15 33 L19 34" />
        <ellipse className="halloween-bug-body" cx="12" cy="33" rx="3.3" ry="4.2" />
        <circle className="halloween-bug-body" cx="12" cy="28.4" r="1.5" />
        <path className="halloween-bug-leg" d="M12 29.5 L12 37" />
      </g>
      <g className="halloween-cocoon">
        <ellipse className="halloween-bug-wrap" cx="57" cy="67" rx="3.6" ry="6" transform="rotate(40 57 67)" />
        <path className="halloween-bug-leg" d="M53 64 L58 62 M52 67 L60 65 M53 70 L60 68" />
      </g>
    </svg>
  );
}
