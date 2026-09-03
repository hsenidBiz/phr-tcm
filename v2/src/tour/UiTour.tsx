import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "../components/ui/button";
import { CASE_ITEMS, WORK_ITEMS } from "../components/Sidebar";
import { IconBack, IconConfirm, IconNext } from "../lib/actionIcons";
import {
  TOUR_STEPS,
  tourAwaitedWhere,
  tourControl,
  tourDestination,
  type TourControl,
  type TourStep,
  type TourWhere,
} from "./tourScript";
import { markTourDone } from "./tourState";

const CARD_W = 340;
const CARD_H = 190; // estimate, for deciding whether the card fits below

/** How long to keep looking for a stop's area before showing the card on
 * its own. The screen has to mount and fade in first (120ms), and a slow
 * machine is allowed several times that. */
const ANCHOR_WAIT_MS = 1500;

/** The name the rail actually shows for the control a waiting stop asks
 * for - read off the rail's own lists, never invented here. */
function controlLabel(control: TourControl): string {
  if (control.kind === "case") return CASE_ITEMS.find((c) => c.id === control.section)?.label ?? "";
  if (control.kind === "work")
    return WORK_ITEMS.find((w) => w.id === control.workSection)?.label ?? "";
  // The pill is named after where it takes you, in both directions.
  return control.to === "work" ? "Work Manager" : "Test Case Manager";
}

/** That control's `data-tour`, so the ring lands on the very thing the
 * card is asking the user to click. */
function controlAnchor(control: TourControl): string {
  if (control.kind === "case") return `nav-${control.section}`;
  if (control.kind === "work") return `nav-${control.workSection}`;
  return "work";
}

/** The waiting card's title and body for the control it names - the exact
 * text the render below shows, pulled out so `tourScript.test.ts` can hold
 * every rail label to the same copy gates as the script itself, without a
 * second copy of this text drifting out of sync with the real one. */
export function tourWaitingCard(control: TourControl): { title: string; body: string } {
  const label = controlLabel(control);
  return {
    title: `Go to ${label}`,
    body:
      control.kind === "switch"
        ? `Click ${label} at the top of the screen to carry on.`
        : `Click ${label} in the menu on the left to carry on.`,
  };
}

/**
 * The guided tour: it rings one area at a time and says what it is for.
 * It never moves the app - a stop that lives somewhere else asks the user
 * to go there and waits, with Next taken away so the ask is not
 * decorative, and picks itself up when the app arrives. Back still
 * navigates for you: forward is taught, backward is convenience.
 *
 * Nothing behind it can be clicked: App makes the screens inert while this
 * is up and disables every rail row except the one the current stop is
 * waiting for, and this layer sits above it in a portal.
 */
