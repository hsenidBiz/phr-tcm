import { useLayoutEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { Button } from "./ui/button";

const TOUR_DONE_KEY = "tcm-v2-tour-done";

/** Fired by Settings' "Show UI tour" button; App listens and reopens. */
export const START_TOUR_EVENT = "tcm-start-tour";

export function tourDone(): boolean {
  try {
    return localStorage.getItem(TOUR_DONE_KEY) === "yes";
  } catch {
    return true; // no storage -> never auto-run
  }
}

function markTourDone() {
  try {
    localStorage.setItem(TOUR_DONE_KEY, "yes");
  } catch {
    // session-only
  }
}

type Step = { target: string; title: string; body: string };

const STEPS: Step[] = [
  {
    target: '[data-tour="org"]',
    title: "Pick your scope",
    body: "Start here: choose the Azure DevOps organization and project you work in. Every screen scopes to this choice.",
  },
  {
    target: '[data-tour="pbi"]',
    title: "Find your PBI",
    body: "Search for the Product Backlog Item you're testing (by title or id). Test cases you create are linked to it, and recents are remembered.",
  },
  {
    target: '[data-tour="nav-manual"]',
    title: "Manual Entry",
    body: "Write test cases by hand - title, tags, module and a numbered step grid - then queue and create them in bulk.",
  },
  {
    target: '[data-tour="nav-import"]',
    title: "Import File",
    body: "Upload test cases from a JSON file (the same format Export JSON produces). A kept id updates that work item; a new entry creates one.",
  },
  {
    target: '[data-tour="nav-edit"]',
    title: "Update Test Cases",
    body: "Browse the PBI's linked cases: click to select, ctrl/shift for many, bulk edit fields, group by title, or export.",
  },
  {
    target: '[data-tour="nav-view"]',
    title: "View Test Cases",
    body: "A read-only view of the PBI's cases: expand for steps, jot local comments on cases that need changes, and open a selection as a browser report.",
  },
  {
    target: '[data-tour="nav-run"]',
    title: "Run Tests",
    body: "See each case's last outcome and history, pick a set, and run them in the always-on-top runner with screenshots and bug filing.",
  },
  {
    target: '[data-tour="nav-suites"]',
    title: "Test Suites",
    body: "Browse every test plan's suite folders, search them, view cases in the browser, or jump straight into Edit / Run.",
  },
  {
    target: '[data-tour="work"]',
    title: "Work Manager",
    body: "A lightweight board of your work items - drag between To Do / In Progress / Done and open any card for full details.",
  },
  {
    target: '[data-tour="settings"]',
    title: "Settings",
    body: "Themes and accents, updates - and you can replay this tour from here anytime.",
  },
];

const CARD_W = 340;
const CARD_H = 190; // estimate for flip-above placement

/** Spotlight walkthrough: dims the app, rings one area at a time and
 * explains it; Next/Back step through, Skip ends. Steps whose anchor is
 * not currently on screen are dropped automatically. */
export default function UiTour({ onClose }: { onClose: () => void }) {
  // Resolve available steps once at open (targets do not change mid-tour).
  const steps = useMemo(() => STEPS.filter((s) => document.querySelector(s.target)), []);
  const [i, setI] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const step = steps[i];

  useLayoutEffect(() => {
    if (!step) return;
    const el = document.querySelector(step.target);
    setRect(el ? el.getBoundingClientRect() : null);
  }, [step]);

  const finish = () => {
    markTourDone();
    onClose();
  };

  if (!step) {
    // Nothing to anchor to (shouldn't happen signed in) - just close.
    finish();
    return null;
  }

  const pad = 6;
  const below = rect ? rect.bottom + CARD_H + 24 < window.innerHeight : true;
  const cardTop = rect ? (below ? rect.bottom + 12 : Math.max(12, rect.top - CARD_H - 12)) : 80;
  const cardLeft = rect
    ? Math.min(Math.max(12, rect.left), Math.max(12, window.innerWidth - CARD_W - 12))
    : 80;

  return createPortal(
    <div className="fixed inset-0 z-[100]" role="dialog" aria-label="Interface tour">
      {/* Blocks interaction underneath while the tour is open. */}
      <div className="fixed inset-0" onClick={() => {}} />
      {rect && (
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
                Back
              </Button>
            )}
            {i < steps.length - 1 ? (
              <Button size="sm" onClick={() => setI((n) => n + 1)}>
                Next
              </Button>
            ) : (
              <Button size="sm" onClick={finish}>
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
