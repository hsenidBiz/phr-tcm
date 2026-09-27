import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type RefObject } from "react";
import { EASE, foldMs, visibleSpan } from "../ui/collapse";
import { reducedMotion } from "../../lib/exitGhost";

/**
 * Where the Settings cards sit, and the slide that moves them.
 *
 * On a wide window the screen is two columns. While the changelog shows
 * only its latest version, the cards that are not about the look of the
 * app (the `moving` ones) sit under it in the right column, so the space
 * under a short changelog is not left empty. Opening the full history
 * needs that column, so the cards slide over to the bottom of the left
 * column first - one after another, like tiles in a sliding puzzle - and
 * only then does the history unfold. Closing it runs the other way: the
 * history folds, then the cards slide back.
 *
 * The slide is a FLIP: every card carries `data-settings-card="<id>"`, its
 * position is read before the change and again after it, and it is played
 * back from the old place to the new one. Positions are always measured,
 * never assumed, so it fits any window. A card may be a new element after
 * it changes column - the measurements are keyed by the id, not the node.
 *
 * Below the breakpoint there is one column and nothing moves; under
 * reduced motion the layout and the expansion change at once.
 */

/** Tailwind's `lg` - the width the Settings grid turns two columns at. */
export const WIDE_QUERY = "(min-width: 64rem)";

/** The attribute every card carries, naming it for the measurements. */
export const CARD_ATTR = "data-settings-card";

/** How long one card takes to glide to its new place. */
export const TILE_MS = 420;
/** The gap between one moving card setting off and the next. */
export const TILE_STAGGER_MS = 70;
/** Slack on top of a timed wait, for an animation or a fold that never
 * reports its end - the sequence must not stall on one. */
const GRACE_MS = 60;

/** Where the moving cards render: under everything in one column, or at
 * the bottom of the left or the right column of the wide layout. */
export type TilePlacement = "single" | "left" | "right";

function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const mq = window.matchMedia?.(query);
      if (!mq) return () => {};
      if (mq.addEventListener) {
        mq.addEventListener("change", onChange);
        return () => mq.removeEventListener("change", onChange);
      }
      mq.addListener?.(onChange);
      return () => mq.removeListener?.(onChange);
    },
    [query],
  );
  return useSyncExternalStore(subscribe, () => window.matchMedia?.(query).matches ?? false);
}

const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

/** Every card's place on screen, by id. */
function measure(root: HTMLElement): Map<string, DOMRect> {
  const rects = new Map<string, DOMRect>();
  for (const el of root.querySelectorAll<HTMLElement>(`[${CARD_ATTR}]`)) {
    rects.set(el.getAttribute(CARD_ATTR)!, el.getBoundingClientRect());
  }
  return rects;
}

/**
 * Play every card that moved from where it was (`before`) to where it is
 * now. The `moving` cards set off one after another in that order; a card
 * that only shifts to make room goes at once when it moves down, and with
 * the last moving card when it moves up, so it never slides over one that
 * has not left yet.
 */
function glide(root: HTMLElement, before: Map<string, DOMRect>, moving: readonly string[]): Animation[] {
  const lastDelay = Math.max(0, moving.length - 1) * TILE_STAGGER_MS;
  const moves: { el: HTMLElement; dx: number; dy: number; delay: number; tile: boolean }[] = [];
  // Every read before any write, so the measuring costs one layout.
  for (const el of root.querySelectorAll<HTMLElement>(`[${CARD_ATTR}]`)) {
    const id = el.getAttribute(CARD_ATTR)!;
    const from = before.get(id);
    if (!from || typeof el.animate !== "function") continue;
    const to = el.getBoundingClientRect();
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
    const i = moving.indexOf(id);
    const delay = i >= 0 ? i * TILE_STAGGER_MS : dy > 0 ? lastDelay : 0;
    moves.push({ el, dx, dy, delay, tile: i >= 0 });
  }
  return moves.map(({ el, dx, dy, delay, tile }) => {
    // A moving card passes over the others' edges on its way: it goes on top.
    if (tile) {
      el.style.position = "relative";
      el.style.zIndex = "1";
    }
    const a = el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], {
      duration: TILE_MS,
      easing: EASE,
      delay,
      // Held at the old place while it waits its turn.
      fill: "backwards",
    });
    const done = () => {
      el.style.position = "";
      el.style.zIndex = "";
    };
    a.onfinish = done;
    a.oncancel = done;
    return a;
  });
}

