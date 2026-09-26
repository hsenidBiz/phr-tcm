// The sticky, grouped table of contents, with scroll-spy: the link for the
// section under the reading line is marked aria-current.

import { GROUPS, type SiteContent } from "../types";
import { h } from "./dom";

export function renderSidebar(content: SiteContent): HTMLElement {
  const nav = h("nav", { class: "sidebar", id: "sidebar", "aria-label": "Guide contents" });

  const groups = h("div", { class: "nav-groups" });
  groups.appendChild(
    h(
      "ul",
      { class: "nav-list" },
      h("li", {}, h("a", { href: "#top", class: "nav-link", "data-spy": "top" }, "Overview")),
      content.recipes.length
        ? h("li", {}, h("a", { href: "#common-tasks", class: "nav-link", "data-spy": "common-tasks" }, "Common tasks"))
        : null,
    ),
  );

  for (const group of GROUPS) {
    const screens = content.screens.filter((s) => s.group === group);
    if (!screens.length) continue;
    groups.appendChild(
      h(
        "div",
        { class: "nav-group" },
        h("p", { class: "nav-group-title" }, group),
        h(
          "ul",
          { class: "nav-list" },
          ...screens.map((s) =>
            h("li", {}, h("a", { href: `#${s.id}`, class: "nav-link", "data-spy": s.id }, h("span", {}, s.title))),
          ),
        ),
      ),
    );
  }
  nav.appendChild(groups);
  return nav;
}

/** Marks the sidebar link of the section crossing the upper third of the viewport. */
export function scrollSpy(nav: HTMLElement, sections: HTMLElement[]): () => void {
  if (typeof IntersectionObserver === "undefined" || !sections.length) return () => {};
  const visible = new Set<string>();
  const order = sections.map((s) => s.id);
  const mark = () => {
    const current = order.find((id) => visible.has(id));
    if (!current) return; // between sections: keep the last one marked
    for (const a of nav.querySelectorAll<HTMLAnchorElement>("a[data-spy]")) {
      if (a.dataset.spy === current) a.setAttribute("aria-current", "true");
      else a.removeAttribute("aria-current");
    }
  };
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) visible.add(e.target.id);
        else visible.delete(e.target.id);
      }
      mark();
    },
    { rootMargin: "-25% 0px -65% 0px" },
  );
  sections.forEach((s) => io.observe(s));
  return () => io.disconnect();
}
