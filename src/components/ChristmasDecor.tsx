import { useEffect, useRef, useState } from "react";
import { HitContext, useBriefState, useDecorPresses, useHit, useReducedMotion, useThemeIs } from "./decorShared";

/** The Christmas theme's scenery: a string of fairy lights along the PBI
 * bar's bottom edge, baubles and a sprig of mistletoe hanging under it,
 * Santa's sleigh and two reindeer crossing the top of the window now and
 * then (dropping presents that fade as they fall), and (ChristmasCorner)
 * snow falling over the content area with a snowman in its bottom-left
 * corner and a decorated tree in its bottom-right one, behind the page. All colours come
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
const SLEIGH_WIDTH = 120;
const SLEIGH_HEIGHT = 40;
/** A present leaves the sack at the back of the sleigh this far in. */
const SACK_X = 6;
const PRESENT_EVERY_S = [0.6, 1.2] as const;
/** As long as christmas-present-fall in index.css, plus a little. */
const PRESENT_FALL_MS = 2700;
const PRESENTS_MAX = 10;
// A wrapped box: the tone class colours the paper, the ribbon is snow white.
const PRESENT_SVG =
  '<svg width="10" height="11" viewBox="0 0 10 11"><rect class="christmas-paper" x="0.5" y="3" width="9" height="7.5" rx="1"/>' +
  '<path class="christmas-ribbon" d="M4.2 3h1.6v7.5H4.2z M0.5 6h9v1.4h-9z"/>' +
  '<path class="christmas-bow" d="M5 3 Q2 0 2.2 2.4 Q2.6 3.4 5 3 Q8 0 7.8 2.4 Q7.4 3.4 5 3z"/></svg>';
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

/** One reindeer, drawn facing right in a 28 x 36 box. `lead` gets the red
 * nose. */
function Reindeer({ x, y, lead }: { x: number; y: number; lead?: boolean }) {
  return (
    <g className="christmas-reindeer" transform={`translate(${x} ${y})`}>
      <path className="christmas-leg" d="M3 27 L0 34 M6 28 L7.5 35.5 M14.5 28 L12.5 35.5 M17.5 27 L21.5 33.5" />
      <ellipse className="christmas-fur-light" cx="0.8" cy="21.5" rx="2" ry="1.4" />
      <ellipse className="christmas-fur" cx="9" cy="24" rx="9" ry="4.8" />
      <ellipse className="christmas-fur-light" cx="9.5" cy="26.6" rx="6" ry="1.9" />
      <path className="christmas-fur" d="M14.5 22 Q18.5 18 18.6 13 L22.6 13.6 Q22 20 17.8 25.5 Z" />
      <path className="christmas-fur" d="M19.6 10.6 L17.6 8.2 L20.8 9.6 Z" />
      <ellipse className="christmas-fur" cx="22.4" cy="12.4" rx="4.2" ry="2.8" />
      <ellipse className="christmas-fur-light" cx="25.4" cy="13.5" rx="1.9" ry="1.4" />
      <circle className={lead ? "christmas-nose-red" : "christmas-nose-dark"} cx="27.1" cy="13.2" r={lead ? 1.4 : 1} />
      <circle className="christmas-eye" cx="22.5" cy="11.6" r="0.6" />
      <path
        className="christmas-antler"
        d="M21 10 L19.2 4 M19.8 6.4 L17 5 M19.4 5 L20.6 2 M23.4 10 L24.4 4 M24 6.4 L26.6 5 M24.2 5 L23.2 2"
      />
      <path className="christmas-harness" d="M14.2 20 Q16.4 25 13.4 29 M16.8 20.6 Q19.4 19.4 21 16.4" />
      <circle className="christmas-gold" cx="17.4" cy="21.8" r="1.1" />
    </g>
  );
}

/** Santa's sleigh: parked off the left edge, then crosses to the right by a
 * CSS transform transition the timer starts, and waits 45-70 s for the next
 * pass. While it crosses, a present drops off the back every second or so,
 * falls and fades out; each is removed when its fall is over. Under reduced
 * motion it never starts. */
