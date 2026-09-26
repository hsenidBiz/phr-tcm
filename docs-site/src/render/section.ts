// One screen: heading and summary, then a stage per figure (the annotated
// shot with its numbered control list beside or below it), then tips and
// how-tos. A screen split into groups (a busy one) shows each shot that
// spans several groups once as an overview - a map whose outlined areas
// link to the subsections - then one subsection per group, each with a
// zoomed crop of the shot around that group's controls.

import { shotSize, type Control, type ControlGroup, type Screen, type Shot, type SiteContent } from "../types";
import { h, rich } from "./dom";
import { icon } from "./icons";
import { figureMaxHeight, figureWidth, planScreen, stageFlow, type Region, type Stage } from "./plan";
import { pairHover, watchOverview } from "./overview-labels";
import { placeholder, renderShot, type ShotView } from "./shot";
import { currentTheme, shotSrc, type Theme } from "./theme";
import type { ViewerContent } from "./viewer";

export { readingOrder } from "./plan";

export type ControlRef = {
  screen: string;
  id: string;
  row: HTMLButtonElement;
  view: ShotView;
};

export type SectionView = {
  section: HTMLElement;
  controls: ControlRef[];
  views: ShotView[];
  /** A grouped screen's subsections (#screen/group), in page order. */
  subsections: HTMLElement[];
};

export type SectionEnv = {
  theme: Theme;
  motion: boolean;
  onRowClick: (ref: ControlRef) => void;
  /** Opens a figure full screen; `from` gets focus back when it closes. */
  openViewer: (build: () => ViewerContent, from: HTMLElement) => void;
};

export const controlAnchor = (screen: string, control: string) => `${screen}/${control}`;

const pct = (n: number, of: number) => `${+((n / of) * 100).toFixed(4)}%`;

export function renderSection(screen: Screen, content: SiteContent, env: SectionEnv): SectionView {
  const titleId = `${screen.id}--title`;
  const section = h(
    "section",
    { id: screen.id, class: "screen", "aria-labelledby": titleId, tabindex: "-1" },
    h(
      "header",
      { class: "screen-head" },
      h("p", { class: "eyebrow" }, screen.group),
      h("h2", { id: titleId }, screen.title),
      h("p", { class: "summary" }, rich(screen.summary)),
    ),
  );

  const refs: ControlRef[] = [];
  const views: ShotView[] = [];
  const subsections: HTMLElement[] = [];
  const available = new Set(content.available);
  const plan = planScreen(screen, content.positions);

  function stageEl(stage: Stage, label: string | null, group: ControlGroup | null): HTMLElement {
    const { shot } = stage;
    const rows = new Map<string, HTMLButtonElement>();
    // On a grouped screen the same shot appears once per group: name the
    // figure by its group (and its shot, when the group has several).
    const name = group ? (label ? `${group.title}, ${shot.alt}` : group.title) : shot.alt;

    const view = renderShot({
      shot,
      controls: stage.controls,
      placed: content.positions[shot.id],
      view: stage.view,
      available: available.has(shot.id),
      theme: env.theme,
      motion: env.motion,
      onActive: (id) => {
        for (const [cid, row] of rows) row.toggleAttribute("data-active", cid === id);
      },
      onMarkerClick: (id) => {
        const ref = refs.find((r) => r.view === view && r.id === id);
        if (ref) env.onRowClick(ref);
      },
      onExpand: (from) => env.openViewer(() => viewerContent(screen, content, env, stage, group), from),
      name,
    });
    views.push(view);

    const list = h("ol", { class: "controls", "aria-label": group ? `Controls in ${name}` : `Controls on ${shot.alt}` });
    for (const { control, n } of stage.controls) {
      const row = controlRow(control, n, view.has(control.id), controlAnchor(screen.id, control.id));
      rows.set(control.id, row);
      const ref: ControlRef = { screen: screen.id, id: control.id, row, view };
      refs.push(ref);
      row.addEventListener("mouseenter", () => view.hover(control.id));
      row.addEventListener("mouseleave", () => view.hover(null));
      row.addEventListener("focus", () => view.hover(control.id));
      row.addEventListener("blur", () => view.hover(null));
      row.addEventListener("click", () => env.onRowClick(ref));
      list.appendChild(h("li", {}, row));
    }

    const listed = stage.controls.length > 0;
    const el = h(
      "div",
      { class: listed ? "stage" : "stage is-solo", "data-flow": "below" },
      label ? h("p", { class: "stage-label", "aria-hidden": "true" }, label) : null,
      view.figure,
      listed ? list : null,
    );
    watchFlow(el, stage, listed);
    return el;
  }

  if (!plan.grouped) {
    const multi = plan.stages.length > 1;
    for (const stage of plan.stages) section.appendChild(stageEl(stage, multi ? stage.shot.alt : null, null));
  } else {
    if (plan.overviews.length) {
      section.appendChild(
        h(
          "div",
          { class: plan.overviews.length > 1 ? "overviews is-multi" : "overviews" },
          ...plan.overviews.map((o) => overview(screen, o.shot, o.regions, available.has(o.shot.id), env.theme)),
        ),
      );
    }
    for (const { group, stages } of plan.groups) {
      const id = controlAnchor(screen.id, group.id);
      const subTitle = `${id}--title`;
      const sub = h(
        "section",
        { id, class: "subsection", "aria-labelledby": subTitle, tabindex: "-1" },
        h(
          "header",
          { class: "subsection-head" },
          h("h3", { id: subTitle }, group.title),
          group.summary ? h("p", { class: "subsection-summary" }, rich(group.summary)) : null,
        ),
      );
      const multi = stages.length > 1;
      for (const stage of stages) sub.appendChild(stageEl(stage, multi ? stage.shot.alt : null, group));
      section.appendChild(sub);
      subsections.push(sub);
    }
  }

  const notes = h("div", { class: "screen-notes" });
  if (screen.tips?.length) {
    notes.appendChild(
      h(
        "aside",
        { class: "callout tips", "aria-label": `Tips for ${screen.title}` },
        h("div", { class: "callout-icon" }, icon("tip")),
        h(
          "div",
          { class: "callout-body" },
          h("p", { class: "callout-title" }, "Good to know"),
          h("ul", {}, ...screen.tips.map((t) => h("li", {}, rich(t)))),
        ),
      ),
    );
  }
  for (const how of screen.howTo ?? []) {
    notes.appendChild(
      h(
        "div",
        { class: "howto" },
        h("h3", {}, icon("steps", 16), h("span", {}, how.title)),
        h("ol", {}, ...how.steps.map((s) => h("li", {}, rich(s)))),
      ),
    );
  }
  if (notes.childElementCount) section.appendChild(notes);

  return { section, controls: refs, views, subsections };
}