export default function UiTour({
  at,
  onNavigate,
  onAwait,
  onClose,
  steps = TOUR_STEPS,
}: {
  /** Where the app is right now - what "is this stop a move?" is measured
   * against. The tour can be started from any tab, so the script on its
   * own cannot answer that. */
  at: TourWhere;
  onNavigate: (where: TourWhere | undefined) => void;
  /** Told which destination the tour is waiting for, and null when it is
   * not, so the host can leave exactly that one control live. MUST be
   * stable (useCallback in the host) - it is an effect dep. */
  onAwait?: (where: TourWhere | null) => void;
  onClose: () => void;
  steps?: TourStep[];
}) {
  const [i, setI] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const step = steps[i];

  // The destination this stop needs, when the app is not there yet - the
  // script's own object (see `tourDestination`), so its identity changes
  // only when the answer does and the effect below does not re-fire on
  // every render.
  const awaited = tourAwaitedWhere(i, at, steps);
  const control = awaited ? tourControl(awaited, at) : null;
  const waiting = control !== null;
  // While waiting, the ring belongs on the control being asked for: this
  // stop's own area is on a screen that is not up yet.
  const anchor = control ? controlAnchor(control) : step?.anchor;

  // Let the host leave that one control live, and nothing else.
  useEffect(() => {
    onAwait?.(awaited ?? null);
    return () => onAwait?.(null);
  }, [onAwait, awaited]);

  useEffect(() => {
    setRect(null);
    if (!anchor) return;
    const selector = `[data-tour="${anchor}"]`;
    const started = Date.now();
    let raf = 0;
    const look = () => {
      const el = document.querySelector(selector);
      if (el) {
        // Scroll the area into view before measuring it - it may be below
        // the fold (the app scrolls inside an inner container, not the
        // window), and a ring measured off screen is useless.
        el.scrollIntoView({ block: "center" });
        setRect(el.getBoundingClientRect());
        return;
      }
      if (Date.now() - started < ANCHOR_WAIT_MS) raf = requestAnimationFrame(look);
    };
    look();
    return () => cancelAnimationFrame(raf);
  }, [anchor]);

  // The ring is `position: fixed`, measured in viewport coordinates, so
  // either a resized window or a scroll moves the area out from under it.
  // Scrolling happens inside the app's own inner container, not the
  // window, so a plain bubbling listener on window would never see it -
  // scroll events don't bubble at all, only the capture phase reaches
  // window as the event travels down to its real target.
  useEffect(() => {
    if (!anchor) return;
    const remeasure = () => {
      const el = document.querySelector(`[data-tour="${anchor}"]`);
      if (el) setRect(el.getBoundingClientRect());
    };
    window.addEventListener("resize", remeasure);
    window.addEventListener("scroll", remeasure, { capture: true, passive: true });
    return () => {
      window.removeEventListener("resize", remeasure);
      window.removeEventListener("scroll", remeasure, { capture: true });
    };
  }, [anchor]);

  // Back is the one direction the tour still walks for the user: it puts
  // the app on the earlier stop's destination (its own, or the last one
  // declared before it) instead of stranding it on this stop's tab.
  const goBack = useCallback(() => {
    // Outside the state updater on purpose: an updater can be re-run, and
    // navigating twice is not free.
    const prev = Math.max(0, i - 1);
    onNavigate(tourDestination(prev, steps));
    setI(prev);
  }, [i, onNavigate, steps]);

  const finish = useCallback(() => {
    markTourDone();
    onClose();
  }, [onClose]);

  if (!step) {
    finish();
    return null;
  }

  const pad = 6;
  const below = rect ? rect.bottom + CARD_H + 24 < window.innerHeight : true;
  // No area to ring (the opening and closing cards, or a screen that took
  // too long): the card sits in the middle and reads as a plain message.
  const cardTop = rect
    ? below
      ? rect.bottom + 12
      : Math.max(12, rect.top - CARD_H - 12)
    : Math.max(12, window.innerHeight / 2 - CARD_H / 2);
  const cardLeft = rect
    ? Math.min(Math.max(12, rect.left), Math.max(12, window.innerWidth - CARD_W - 12))
    : Math.max(12, window.innerWidth / 2 - CARD_W / 2);

  return createPortal(
    <div
      // pointer-events-none is load-bearing, not tidiness: this container
      // covers the whole viewport, and a transparent covering element is
      // still what a click hits. Without it, dropping the swallow layer
      // below changes nothing - every click lands here instead of on the
      // rail the tour just asked the user to click. Each child that must
      // be clickable turns pointer events back on for itself.
      className="pointer-events-none fixed inset-0 z-[100]"
      role="dialog"
      aria-label="Interface tour"
      data-waiting={waiting ? "true" : undefined}
    >
      {/* Swallows every click that is not on the card - except while the
          tour is waiting for one, which has to get through. */}
      {waiting ? null : <div className="pointer-events-auto fixed inset-0" onClick={() => {}} />}
      {rect ? (
        <div
          className="pointer-events-none fixed rounded-lg border-2 border-accent transition-all duration-300"
          style={{
            top: rect.top - pad,
            left: rect.left - pad,
            width: rect.width + pad * 2,
            height: rect.height + pad * 2,
            boxShadow: "0 0 0 9999px rgba(0, 0, 0, 0.62)",
          }}
        />
      ) : (
        // Nothing ringed: dim everything, so the card still reads as the
        // only live thing on screen.
        <div className="pointer-events-none fixed inset-0 bg-black/60" />
      )}
      <div
        className="pointer-events-auto fixed space-y-2 rounded-lg border border-border bg-surface p-4 shadow-2xl transition-all duration-300"
        style={{ top: cardTop, left: cardLeft, width: CARD_W }}
      >
        <div className="flex items-baseline justify-between gap-2">
          <h2 className="text-sm font-semibold text-text">
            {control ? tourWaitingCard(control).title : step.title}
          </h2>
          <span className="shrink-0 text-xs text-faint">
            {i + 1} / {steps.length}
          </span>
        </div>
        <p className="text-sm text-muted">{control ? tourWaitingCard(control).body : step.body}</p>
        <div className="flex items-center gap-2 pt-1">
          <button className="text-xs text-faint hover:text-text" onClick={finish}>
            Skip tour
          </button>
          <div className="ml-auto flex gap-2">
            {i > 0 && (
              <Button variant="outline" size="sm" onClick={goBack}>
                <IconBack aria-hidden />
                Back
              </Button>
            )}
            {/* No Next while the tour is waiting to be walked somewhere:
                if it still advanced, the ask would be decorative. */}
            {waiting ? null : i < steps.length - 1 ? (
              <Button size="sm" onClick={() => setI((n) => n + 1)}>
                <IconNext aria-hidden />
                Next
              </Button>
            ) : (
              <Button size="sm" onClick={finish}>
                <IconConfirm aria-hidden />
                Done
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
