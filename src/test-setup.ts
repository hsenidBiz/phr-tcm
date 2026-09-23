import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";

// vitest runs without injected globals, so RTL's automatic cleanup never
// registers - without this, each test's DOM leaks into the next.
// The fading copy a dialog leaves behind as it closes (lib/exitGhost) is a
// picture, and a text query must not read it: a test that has just closed
// a dialog would otherwise still "see" its buttons for the next 150ms.
configure({ defaultIgnore: "script, style, [data-exit-ghost], [data-exit-ghost] *" });

afterEach(() => {
  cleanup();
  // A dialog unmounted by that cleanup leaves its fading copy in the page
  // for the length of the close animation (lib/exitGhost) - on a real
  // timer, which the next test would otherwise find by text.
  for (const ghost of document.querySelectorAll("[data-exit-ghost]")) ghost.remove();
});

// jsdom gaps that Base UI (the command palette's list and its scroll area) relies on.
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}
// jsdom has no Element.getAnimations - Base UI's ScrollArea viewport calls it
// (with `{ subtree: true }`) to wait out any transform animation before it
// recomputes thumb geometry; an empty list makes it skip straight past that.
// It also feeds Base UI's shared useAnimationsFinished path, so every Base UI
// popup in the suite (the command dialog, toast, any later part) now takes
// the async Promise.all([]) exit instead of the old synchronous one - closer
// to a real browser, and the full suite still passes, but it is a timing
// change worth knowing about on a machine with a documented App.test flake.
if (!Element.prototype.getAnimations) {
  Element.prototype.getAnimations = () => [];
}

// jsdom has no IntersectionObserver; motion's useInView (CountUp) needs one.
if (typeof globalThis.IntersectionObserver === "undefined") {
  globalThis.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
}

// jsdom implements no FontFaceSet; SplitText waits on document.fonts before
// it will split, so report fonts as already loaded.
if (typeof document !== "undefined" && !document.fonts) {
  Object.defineProperty(document, "fonts", {
    configurable: true,
    value: {
      status: "loaded",
      ready: Promise.resolve(),
      check: () => true,
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    },
  });
}

// jsdom never implemented matchMedia, and GSAP's ScrollTrigger calls it
// unguarded when SplitText registers its plugins at import time.
if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

// jsdom's HTMLDialogElement has no showModal/close (Astryx Dialog uses the
// native element). Minimal polyfill: track open state and fire the "close"
// event dismissal logic listens for.
if (typeof HTMLDialogElement !== "undefined" && !HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.show = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement, returnValue?: string) {
    if (returnValue !== undefined) this.returnValue = returnValue;
    this.removeAttribute("open");
    this.dispatchEvent(new Event("close", { bubbles: false }));
  };
}
