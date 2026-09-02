import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "../components/ui/button";
import { IconBack, IconConfirm, IconNext } from "../lib/actionIcons";
import { TOUR_STEPS, type TourStep, type TourWhere } from "./tourScript";
import { markTourDone } from "./tourState";

const CARD_W = 340;
const CARD_H = 190; // estimate, for deciding whether the card fits below

/** How long to keep looking for a stop's area before showing the card on
 * its own. The screen has to mount and fade in first (120ms), and a slow
 * machine is allowed several times that. */
const ANCHOR_WAIT_MS = 1500;

/**
 * The guided tour: it walks the app itself - asking App to switch tab or
 * cross into the Work Manager - rings one area at a time and says what it
 * is for. Back and Next move; Skip tour (or Done at the end) closes.
 *
 * Nothing behind it can be clicked: App makes the whole shell inert while
 * this is up, and this layer sits above it in a portal.
 */
export default function UiTour({
  onNavigate,
  onClose,
  steps = TOUR_STEPS,
}: {
  /** MUST be stable (useCallback in the host) - it is an effect dep. */
  onNavigate: (where: TourWhere | undefined) => void;
  onClose: () => void;
  steps?: TourStep[];
}) {
  const [i, setI] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const step = steps[i];
  const anchor = step?.anchor;
  const where = step?.where;

  // Send the app where this stop lives, then wait for the area to appear:
  // a tab switch has to mount and fade its screen in first.
  useEffect(() => {
    onNavigate(where);
  }, [onNavigate, where]);

  useEffect(() => {
    setRect(null);
    if (!anchor) return;
    const selector = `[data-tour="${anchor}"]`;
    const started = Date.now();
    let raf = 0;
    const look = () => {
      const el = document.querySelector(selector);
      if (el) {
        setRect(el.getBoundingClientRect());
        return;
      }
      if (Date.now() - started < ANCHOR_WAIT_MS) raf = requestAnimationFrame(look);
    };
    look();
    return () => cancelAnimationFrame(raf);
  }, [anchor]);

  // A resized window moves the area out from under its ring.
  useEffect(() => {
    if (!anchor) return;
    const onResize = () => {
      const el = document.querySelector(`[data-tour="${anchor}"]`);
      if (el) setRect(el.getBoundingClientRect());
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [anchor]);

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
    <div className="fixed inset-0 z-[100]" role="dialog" aria-label="Interface tour">
      {/* Swallows every click that is not on the card. */}
      <div className="fixed inset-0" onClick={() => {}} />
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
        className="fixed space-y-2 rounded-lg border border-border bg-surface p-4 shadow-2xl transition-all duration-300"
        style={{ top: cardTop, left: cardLeft, width: CARD_W }}
      >
        <div className="flex items-baseline justify-between gap-2">
          <h2 className="text-sm font-semibold text-text">{step.title}</h2>
          <span className="shrink-0 text-xs text-faint">
            {i + 1} / {steps.length}
          </span>
        </div>
        <p className="text-sm text-muted">{step.body}</p>
        <div className="flex items-center gap-2 pt-1">
          <button className="text-xs text-faint hover:text-text" onClick={finish}>
            Skip tour
          </button>
          <div className="ml-auto flex gap-2">
            {i > 0 && (
              <Button variant="outline" size="sm" onClick={() => setI((n) => n - 1)}>
                <IconBack aria-hidden />
                Back
              </Button>
            )}
            {i < steps.length - 1 ? (
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