/** Keeps a stage in step with its width and the window's height: how wide
 *  its figure is drawn (--fig-w: capped so it fits on screen), and whether
 *  the list goes beside the figure (both fit) or below it (in two columns
 *  when there is room). */
const flowSyncs = new Set<() => void>();
let onWindowResize: (() => void) | null = null;
function watchFlow(el: HTMLElement, stage: Stage, listed: boolean) {
  const sync = () => {
    const fig = figureWidth(stage, figureMaxHeight(window.innerHeight));
    el.style.setProperty("--fig-w", String(fig));
    const next = listed ? stageFlow(el.clientWidth, fig) : "below";
    if (el.dataset.flow !== next) el.dataset.flow = next;
  };
  sync();
  if (typeof ResizeObserver !== "undefined") new ResizeObserver(sync).observe(el);
  // One listener for every stage: a window resized only in height changes
  // the cap without resizing any stage.
  flowSyncs.add(sync);
  if (!onWindowResize) {
    onWindowResize = () => flowSyncs.forEach((s) => s());
    window.addEventListener("resize", onWindowResize);
  }
}

/** Forgets every stage (the page is being torn down). */
export function releaseStages() {
  flowSyncs.clear();
  if (onWindowResize) window.removeEventListener("resize", onWindowResize);
  onWindowResize = null;
}

/** A grouped screen's map: the whole shot with an outlined area per group.
 *  Each area's label sits outside the shot (overview-labels.ts), with a
 *  leader to its outline, so no label covers what it names; the labels are
 *  the links to the subsections, and an area is a mouse shortcut to the
 *  same place. No numbered markers. */