/** Resolves once every animation has ended (or been cancelled), or after
 * the longest of them plus some slack if one never says so. */
function settled(anims: Animation[]): Promise<void> {
  if (anims.length === 0) return Promise.resolve();
  const longest = Math.max(
    ...anims.map((a) => {
      const t = a.effect?.getTiming();
      return Number(t?.duration ?? TILE_MS) + Number(t?.delay ?? 0);
    }),
  );
  const ended = Promise.all(anims.map((a) => a.finished.catch(() => undefined))).then(() => undefined);
  return Promise.race([ended, wait(longest + GRACE_MS)]);
}

type Pending = {
  rects: Map<string, DOMRect>;
  placement: TilePlacement;
  done?: () => void;
};

export function useTileLayout({
  rootRef,
  moving,
  fold,
  changelogShown,
}: {
  /** The element around every card - measurements are taken inside it. */
  rootRef: RefObject<HTMLElement | null>;
  /** The ids of the cards that change column, in the order they set off. */
  moving: readonly string[];
  /** The folding box of the history, while it is open - its closing is
   * waited out before the cards move back. */
  fold: () => HTMLElement | null;
  /** False while the right column shows something other than the
   * changelog (the app log): the cards then stay under it. */
  changelogShown: boolean;
}) {
  const wide = useMediaQuery(WIDE_QUERY);
  // Where the moving cards sit while the changelog shows, and whether its
  // history is open. The two agree except while a sequence runs.
  const [side, setSide] = useState<"left" | "right">("right");
  const [expanded, setExpanded] = useState(false);
  const placement: TilePlacement = !wide ? "single" : changelogShown ? side : "right";

  const pending = useRef<Pending | null>(null);
  const running = useRef<Animation[]>([]);
  const busy = useRef(false);
  const alive = useRef(true);
  const latest = useRef({ placement, wide });
  useLayoutEffect(() => {
    latest.current = { placement, wide };
  });

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      for (const a of running.current) a.cancel();
      running.current = [];
    };
  }, []);

  // The render after a `flip`: play back whatever moved. Only a change of
  // placement moves the cards on purpose - anything else (a taller panel)
  // lands as it always has.
  useLayoutEffect(() => {
    const p = pending.current;
    if (!p) return;
    pending.current = null;
    const root = rootRef.current;
    if (!root || !wide || p.placement === placement || p.placement === "single") {
      p.done?.();
      return;
    }
    // A slide still under way gives way to this one, which starts from
    // wherever the cards were caught (the snapshot included the motion).
    for (const a of running.current) a.cancel();
    const anims = glide(root, p.rects, moving);
    running.current = anims;
    void settled(anims).then(() => {
      if (running.current === anims) running.current = [];
      p.done?.();
    });
  });

  /** Apply `change` (a state update that may move the cards), gliding the
   * cards it moves. Without the wide layout, or under reduced motion, it
   * is just `change()`. */
  const flip = useCallback(
    (change: () => void, done?: () => void) => {
      const root = rootRef.current;
      if (root && latest.current.wide && !reducedMotion()) {
        pending.current = { rects: measure(root), placement: latest.current.placement, done };
      } else {
        done?.();
      }
      change();
    },
    [rootRef],
  );

  const slide = useCallback(
    (to: "left" | "right") =>
      new Promise<void>((resolve) => {
        flip(() => setSide(to), resolve);
      }),
    [flip],
  );

  /** Show more / Show less: the cards and the history, in their order. A
   * click while a sequence runs is ignored, so the two cannot end up out
   * of step. */
  const toggle = useCallback(() => {
    if (busy.current) return;
    const opening = !expanded;
    const to = opening ? "left" : "right";
    if (!latest.current.wide || reducedMotion() || side === to) {
      setSide(to);
      setExpanded(opening);
      return;
    }
    busy.current = true;
    void (async () => {
      try {
        if (opening) {
          await slide(to);
          if (!alive.current) return;
          setExpanded(true);
        } else {
          // What the fold will take: the same measure Collapse plays its
          // closing by, and nothing when none of it is on screen.
          const box = fold();
          const span = box ? visibleSpan(box) : 0;
          setExpanded(false);
          await wait(span < 2 ? 0 : foldMs(span) + GRACE_MS);
          if (!alive.current) return;
          await slide(to);
        }
      } finally {
        busy.current = false;
      }
    })();
  }, [expanded, side, slide, fold]);

  return { placement, expanded, toggle, flip };
}