function Sleigh({ random }: { random: Random }) {
  const el = useRef<HTMLDivElement>(null);
  const presents = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();

  useEffect(() => {
    const node = el.current;
    const layer = presents.current;
    if (!node || !layer || reduced) return;
    // Every pending timer, so leaving clears the drops and removals too.
    const timers = new Set<ReturnType<typeof setTimeout>>();
    const later = (fn: () => void, ms: number) => {
      const t = setTimeout(() => {
        timers.delete(t);
        fn();
      }, ms);
      timers.add(t);
    };
    const between = ([lo, hi]: readonly [number, number]) => (lo + random() * (hi - lo)) * 1000;
    // True while the sleigh is crossing: presents drop only then.
    let crossing = false;

    const drop = () => {
      if (!crossing) return;
      const x = node.getBoundingClientRect().left + SACK_X;
      if (x > -10 && x < window.innerWidth && layer.childElementCount < PRESENTS_MAX) {
        const present = document.createElement("div");
        present.className = `christmas-present christmas-tone-${Math.floor(random() * 4)}`;
        present.style.left = `${Math.round(x)}px`;
        present.style.setProperty("--drift", `${-Math.round(8 + random() * 22)}px`);
        present.style.setProperty("--spin", `${Math.round((random() * 2 - 1) * 70)}deg`);
        present.innerHTML = PRESENT_SVG;
        layer.appendChild(present);
        later(() => present.remove(), PRESENT_FALL_MS);
      }
      later(drop, between(PRESENT_EVERY_S));
    };
    const park = () => {
      node.style.transitionDuration = "0s";
      node.style.transform = `translateX(-${SLEIGH_WIDTH + 20}px)`;
      void node.offsetWidth;
    };
    const pass = () => {
      const seconds = Math.max(8, (window.innerWidth + 2 * SLEIGH_WIDTH) / (90 + random() * 40));
      node.style.transitionDuration = `${seconds}s`;
      node.style.transform = `translateX(${window.innerWidth + 20}px)`;
      crossing = true;
      later(drop, 600);
      later(rest, seconds * 1000 + 200);
    };
    const rest = () => {
      crossing = false;
      park();
      later(pass, between(BETWEEN_PASSES_S));
    };

    park();
    later(pass, between(FIRST_PASS_S));
    return () => {
      timers.forEach(clearTimeout);
      timers.clear();
      layer.replaceChildren();
    };
  }, [random, reduced]);

  return (
    <>
      <div ref={presents} className="christmas-presents" />
      <div ref={el} className="christmas-sleigh">
        <svg width={SLEIGH_WIDTH} height={SLEIGH_HEIGHT} viewBox="0 0 120 40">
          {/* Reins first, so the reindeer and Santa's mitten sit over them. */}
          <path className="christmas-rein" d="M35 17 Q52 11 69 19.5 M35 17 Q66 8 101 18.6" />
          <Reindeer x={52} y={0} />
          <Reindeer x={84} y={-0.8} lead />
          <path
            className="christmas-runner"
            d="M2 37.5 L39 37.5 Q46.5 37.5 46.5 31.5 Q46.5 28.5 44 29 M11 32 L11 37.5 M33 32 L33 37.5"
          />
          <ellipse className="christmas-sack" cx="9.5" cy="16" rx="7" ry="8" />
          <path className="christmas-sack-tie" d="M6 9.5 Q9.5 11 13 9.5" />
          <rect className="christmas-tone-1 christmas-paper" x="5" y="4.5" width="5" height="5" rx="0.6" transform="rotate(-12 7.5 7)" />
          <rect className="christmas-tone-3 christmas-paper" x="10" y="5" width="4.5" height="4.5" rx="0.6" transform="rotate(10 12 7)" />
          <g className="christmas-santa">
            <circle className="christmas-suit" cx="24" cy="20" r="7" />
            <path className="christmas-trim" d="M17.6 21.4 Q24 23.6 30.4 21.4" />
            <path className="christmas-suit-arm" d="M27.5 18.5 L34.2 16.8" />
            <circle className="christmas-mitten" cx="35" cy="16.8" r="1.6" />
            <circle className="christmas-skin" cx="25" cy="10.4" r="3.6" />
            <path className="christmas-trim-fill" d="M21.4 11 Q21.6 17 25.4 17 Q29 16.6 28.6 11 Q25 13.4 21.4 11z" />
            <path className="christmas-suit" d="M21.4 9 Q24.6 1 31.4 3.6 L28.6 9 Z" />
            <rect className="christmas-trim-fill" x="21" y="7.8" width="8" height="2.2" rx="1.1" />
            <circle className="christmas-trim-fill" cx="31.4" cy="3.8" r="1.4" />
          </g>
          <path
            className="christmas-sleigh-body"
            d="M3 17 Q1.5 32.5 13 32.5 L37.5 32.5 Q44.5 32 45.4 21.5 Q40.5 26.5 34 25.6 L13 25.6 Q8.4 25 8.2 17 Z"
          />
          <path className="christmas-sleigh-trim" d="M3 17 L8.2 17 M5 30 Q11 31.4 17 31 L38 31 Q43 30.4 44.4 25" />
          <circle className="christmas-gold" cx="25" cy="29" r="1.2" />
        </svg>
      </div>
    </>
  );
}

