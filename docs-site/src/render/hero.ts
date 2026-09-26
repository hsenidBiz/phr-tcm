// The opening: a one-line promise, the app floating in a window frame over
// an accent glow, and the Quick start strip into the sections.

import { SHOT_HEIGHT, SHOT_WIDTH, type SiteContent } from "../types";
import { h, rich } from "./dom";
import { icon } from "./icons";
import { placeholder } from "./shot";
import { shotSrc, type Theme } from "./theme";

export const PRODUCT = "Test Case Manager";

export function renderHero(
  content: SiteContent,
  env: { theme: Theme; appIcon: string; exists: (id: string) => boolean; searchButton: HTMLElement },
): HTMLElement[] {
  const { intro } = content;
  const heroShot = intro.heroShot;
  const hasImage = !!heroShot && content.available.includes(heroShot);
  const firstScreen = content.screens[0];

  const windowFrame = h(
    "div",
    { class: "window" },
    h(
      "div",
      { class: "window-bar", "aria-hidden": "true" },
      h("img", { class: "window-icon", src: env.appIcon, alt: "", width: 16, height: 16 }),
      h("span", { class: "window-title" }, PRODUCT),
      h("span", { class: "window-controls" }, h("i", { class: "wc-min" }), h("i", { class: "wc-max" }), h("i", { class: "wc-close" })),
    ),
    h(
      "div",
      { class: "window-body" },
      hasImage
        ? h("img", {
            "data-shot": heroShot,
            src: shotSrc(env.theme, heroShot!),
            alt: `${PRODUCT}, the main window`,
            width: SHOT_WIDTH,
            height: SHOT_HEIGHT,
            decoding: "async",
          })
        : placeholder(`${PRODUCT}, the main window`),
    ),
  );

  const hero = h(
    "header",
    { class: "hero", id: "top" },
    h(
      "div",
      { class: "hero-copy" },
      h("p", { class: "hero-eyebrow" }, h("span", { class: "hero-dot", "aria-hidden": "true" }), `How To Use ${PRODUCT}`),
      h("h1", {}, rich(intro.promise)),
      h("p", { class: "hero-lead" }, rich(intro.lead)),
      h(
        "div",
        { class: "hero-actions" },
        firstScreen
          ? h("a", { class: "btn btn-primary", href: `#${firstScreen.id}` }, h("span", {}, "Start with the basics"), icon("arrow", 16))
          : null,
        env.searchButton,
      ),
    ),
    h(
      "div",
      { class: "hero-visual" },
      h("div", { class: "hero-glow", "aria-hidden": "true" }),
      h("div", { class: "hero-grid", "aria-hidden": "true" }),
      windowFrame,
    ),
  );

  const steps = intro.quickStart.map((step, i) => {
    const inner = [
      h("span", { class: "qs-num", "aria-hidden": "true" }, String(i + 1).padStart(2, "0")),
      h("span", { class: "qs-label" }, step.label),
      h("span", { class: "qs-hint" }, step.hint),
    ];
    return h(
      "li",
      { class: "qs-step" },
      env.exists(step.link) ? h("a", { href: `#${step.link}`, class: "qs-card" }, ...inner) : h("div", { class: "qs-card" }, ...inner),
    );
  });

  const quick = h(
    "section",
    { class: "quickstart", "aria-labelledby": "quickstart-title" },
    h("div", { class: "quickstart-head" }, h("h2", { id: "quickstart-title" }, "Quick start"), h("p", {}, "From an empty PBI to a finished run, in six steps.")),
    h("ol", { class: "qs-steps" }, ...steps),
  );

  return [hero, quick];
}
