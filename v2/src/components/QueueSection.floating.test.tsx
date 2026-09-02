/**
 * The floating copy of the queue's main button, with the REAL useOnScreen
 * behind it.
 *
 * QueueSection.test.tsx mocks that hook out, which is fine for asking what
 * the copy looks like - but it cannot see the flow the copy was built for:
 * an EMPTY queue that a 100-case import then fills. The action row does
 * not exist while the queue is empty, so the hook has to start watching
 * when the row arrives. Mocked out, that is invisible; here it is the
 * whole test.
 */
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { TestCase } from "../bindings";
import QueueSection from "./QueueSection";

const real = globalThis.IntersectionObserver;
afterEach(() => {
  globalThis.IntersectionObserver = real;
  clearMocks();
  localStorage.clear();
});

/** jsdom's own stand-in is a no-op that never calls anything back, so the
 * test brings an observer it can fire by hand. */
function stubObserver() {
  const watched: { el: Element; cb: (e: { isIntersecting: boolean }[]) => void }[] = [];
  globalThis.IntersectionObserver = class {
    cb: (e: { isIntersecting: boolean }[]) => void;
    constructor(cb: (e: { isIntersecting: boolean }[]) => void) {
      this.cb = cb;
    }
    observe(el: Element) {
      watched.push({ el, cb: this.cb });
    }
    unobserve(el: Element) {
      const i = watched.findIndex((w) => w.el === el);
      if (i >= 0) watched.splice(i, 1);
    }
    disconnect() {
      for (let i = watched.length - 1; i >= 0; i--) if (watched[i].cb === this.cb) watched.splice(i, 1);
    }
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
  return {
    watched,
    /** Report every watched element as off/on screen. */
    report(isIntersecting: boolean) {
      act(() => {
        for (const w of [...watched]) w.cb([{ isIntersecting }]);
      });
    },
  };
}

function makeCase(title: string): TestCase {
  return {
    title,
    steps: [{ action: "Open page", expected: "Page shown" }],
    tags: "smoke",
    automation_status: "Not Automated",
    module_value: "",
    preconditions: "",
    update_id: null,
    spec_order: null,
    tester_order: null,
  };
}

/** Owns the queue the way Import File does: empty on arrival, filled when
 * a file lands. */
function Harness() {
  const [queue, setQueue] = useState<TestCase[]>([]);
  return (
    <>
      <button onClick={() => setQueue([makeCase("Login works"), makeCase("Logout works")])}>
        Import a file
      </button>
      <QueueSection org="acme" project="Web" pbiId={42} queue={queue} setQueue={setQueue} />
    </>
  );
}

function renderHarness() {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return [];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness />
    </QueryClientProvider>,
  );
}

test("a queue that starts EMPTY still gets the floating button once it fills", async () => {
  const io = stubObserver();
  renderHarness();

  // Nothing to watch and nothing to float: the whole section is out of the
  // way while the queue is empty.
  expect(document.querySelector("[data-sticky-action]")).toBeNull();
  expect(io.watched).toHaveLength(0);

  fireEvent.click(screen.getByRole("button", { name: "Import a file" }));
  await screen.findByRole("button", { name: /Review 2 test cases/ });

  // The action row arrived after mount, and the hook picked it up - this
  // is the assertion a ref-reading hook fails: it looked once, at mount,
  // when there was nothing there.
  await waitFor(() => expect(io.watched).toHaveLength(1));

  // Assumed on screen until told otherwise, so the copy is still hidden.
  const floating = document.querySelector("[data-sticky-action]") as HTMLElement;
  expect(floating.className).toContain("opacity-0");

  // Scrolled past the real row: the copy comes up, saying the same thing.
  io.report(false);
  expect(floating.className).toContain("opacity-100");
  expect(floating).toHaveTextContent("Review 2 test cases");
  expect(floating).toHaveAttribute("aria-hidden");

  // And back down again when the real row returns.
  io.report(true);
  expect(floating.className).toContain("opacity-0");
  expect(floating.className).toContain("pointer-events-none");
});
