import { useEffect, useRef, useState } from "react";
import { HitContext, useBriefState, useDecorPresses, useHit, useReducedMotion, useThemeIs } from "./decorShared";

/** The Christmas theme's scenery: a string of fairy lights along the PBI
 * bar's bottom edge, baubles hanging under it, Santa's sleigh crossing the
 * top of the window now and then, and (ChristmasCorner) snow falling over the
 * content area with a snowman in its bottom-left corner. All colours come
 * from the theme through the .christmas-* classes in index.css. It is
 * decoration: aria-hidden, takes no height, and takes no click itself
 * (pointer-events none everywhere). A bauble answers a press on the page
 * under it through the shared document listener, which leaves anything
 * interactive alone. Rendered only while the Christmas theme is on, and
 * never in a docs capture. */

const LIGHTS = 14;

// The wire is a quadratic curve from (0, 2) to (100, 2) drooping to y = 7 in
// the middle, drawn in a box 14px tall; a bulb at fraction t hangs on it.
const wireY = (t: number) => 2 + 20 * t * (1 - t);

const BULBS = Array.from({ length: LIGHTS }, (_, i) => {
  const t = (i + 0.5) / LIGHTS;
  return { left: `${(t * 100).toFixed(2)}%`, top: wireY(t).toFixed(2), tone: i % 4, group: i % 3 };
});

// Same offsets as the pumpkins, so they stay clear of the right-hand controls.
const BAUBLES = [
  { right: "48%", string: 6, delay: "-0.9s", tone: 0 },
  { right: "40%", string: 16, delay: "-2.2s", tone: 1 },
  { right: "31%", string: 10, delay: "-1.6s", tone: 3 },
  { right: "22%", string: 20, delay: "-0.4s", tone: 2 },
] as const;

const FLAKES = Array.from({ length: 20 }, (_, i) => {
  const duration = 9 + ((i * 7) % 10);
  return {
    left: `${((i * 61.8) % 100).toFixed(1)}%`,
    size: 2 + (i % 3),
    duration: `${duration}s`,
    delay: `${-((i * 1.7) % duration).toFixed(1)}s`,
    drift: `${((i % 5) - 2) * 14}px`,
    opacity: (0.5 + (i % 4) * 0.12).toFixed(2),
  };
});

const SPIN_MS = 1200;
const SLEIGH_WIDTH = 70;
const FIRST_PASS_S = [8, 20] as const;
const BETWEEN_PASSES_S = [45, 70] as const;

type Random = () => number;

function Bauble({ right, string, delay, tone }: (typeof BAUBLES)[number]) {
  const [spinning, spin] = useBriefState(SPIN_MS);
  const [everSpun, setEverSpun] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const body = useRef<SVGSVGElement>(null);
  useHit(body, () => {
    // Restart the spin on every press: drop the class, flush, add it back.
    const el = root.current;
    if (el) {
      el.classList.remove("is-spin");
      void el.offsetWidth;
      el.classList.add("is-spin");
    }
    setEverSpun(true);
    spin();
  });
  // After a spin the sway starts at rest too, so the hand-off has no snap.
  const style = { right, "--sway-delay": everSpun ? "0s" : delay } as React.CSSProperties;
  return (
    <div
      ref={root}
      className={`christmas-bauble christmas-tone-${tone}${spinning ? " is-spin" : ""}`}
      style={style}
    >
      <div className="christmas-string" style={{ height: string }} />
      <div className="christmas-ball">
        <svg ref={body} width="20" height="22" viewBox="0 0 20 22">
          <rect className="christmas-cap" x="7" y="0" width="6" height="3" rx="1" />
          <circle className="christmas-body" cx="10" cy="13" r="8" />
          <path className="christmas-shine" d="M5 12 Q5.5 8.5 9 7.5" />
        </svg>
      </div>
    </div>
  );
}

/** Santa's sleigh: parked off the left edge, then crosses to the right by a
 * CSS transform transition the timer starts, and waits 45-70 s for the next
 * pass. Under reduced motion it never starts. */
