import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach, vi } from "vitest";

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

// jsdom has no IntersectionObserver; the app's on-screen checks (ElectricLogo,
// Threads, hooks/useOnScreen) need one.
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

// The Auto Run screen reads its scripts in one batched call
// (`auto_run_load_scripts`, lib/scriptBatch). Tests describe scripts one
// case at a time (`auto_run_load_script`), which is what each case's answer
// is; a handler that does not answer the batch itself gets it assembled
// from those per-case answers, a thrown one becoming that case's error.
vi.mock("@tauri-apps/api/mocks", async (importOriginal) => {
  const real = await importOriginal<typeof import("@tauri-apps/api/mocks")>();
  type Handler = Parameters<typeof real.mockIPC>[0];
  const message = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  return {
    ...real,
    mockIPC: (handler: Handler, options?: Parameters<typeof real.mockIPC>[1]) =>
      real.mockIPC(async (cmd, args) => {
        if (cmd !== "auto_run_load_scripts") return handler(cmd, args);
        let own: unknown;
        try {
          own = await handler(cmd, args);
        } catch {
          own = undefined;
        }
        // A real answer is a list; a catch-all `return null` is not one.
        if (Array.isArray(own)) return own;
        const ids = (args as { caseIds: number[] }).caseIds;
        return Promise.all(
          ids.map(async (id) => {
            try {
              const script = await handler("auto_run_load_script", { caseId: id });
              return { case_id: id, script: script ?? null, error: null };
            } catch (e) {
              return { case_id: id, script: null, error: message(e) };
            }
          }),
        );
      }, options),
  };
});
