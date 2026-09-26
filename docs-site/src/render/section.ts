// One screen: heading and summary, then a stage per shot (the annotated
// figure with its numbered control list beside it), then tips and how-tos.

import type { Box, Control, Screen, SiteContent } from "../types";
import { h, rich } from "./dom";
import { icon } from "./icons";
import { renderShot, type ShotView } from "./shot";
import type { Theme } from "./theme";

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
};

export const controlAnchor = (screen: string, control: string) => `${screen}/${control}`;

export function renderSection(
  screen: Screen,
  content: SiteContent,
  env: { theme: Theme; motion: boolean; onRowClick: (ref: ControlRef) => void },
): SectionView {
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
  const available = new Set(content.available);
  const multi = screen.shots.length > 1;

  for (const shot of screen.shots) {
    const onShot = readingOrder(
      screen.controls.filter((c) => c.shot === shot.id),
      content.positions[shot.id]?.controls,
    ).map((control, i) => ({ control, n: i + 1 }));
    const rows = new Map<string, HTMLButtonElement>();

    const view = renderShot({
      shot,
      controls: onShot,
      placed: content.positions[shot.id],
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
    });
    views.push(view);

    const list = h("ol", { class: "controls", "aria-label": `Controls on ${shot.alt}` });
    for (const { control, n } of onShot) {
      const row = controlRow(screen, control, n, view.has(control.id));
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

    section.appendChild(
      h(
        "div",
        { class: onShot.length ? "stage" : "stage is-solo" },
        multi ? h("p", { class: "stage-label", "aria-hidden": "true" }, shot.alt) : null,
        view.figure,
        onShot.length ? list : null,
      ),
    );
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

  return { section, controls: refs, views };
}

/** Controls on a shot in reading order: placed ones by visual row (centres
 *  within ROW_TOLERANCE shot px share a row), each row left to right; then
 *  any control without a position yet, in the order the content lists them. */
const ROW_TOLERANCE = 16;
export function readingOrder(controls: Control[], boxes: Record<string, Box> | undefined): Control[] {
  const placed = controls
    .filter((c) => boxes?.[c.id])
    .map((c) => ({ c, b: boxes![c.id] }))
    .sort((a, z) => a.b.y + a.b.h / 2 - (z.b.y + z.b.h / 2));
  const rows: (typeof placed)[] = [];
  for (const item of placed) {
    const row = rows[rows.length - 1];
    const cy = item.b.y + item.b.h / 2;
    if (row && cy - (row[0].b.y + row[0].b.h / 2) <= ROW_TOLERANCE) row.push(item);
    else rows.push([item]);
  }
  const ordered = rows.flatMap((row) => row.sort((a, z) => a.b.x - z.b.x)).map((i) => i.c);
  return [...ordered, ...controls.filter((c) => !boxes?.[c.id])];
}

function controlRow(screen: Screen, c: Control, n: number, placed: boolean): HTMLButtonElement {
  return h(
    "button",
    {
      type: "button",
      class: "row",
      id: controlAnchor(screen.id, c.id),
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