function Sleigh({ random }: { random: Random }) {
  const el = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();

  useEffect(() => {
    const node = el.current;
    if (!node || reduced) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const between = ([lo, hi]: readonly [number, number]) => (lo + random() * (hi - lo)) * 1000;

    const park = () => {
      node.style.transitionDuration = "0s";
      node.style.transform = `translateX(-${SLEIGH_WIDTH + 20}px)`;
      void node.offsetWidth;
    };
    const pass = () => {
      const seconds = Math.max(8, (window.innerWidth + 2 * SLEIGH_WIDTH) / (90 + random() * 40));
      node.style.transitionDuration = `${seconds}s`;
      node.style.transform = `translateX(${window.innerWidth + 20}px)`;
      timer = setTimeout(rest, seconds * 1000 + 200);
    };
    const rest = () => {
      park();
      timer = setTimeout(pass, between(BETWEEN_PASSES_S));
    };

    park();
    timer = setTimeout(pass, between(FIRST_PASS_S));
    return () => clearTimeout(timer);
  }, [random, reduced]);

  return (
    <div ref={el} className="christmas-sleigh">
      <svg width={SLEIGH_WIDTH} height="26" viewBox="0 0 70 26">
        <path d="M2 8 Q2 18 8 19 L26 19 Q28 12 24 8z" />
        <circle cx="14" cy="5.5" r="3.6" />
        <circle cx="6" cy="8" r="3.4" />
        <path className="christmas-line" d="M0 22 Q14 24 30 21 Q34 20 33 17 M28 15 L38 14 M48 14 L56 14" />
        <g className="christmas-reindeer">
          <ellipse cx="44" cy="13" rx="6" ry="3.5" />
          <circle cx="51" cy="8" r="2.5" />
          <path className="christmas-line" d="M51 6 l-1 -4 m1 4 l3 -3 M40 15 l-2 6 M42 16 l1 5 M47 16 l-1 5 M49 15 l2 5" />
        </g>
        <g className="christmas-reindeer" transform="translate(18 0)">
          <ellipse cx="44" cy="13" rx="6" ry="3.5" />
          <circle cx="51" cy="8" r="2.5" />
          <path className="christmas-line" d="M51 6 l-1 -4 m1 4 l3 -3 M40 15 l-2 6 M42 16 l1 5 M47 16 l-1 5 M49 15 l2 5" />
        </g>
      </svg>
    </div>
  );
}

export default function ChristmasDecor({ random = Math.random }: { random?: Random }) {
  const on = useThemeIs("christmas");
  const register = useDecorPresses(on);

  if (!on) return null;

  return (
    <HitContext.Provider value={register}>
      <div className="christmas-decor" aria-hidden="true">
        <div className="christmas-lights">
          <svg width="100%" height="14" viewBox="0 0 100 14" preserveAspectRatio="none">
            <path d="M0 2 Q50 12 100 2" vectorEffect="non-scaling-stroke" />
          </svg>
          {[0, 1, 2].map((g) => (
            <div key={g} className={`christmas-twinkle christmas-twinkle-${g}`}>
              {BULBS.filter((b) => b.group === g).map((b) => (
                <span
                  key={b.left}
                  className={`christmas-bulb christmas-tone-${b.tone}`}
                  style={{ left: b.left, top: `${b.top}px` }}
                />
              ))}
            </div>
          ))}
        </div>
        {BAUBLES.map((b) => (
          <Bauble key={b.right} {...b} />
        ))}
        <Sleigh random={random} />
      </div>
    </HitContext.Provider>
  );
}

/** The snowfall and the snowman, in the (positioned) content column, so
 * both clear the left rail. */
export function ChristmasCorner() {
  if (!useThemeIs("christmas")) return null;

  return (
    <>
      <div className="christmas-snow" aria-hidden="true">
        {FLAKES.map((f, i) => (
          <span
            key={i}
            className="christmas-flake"
            style={
              {
                left: f.left,
                width: f.size,
                height: f.size,
                opacity: f.opacity,
                animationDuration: f.duration,
                animationDelay: f.delay,
                "--drift": f.drift,
              } as React.CSSProperties
            }
          />
        ))}
      </div>
      <svg className="christmas-snowman" width="90" height="90" viewBox="0 0 80 80" aria-hidden="true">
        <path className="christmas-drift" d="M0 80 L0 69 Q18 63 38 69 Q56 74 72 79 L74 80z" />
        <path className="christmas-arm" d="M17 45 L8 36 M35 45 L44 39" />
        <circle className="christmas-snow-body" cx="26" cy="62" r="12" />
        <circle className="christmas-snow-body" cx="26" cy="44" r="9" />
        <circle className="christmas-snow-body" cx="26" cy="30" r="7" />
        <path className="christmas-scarf" d="M19 36 Q26 40 33 36 L33 40 Q26 44 19 40z M30 39 l3 10 l-4 0z" />
        <circle className="christmas-coal" cx="23.5" cy="28.5" r="1" />
        <circle className="christmas-coal" cx="28.5" cy="28.5" r="1" />
        <circle className="christmas-coal" cx="26" cy="50" r="1" />
        <circle className="christmas-coal" cx="26" cy="55" r="1" />
        <path className="christmas-nose" d="M26 31 l7 1.5 l-7 1.5z" />
      </svg>
    </>
  );
}