function overview(screen: Screen, shot: Shot, regions: Region[], has: boolean, theme: Theme): HTMLElement {
  const size = shotSize(shot);
  const media = has
    ? h("img", { "data-shot": shot.id, src: shotSrc(theme, shot.id), alt: shot.alt, width: size.w, height: size.h, loading: "lazy", decoding: "async" })
    : placeholder(shot.alt);
  const above = h("div", { class: "ov-strip" });
  const below = h("div", { class: "ov-strip is-below" });
  const areas = regions.map(({ group, box }) => {
    const href = `#${controlAnchor(screen.id, group.id)}`;
    // A part in the lower half is labelled below the shot, nearer to it.
    const strip = box.y + box.h / 2 > size.h / 2 ? below : above;
    strip.appendChild(h("a", { class: "ov-label", href, "data-g": group.id }, group.title));
    const a = h("a", { class: "region", href, "data-g": group.id, tabindex: "-1", "aria-hidden": "true" });
    Object.assign(a.style, { left: pct(box.x, size.w), top: pct(box.y, size.h), width: pct(box.w, size.w), height: pct(box.h, size.h) });
    return a;
  });
  const frame = h("div", { class: size.w < 1440 ? "frame is-narrow" : "frame" }, media, h("div", { class: "regions" }, ...areas));
  frame.style.setProperty("--shot-w", String(size.w));
  frame.style.setProperty("--shot-h", String(size.h));
  frame.style.setProperty("--fig-w", String(size.w));
  const ov = h(
    "nav",
    { class: "ov", "aria-label": `Parts of ${shot.alt}` },
    above.childElementCount ? above : null,
    frame,
    below.childElementCount ? below : null,
  );
  pairHover(ov);
  watchOverview(ov);
  return h(
    "figure",
    { class: "shot overview", "data-overview": shot.id },
    ov,
    h("figcaption", { class: "overview-caption" }, icon("section", 14), h("span", {}, "Pick a part of the screen to go to its section.")),
  );
}

/** The full-screen view of one stage: a fresh copy of the figure (in the
 *  theme showing now) and a compact list with the same spotlight. */
function viewerContent(screen: Screen, content: SiteContent, env: SectionEnv, stage: Stage, group: ControlGroup | null): ViewerContent {
  const { shot } = stage;
  const rows = new Map<string, HTMLButtonElement>();
  const toggle = (id: string) => {
    const next = view.pinned() === id ? null : id;
    view.pin(next);
    for (const [cid, row] of rows) row.toggleAttribute("data-pinned", cid === next);
    if (next) rows.get(next)?.scrollIntoView?.({ block: "nearest" });
  };
  const view: ShotView = renderShot({
    shot,
    controls: stage.controls,
    placed: content.positions[shot.id],
    view: stage.view,
    available: content.available.includes(shot.id),
    theme: currentTheme(),
    motion: env.motion,
    onActive: (id) => {
      for (const [cid, row] of rows) row.toggleAttribute("data-active", cid === id);
    },
    onMarkerClick: toggle,
  });
  const list = h("ol", { class: "controls is-compact", "aria-label": `Controls on ${shot.alt}` });
  for (const { control, n } of stage.controls) {
    const row = controlRow(control, n, view.has(control.id), null);
    rows.set(control.id, row);
    row.addEventListener("mouseenter", () => view.hover(control.id));
    row.addEventListener("mouseleave", () => view.hover(null));
    row.addEventListener("focus", () => view.hover(control.id));
    row.addEventListener("blur", () => view.hover(null));
    row.addEventListener("click", () => toggle(control.id));
    list.appendChild(h("li", {}, row));
  }
  return {
    title: shot.alt,
    context: group ? `${screen.title} · ${group.title}` : screen.title,
    figure: view.figure,
    list: stage.controls.length ? list : null,
  };
}

function controlRow(c: Control, n: number, placed: boolean, id: string | null): HTMLButtonElement {
  return h(
    "button",
    {
      type: "button",
      class: "row",
      id,
      "data-for": c.id,
      "data-placed": placed ? "true" : "false",
    },
    h("span", { class: "row-num", "aria-hidden": "true" }, String(n)),
    h(
      "span",
      { class: "row-text" },
      h("span", { class: "row-name" }, rich(c.name)),
      h("span", { class: "row-does" }, rich(c.does)),
      ...(c.tips ?? []).map((t) => h("span", { class: "row-tip" }, rich(t))),
    ),
  );
}