/** A sprig of mistletoe tied with a red bow, hanging under the PBI bar and
 * swaying like the baubles. */
function Mistletoe() {
  return (
    <div className="christmas-mistletoe" style={{ right: "57%" }}>
      <div className="christmas-sprig-string" style={{ height: 10 }} />
      <svg width="30" height="28" viewBox="0 0 30 28">
        <path className="christmas-stem" d="M15 6 L15 14 M15 10 L8 16 M15 10 L22 16" />
        <path className="christmas-leaf" d="M15 13 Q6 12 3.5 19 Q11 20.5 15 13z" />
        <path className="christmas-leaf" d="M15 13 Q24 12 26.5 19 Q19 20.5 15 13z" />
        <path className="christmas-leaf-light" d="M15 14 Q9.5 19 10.5 26 Q16.5 21 15 14z" />
        <path className="christmas-leaf-light" d="M15 14 Q20.5 19 19.5 26 Q13.5 21 15 14z" />
        <circle className="christmas-berry" cx="13.4" cy="15.6" r="1.8" />
        <circle className="christmas-berry" cx="16.8" cy="15.4" r="1.8" />
        <circle className="christmas-berry" cx="15" cy="18" r="1.8" />
        <path
          className="christmas-bow-red"
          d="M15 5 Q8 0 8.5 5.5 Q10 7.5 15 5 Q22 0 21.5 5.5 Q20 7.5 15 5z M14 5.5 L11.5 11 M16 5.5 L18.5 11"
        />
        <circle className="christmas-bow-knot" cx="15" cy="5.2" r="1.4" />
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
        <Mistletoe />
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
      <svg className="christmas-tree" width="66" height="92" viewBox="0 0 60 84" aria-hidden="true">
        <path className="christmas-drift" d="M0 84 L0 82 Q12 79 28 77 Q44 75 60 76 L60 84z" />
        <rect className="christmas-trunk" x="27" y="69" width="6" height="10" rx="1" />
        <path className="christmas-pine" d="M5 72 L30 43 L55 72 Q30 77.5 5 72z" />
        <path className="christmas-pine-light" d="M30 43 L14 63 Q20 66 26 64z" />
        <path className="christmas-pine" d="M10 55 L30 27 L50 55 Q30 60 10 55z" />
        <path className="christmas-pine-light" d="M30 27 L17 46 Q22 49 27 47z" />
        <path className="christmas-pine" d="M15 38 L30 12 L45 38 Q30 42.5 15 38z" />
        <path className="christmas-pine-light" d="M30 12 L20 30 Q24 32.5 28 31z" />
        <path
          className="christmas-tree-snow"
          d="M6 71.6 Q12 69.6 17 72.4 M41 72.6 Q47 70 54 71.6 M11 54.6 Q16 52.6 20 55.2 M40 55.4 Q45 53 49 54.6 M16 37.6 Q21 35.8 24 38.2"
        />
        <path className="christmas-garland" d="M11 63 Q30 71.5 49 61.5 M15 46.5 Q30 53.5 45 45.5 M19.5 31.5 Q30 36.5 40.5 30.5" />
        <circle className="christmas-tone-0 christmas-ornament" cx="18" cy="68" r="2.1" />
        <circle className="christmas-tone-3 christmas-ornament" cx="40" cy="67" r="2.1" />
        <circle className="christmas-tone-1 christmas-ornament" cx="29" cy="59" r="2" />
        <circle className="christmas-tone-2 christmas-ornament" cx="22" cy="51" r="1.9" />
        <circle className="christmas-tone-0 christmas-ornament" cx="37" cy="50" r="1.9" />
        <circle className="christmas-tone-3 christmas-ornament" cx="27" cy="35" r="1.7" />
        <circle className="christmas-tone-1 christmas-ornament" cx="34" cy="24" r="1.6" />
        <path
          className="christmas-star"
          d="M30 3.2 L31.9 8 L37 8.2 L33 11.3 L34.4 16.2 L30 13.4 L25.6 16.2 L27 11.3 L23 8.2 L28.1 8 Z"
        />
        <rect className="christmas-tone-0 christmas-paper" x="13" y="74" width="9" height="7" rx="1" />
        <path className="christmas-ribbon" d="M16.8 74h1.4v7h-1.4z M13 77h9v1.2h-9z" />
        <rect className="christmas-tone-3 christmas-paper" x="38" y="75.5" width="8" height="6" rx="1" />
        <path className="christmas-ribbon" d="M41.3 75.5h1.4v6h-1.4z M38 78h8v1.2h-8z" />
      </svg>
    </>
  );
}
