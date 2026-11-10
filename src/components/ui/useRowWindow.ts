/**
 * Renders only the rows of a long list that are near the visible part of
 * the page (backlog A1.5).
 *
 * The screens scroll inside App's <main>, not inside the list, so the list
 * finds its scrolling ancestor itself and tells the virtualizer where it
 * starts in it (`scrollMargin`). Several lists on one page (one per group)
 * each get their own window over the same scroller.
 *
 * Below `WINDOW_FROM` rows every row is rendered, as before: a short list
 * gains nothing, and jsdom (which does no layout) keeps seeing every row in
 * the tests. Row heights are measured as rows render, so an expanded row
 * (Run Tests' step preview) takes the room it needs.
 *
 * The caller renders `items`, each after a spacer of `space` pixels when
 * that is more than 0 (rows skipped before it), then a spacer of `after`
 * pixels, and passes `measure` as the ref of each row's outer element,
 * with `data-index={item.index}` on it.
 *
 * `pinned` rows are always rendered, wherever the page is scrolled: an
 * open editor must never be unmounted (its unsaved edits would go).
 */
import { defaultRangeExtractor, useVirtualizer, type Range, type VirtualItem } from "@tanstack/react-virtual";
import { useCallback, useLayoutEffect, useRef, useState } from "react";

const NO_MEASURE = () => {};

/** Lists shorter than this render every row. */
export const WINDOW_FROM = 150;

function scrollParent(el: HTMLElement | null): HTMLElement | null {
  for (let p = el?.parentElement ?? null; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === "auto" || y === "scroll") return p;
  }
  return null;
}

export type RowWindow = {
  /** Whether the list is windowed at all (long enough). */
  windowed: boolean;
  /** The rows to render: every row when not windowed. `space` is the
   * height of the rows skipped just before this one (0 when none). */
  items: { index: number; key: number | string; space: number }[];
  /** The height of the rows skipped after the last rendered one. */
  after: number;
  /** Ref for the list's own outer element. */
  listRef: (el: HTMLElement | null) => void;
  /** Ref for each rendered row's outer element (it must carry data-index). */
  measure: (el: Element | null) => void;
};

export function useRowWindow(
  count: number,
  opts: {
    estimate: number;
    keyOf?: (index: number) => number | string;
    /** Space between rows in pixels (a `space-y-*` list's margin). */
    gap?: number;
    /** Rows always rendered (open editors). */
    pinned?: readonly number[];
  },
): RowWindow {
  const windowed = count >= WINDOW_FROM;
  const [list, setList] = useState<HTMLElement | null>(null);
  const scroller = useRef<HTMLElement | null>(null);
  const [margin, setMargin] = useState(0);
  const listRef = useCallback((el: HTMLElement | null) => {
    setList(el);
    scroller.current = scrollParent(el);
  }, []);

  // Where the list starts inside its scroller. It moves when anything above
  // it changes height (a group folding open, a toolbar wrapping), so it is
  // checked after every render and whenever the scroller resizes.
  useLayoutEffect(() => {
    if (!windowed || !list || !scroller.current) return;
    const s = scroller.current;
    const top = list.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
    if (Math.abs(top - margin) > 1) setMargin(top);
  });

  const keyOf = opts.keyOf;
  const pinnedKey = (opts.pinned ?? []).join(",");
  const rangeExtractor = useCallback(
    (range: Range) => {
      const base = defaultRangeExtractor(range);
      if (!pinnedKey) return base;
      const extra = pinnedKey.split(",").map(Number).filter((i) => i >= 0 && i < range.count);
      return [...new Set([...base, ...extra])].sort((a, b) => a - b);
    },
    [pinnedKey],
  );
  const virtualizer = useVirtualizer({
    count: windowed ? count : 0,
    getScrollElement: () => scroller.current,
    estimateSize: () => opts.estimate,
    overscan: 12,
    gap: opts.gap ?? 0,
    rangeExtractor,
    scrollMargin: margin,
    getItemKey: keyOf ? (i) => keyOf(i) : undefined,
  });

  if (!windowed) {
    return {
      windowed,
      items: Array.from({ length: count }, (_, index) => ({ index, key: keyOf ? keyOf(index) : index, space: 0 })),
      after: 0,
      listRef,
      // One function for every render, so memoized rows stay memoized.
      measure: NO_MEASURE,
    };
  }
  const gap = opts.gap ?? 0;
  const virtual: VirtualItem[] = virtualizer.getVirtualItems();
  const items = virtual.map((v, i) => {
    const prev = virtual[i - 1];
    const space = v.start - (prev ? prev.end + gap : margin);
    return { index: v.index, key: v.key as number | string, space: space > 0.5 ? space : 0 };
  });
  const last = virtual[virtual.length - 1];
  const total = virtualizer.getTotalSize();
  return {
    windowed,
    items,
    after: last ? Math.max(0, total - (last.end - margin)) : 0,
    listRef,
    measure: virtualizer.measureElement,
  };
}
