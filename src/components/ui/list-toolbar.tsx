import { useEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "../../lib/cn";
import { Switch } from "./switch";

/** The nearest ancestor that scrolls vertically, or null for the window. */
function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === "auto" || y === "scroll") return p;
  }
  return null;
}

/**
 * Whether a `sticky` element sitting at `top: -1px` is stuck: once stuck, its
 * top edge is one pixel above the top of what scrolls it, so it is no longer
 * wholly inside it. Watched against that scroller, not the window - the page
 * scrolls in a box under the context bar.
 */
function useStuck(fullWidth: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(false);
  // Sticky stops at the scroller's top PADDING, not its edge: the main page
  // scrolls in a box with 24px of top padding, and rows showed through that
  // gap above the stuck bar (owner, 2026-10-27: "close the gap between the
  // search bar and the top"). So the bar's top is minus that padding, minus
  // the one pixel the stuck check needs.
  const [top, setTop] = useState(-1);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const root = scrollParent(el);
    if (root) setTop(-(parseFloat(getComputedStyle(root).paddingTop) || 0) - 1);
    if (typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(
      ([e]) => {
        if (!e) return;
        const top = e.rootBounds?.top ?? 0;
        setStuck(e.intersectionRatio < 1 && e.boundingClientRect.top < top);
      },
      { root, threshold: [1] },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  // While stuck the bar reaches both sides of the scroller (owner,
  // 2026-10-27: "close the gap on the sides as well"): rows showed beside it
  // through the page gutter and a card's own padding. How far each side
  // reaches is measured from the bar's parent - its resting box - to the
  // scroller's visible edges, again whenever the scroller changes size.
  const [bleed, setBleed] = useState({ left: 0, right: 0 });
  useEffect(() => {
    const el = ref.current;
    const parent = el?.parentElement;
    const root = el ? scrollParent(el) : null;
    if (!stuck || !fullWidth || !parent || !root) {
      setBleed({ left: 0, right: 0 });
      return;
    }
    const measure = () => {
      const p = parent.getBoundingClientRect();
      const ps = getComputedStyle(parent);
      const r = root.getBoundingClientRect();
      const rootLeft = r.left + root.clientLeft;
      const contentLeft = p.left + parseFloat(ps.borderLeftWidth) + parseFloat(ps.paddingLeft);
      const contentRight = p.right - parseFloat(ps.borderRightWidth) - parseFloat(ps.paddingRight);
      setBleed({
        left: Math.max(0, Math.round(contentLeft - rootLeft)),
        right: Math.max(0, Math.round(rootLeft + root.clientWidth - contentRight)),
      });
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(root);
    return () => ro.disconnect();
  }, [stuck, fullWidth]);

  return [ref, stuck, top, bleed] as const;
}

/**
 * The one toolbar line of a list screen: a wide search first, then that
 * screen's filters, each 40px tall so the line is level. It wraps on a
 * narrow window and the search (`flex-1 min-w-64`) gives way before any
 * filter (`shrink-0`) is cut off. Pass `SearchInput` with `className="w-full"`.
 *
 * The filters are the row's own children, not each wrapped in a box: a
 * filter that renders nothing (a component returning null, a `cond &&` that
 * is "") must leave no empty box behind, or the row gets a double gap.
 *
 * It sticks to the top of the scrolling page (owner, 2026-10-27: "make it
 * sticky so a user can search even when they have scrolled down"), so it
 * paints its own background - the page's, or the card's with `surface` -
 * and sits above the rows (z-20, under dropdowns and the action dock at
 * z-40). Sticky holds only while its parent is on screen, so a screen puts
 * it in the same box as the list it filters.
 *
 * Its border shows only while it is stuck (owner, 2026-10-27: it "blends in
 * with the background" once rows scroll under it, but no border "when the
 * user has scrolled all the way to the top"). The border is always there,
 * just transparent at rest, so nothing moves when it appears.
 *
 * Stuck, it reaches both sides of the scroller - unless the list shares the
 * page with another column (Auto Run's Setup panel beside its case list):
 * there `fullWidth={false}` keeps it in its own column, a bordered box all
 * round instead (owner, 2026-10-27: "the bar crosses the setup screen").
 */
export function ListToolbar({
  search,
  surface = false,
  fullWidth = true,
  children,
}: {
  search: ReactNode;
  /** It sits on a card (`bg-surface`) rather than on the page. */
  surface?: boolean;
  /** Stuck, reach the scroller's sides (false: stay in its own column). */
  fullWidth?: boolean;
  children?: ReactNode;
}) {
  const [ref, stuck, top, bleed] = useStuck(fullWidth);
  // Stuck, it reaches out by `bleed` on each side and pads back in by the
  // same amount, so its controls stay exactly where they were.
  const style = stuck && fullWidth
    ? {
        top,
        marginLeft: -bleed.left,
        marginRight: -bleed.right,
        paddingLeft: bleed.left + 8,
        paddingRight: bleed.right + 8,
      }
    : { top };
  return (
    <div
      ref={ref}
      data-list-toolbar
      data-stuck={stuck ? "" : undefined}
      style={style}
      className={cn(
        "sticky z-20 flex flex-wrap items-center gap-2 border p-2 transition-colors *:shrink-0",
        // At rest a borderless rounded box; stuck, a full-width bar with a
        // line under it. The border is always 1px, so nothing moves.
        !stuck
          ? "rounded-md border-transparent"
          : fullWidth
            ? "rounded-none border-transparent border-b-border"
            : "rounded-md border-border",
        surface ? "bg-surface" : "bg-bg",
      )}
    >
      {/* Basis 0 and growing, so shrink-0 costs it nothing: it takes the
          free width and wraps the filters before going under min-w-64. */}
      <div className="min-w-64 flex-1">{search}</div>
      {children}
    </div>
  );
}

/**
 * An on/off filter for a toolbar: the app's switch and its label, 40px tall
 * so it sits level with the search. No border (owner, 2026-10-27): only the
 * search and real buttons are boxed, so a setting does not read as a button
 * and sits like the plain checkboxes beside it. The label wraps the switch,
 * so clicking either toggles it; the switch is named by `label`.
 */
export function ToolbarSwitch({
  checked,
  onCheckedChange,
  label,
  title,
}: {
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
  label: string;
  title?: string;
}) {
  return (
    <label
      title={title}
      className="flex h-10 shrink-0 items-center gap-2 px-1 text-sm text-muted"
    >
      <Switch checked={checked} onCheckedChange={onCheckedChange} ariaLabel={label} />
      {label}
    </label>
  );
}
