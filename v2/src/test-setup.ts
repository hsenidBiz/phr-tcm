import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// vitest runs without injected globals, so RTL's automatic cleanup never
// registers - without this, each test's DOM leaks into the next.
afterEach(() => cleanup());

// jsdom gaps that cmdk relies on.
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
