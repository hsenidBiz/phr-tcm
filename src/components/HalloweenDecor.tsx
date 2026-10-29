import { useEffect, useState } from "react";
import { isCaptureMode } from "../dev/capture";

/** The Halloween theme's scenery: pumpkins hanging under the PBI bar, bats
 * crossing the window, a cobweb in the corner and a spider on a thread. All
 * colours come from the theme through the .halloween-* classes in index.css,
 * so it follows the palette. It is pure decoration: aria-hidden, never takes
 * a click, and takes no height. Rendered only while the Halloween theme is on. */

const PUMPKINS = [
  { right: "48%", string: 36, delay: "-0.9s", grin: "smile" },
  { right: "40%", string: 20, delay: "-2.2s", grin: "teeth" },
  { right: "31%", string: 44, delay: "-1.6s", grin: "smile" },
  { right: "22%", string: 28, delay: "-0.4s", grin: "teeth" },
] as const;

const BATS = [
  { top: 56, width: 22, duration: "16s", delay: "-3s" },
  { top: 110, width: 18, duration: "21s", delay: "-10s" },
  { top: 84, width: 20, duration: "14s", delay: "-7s" },
] as const;

function themeIsHalloween(): boolean {
  return document.documentElement.getAttribute("data-theme") === "halloween";
}

export default function HalloweenDecor() {
  const [on, setOn] = useState(themeIsHalloween);

  useEffect(() => {
    const sync = () => setOn(themeIsHalloween());
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  // The docs screenshots keep the app as a normal install shows it.
  if (!on || isCaptureMode()) return null;

  return (
    <div className="halloween-decor" aria-hidden="true">
      {PUMPKINS.map((p) => (
        <div key={p.right} className="halloween-pumpkin" style={{ right: p.right, animationDelay: p.delay }}>
          <div className="halloween-string" style={{ height: p.string }} />
          <svg width="28" height="24" viewBox="0 0 28 24">
            <rect className="halloween-stem" x="12.5" y="0" width="3" height="5" rx="1" />
            <ellipse className="halloween-body" cx="14" cy="14" rx="13" ry="10" />
            <path className="halloween-rib" d="M9 5 Q6 14 9 23 M19 5 Q22 14 19 23" />
            {p.grin === "smile" ? (
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
      ))}

      {BATS.map((b) => (
        <div
          key={b.top}
          className="halloween-bat"
          style={{ top: b.top, animationDuration: b.duration, animationDelay: b.delay }}
        >
          <svg width={b.width} height={Math.round(b.width * 0.55)} viewBox="0 0 22 12">
            <path d="M11 4 Q8 0 0 3 Q4 5 4 9 Q7 6 11 8 Q15 6 18 9 Q18 5 22 3 Q14 0 11 4z" />
          </svg>
        </div>
      ))}

      <svg className="halloween-web" width="90" height="90" viewBox="0 0 80 80">
        <path d="M0 0 L80 80 M0 20 L60 80 M0 45 L35 80 M0 80 L0 0 M0 80 L80 80" />
        <path d="M0 62 Q10 66 18 80 M0 42 Q22 48 38 80 M0 22 Q34 30 58 80" />
      </svg>

      <div className="halloween-spider">
        <div className="halloween-spider-thread" />
        <svg width="16" height="12" viewBox="0 0 16 12">
          <ellipse cx="8" cy="6" rx="4" ry="4" />
          <path d="M4 5 L0 2 M4 7 L0 9 M12 5 L16 2 M12 7 L16 9" />
        </svg>
      </div>
    </div>
  );
}
