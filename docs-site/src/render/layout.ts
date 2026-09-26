// Assembles the page - top bar, sidebar, hero, screen sections, common
// tasks - and wires the behaviour that spans them: deep links
// (#screen, #screen/control), the search palette, the theme, reveals.

import type { SiteContent } from "../types";
import { h, reducedMotion, rich } from "./dom";
import { icon } from "./icons";
import { PRODUCT, renderHero } from "./hero";
import { createPalette, searchButton } from "./search";
import { renderSection, type ControlRef } from "./section";
import type { ShotView } from "./shot";
import { renderSidebar, scrollSpy } from "./sidebar";
import { applyTheme, initialTheme, themeToggle } from "./theme";
import appIcon from "../../../src-tauri/icons/64x64.png";

/** Every class that starts an animation. Under reduced motion none is ever applied. */
export const ANIMATION_CLASSES = ["reveal", "pulse", "flash"] as const;

export type SiteHandle = { destroy(): void };

/** Below this width the sidebar becomes a drawer (matches styles.css). */
const NARROW = "(max-width: 960px)";

export function render(root: HTMLElement, content: SiteContent): SiteHandle {
  const motion = !reducedMotion();
  document.documentElement.dataset.motion = motion ? "full" : "reduce";
  const theme = initialTheme();
  document.documentElement.dataset.theme = theme;

  const cleanups: (() => void)[] = [];
  const timers = new Set<ReturnType<typeof setTimeout>>();
  const on = <E extends Event>(t: EventTarget, type: string, fn: (e: E) => void) => {
    t.addEventListener(type, fn as EventListener);
    cleanups.push(() => t.removeEventListener(type, fn as EventListener));
  };

  const refs = new Map<string, ControlRef>();
  const views: ShotView[] = [];

  // ---- sections -------------------------------------------------------
  const sections = content.screens.map((screen) =>
    renderSection(screen, content, { theme, motion, onRowClick: (ref) => togglePin(ref) }),
  );
  for (const s of sections) {
    views.push(...s.views);
    for (const c of s.controls) refs.set(`${c.screen}/${c.id}`, c);
  }

  const recipeIds = new Set(content.recipes.map((r) => `common-tasks/${r.id}`));
  const exists = (id: string) =>
    content.screens.some((s) => s.id === id) || refs.has(id) || (id === "common-tasks" && content.recipes.length > 0) || recipeIds.has(id);

  // ---- chrome ---------------------------------------------------------
  const palette = createPalette(content, (href) => go(href, { push: true }));
  const toggle = themeToggle(root);
  cleanups.push(toggle.dispose);

  const menuBtn = h("button", { type: "button", class: "icon-btn menu-btn", "aria-label": "Open contents", "aria-controls": "sidebar", "aria-expanded": "false" }, icon("menu"));
  const topbar = h(
    "header",
    { class: "topbar" },
    menuBtn,
    h(
      "a",
      { class: "brand", href: "#top" },
      h("img", { class: "brand-icon", src: appIcon, alt: "", width: 28, height: 28 }),
      h("span", { class: "brand-text" }, h("span", { class: "brand-name" }, PRODUCT), h("span", { class: "brand-sub" }, "How To Use")),
    ),
    h("div", { class: "topbar-spacer" }),
    searchButton("search-trigger", "Search", palette.open),
    toggle.button,
  );

  const sidebar = renderSidebar(content);
  const heroParts = renderHero(content, {
    theme,
    appIcon,
    exists,
    searchButton: searchButton("btn btn-ghost", "Search the guide", palette.open),
  });

  const main = h("main", { id: "main", class: "content", tabindex: "-1" }, ...heroParts);
  for (const s of sections) main.appendChild(s.section);
  if (content.recipes.length) main.appendChild(renderRecipes(content, exists));
  main.appendChild(
    h(
      "footer",
      { class: "footer" },
      h("p", {}, `${PRODUCT} · How To Use`),
      h("p", { class: "footer-hint" }, rich("Press [[Ctrl+K]] anywhere to search.")),
    ),
  );

  const scrim = h("div", { class: "nav-scrim", "aria-hidden": "true" });
  const site = h(
    "div",
    { class: "site" },
    h("a", { class: "skip-link", href: "#main" }, "Skip to content"),
    topbar,
    h("div", { class: "shell" }, sidebar, scrim, main),
    palette.el,
  );
  root.replaceChildren(site);
  applyTheme(theme, root);

  // ---- contents drawer (narrow windows) -------------------------------
  // Below the breakpoint the sidebar is an off-screen drawer. Closed, it is
  // inert, so its links leave the Tab order and the accessibility tree;
  // opening moves focus into it and closing hands focus back to the toggle.
  let narrowQuery: MediaQueryList | null = null;
  try {
    narrowQuery = typeof window.matchMedia === "function" ? window.matchMedia(NARROW) : null;
  } catch {
    narrowQuery = null;
  }
  const syncInert = () =>
    sidebar.toggleAttribute("inert", !!narrowQuery?.matches && !site.hasAttribute("data-nav-open"));
  const setNav = (open: boolean) => {
    const was = site.hasAttribute("data-nav-open");
    const focusInside = sidebar.contains(document.activeElement);
    site.toggleAttribute("data-nav-open", open);
    menuBtn.setAttribute("aria-expanded", String(open));
    menuBtn.setAttribute("aria-label", open ? "Close contents" : "Open contents");
    menuBtn.replaceChildren(icon(open ? "close" : "menu"));
    syncInert();
    if (open && !was) sidebar.querySelector<HTMLElement>("a")?.focus();
    if (!open && was && focusInside) menuBtn.focus();
  };
  menuBtn.addEventListener("click", () => setNav(!site.hasAttribute("data-nav-open")));
  scrim.addEventListener("click", () => setNav(false));
  const onBreakpoint = () => {
    if (!narrowQuery?.matches) setNav(false);
    syncInert();
  };
  narrowQuery?.addEventListener?.("change", onBreakpoint);
  cleanups.push(() => narrowQuery?.removeEventListener?.("change", onBreakpoint));
  syncInert();

  // ---- navigation -----------------------------------------------------
  function flash(el: HTMLElement) {
    if (!motion) return;
    el.classList.remove("flash");
    void el.offsetWidth; // restart the animation
    el.classList.add("flash");
    const t = setTimeout(() => {
      el.classList.remove("flash");
      timers.delete(t);
    }, 1800);
    timers.add(t);
  }

  function pinOnly(ref: ControlRef | null) {
    for (const v of views) v.pin(ref && ref.view === v ? ref.id : null);
  }

  function togglePin(ref: ControlRef) {
    const anchor = `${ref.screen}/${ref.id}`;
    const already = ref.view.figure.dataset.active === ref.id && ref.row.hasAttribute("data-pinned");
    for (const r of refs.values()) r.row.removeAttribute("data-pinned");
    if (already) {
      pinOnly(null);
      return;
    }
    pinOnly(ref);
    ref.row.setAttribute("data-pinned", "");
    try {
      history.replaceState(null, "", `#${anchor}`);
    } catch {
      /* some file:// contexts refuse; the spotlight still works */
    }
  }

  function go(target: string, opts: { push?: boolean; initial?: boolean } = {}) {
    const raw = target.replace(/^#/, "");
    let id = raw;
    try {
      id = decodeURIComponent(raw);
    } catch {
      /* a stray "%" (e.g. #50%) is not an escape; use the id as written */
    }
    const el = id ? document.getElementById(id) : null;
    setNav(false);
    if (!el || !root.contains(el)) return;
    if (opts.push) {
      try {
        history.pushState(null, "", `#${id}`);
      } catch {
        /* see togglePin */
      }
    }
    const ref = refs.get(id);
    const behavior: ScrollBehavior = motion && !opts.initial ? "smooth" : "auto";
    el.scrollIntoView({ behavior, block: ref ? "center" : "start" });
    for (const r of refs.values()) r.row.removeAttribute("data-pinned");
    if (ref) {
      pinOnly(ref);
      ref.row.setAttribute("data-pinned", "");
      ref.row.focus({ preventScroll: true });
      flash(ref.row);
    } else {
      pinOnly(null);
      if (el.tabIndex >= 0 || el.hasAttribute("tabindex")) el.focus({ preventScroll: true });
      flash(el.querySelector<HTMLElement>("h2") ?? el);
    }
  }

  // In-page links scroll smoothly and keep the spotlight logic in one place.
  on<MouseEvent>(root, "click", (e) => {
    const a = (e.target as Element | null)?.closest?.('a[href^="#"]') as HTMLAnchorElement | null;
    if (!a || !root.contains(a)) return;
    const href = a.getAttribute("href") ?? "";
    if (href === "#main") return; // skip link: native focus behaviour
    e.preventDefault();
    if (href === "#top") {
      window.scrollTo({ top: 0, behavior: motion ? "smooth" : "auto" });
      try {
        history.pushState(null, "", "#top");
      } catch {
        /* see togglePin */
      }
      setNav(false);
      return;
    }
    go(href, { push: true });
  });

  on(window, "hashchange", () => go(location.hash));

  on<KeyboardEvent>(document, "keydown", (e) => {
    const k = e.key.toLowerCase();
    if ((e.ctrlKey || e.metaKey) && k === "k") {
      e.preventDefault();
      if (palette.isOpen()) palette.close();
      else palette.open();
      return;
    }
    const typing = e.target instanceof HTMLElement && (e.target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName));
    if (k === "/" && !typing && !palette.isOpen()) {
      e.preventDefault();
      palette.open();
    } else if (k === "escape" && !palette.isOpen()) {
      pinOnly(null);
      for (const r of refs.values()) r.row.removeAttribute("data-pinned");
      setNav(false);
    }
  });

  // A click away from the rows and markers lets go of a pinned spotlight.
  on<MouseEvent>(document, "click", (e) => {
    const t = e.target as Element | null;
    if (t?.closest?.(".row, .marker, .palette")) return;
    if (!views.some((v) => v.figure.dataset.active)) return;
    pinOnly(null);
    for (const r of refs.values()) r.row.removeAttribute("data-pinned");
  });

  // ---- scroll-spy and reveals ----------------------------------------
  const spyTargets = [heroParts[0], ...sections.map((s) => s.section)];
  const recipesSection = main.querySelector<HTMLElement>("#common-tasks");
  if (recipesSection) spyTargets.push(recipesSection);
  cleanups.push(scrollSpy(sidebar, spyTargets));

  if (motion && typeof IntersectionObserver !== "undefined") {
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (!e.isIntersecting) continue;
          e.target.classList.add("is-in");
          io.unobserve(e.target);
        }
      },
      { rootMargin: "0px 0px -8% 0px", threshold: 0.04 },
    );
    for (const el of main.querySelectorAll<HTMLElement>(".screen, .quickstart, .recipes")) {
      el.classList.add("reveal");
      io.observe(el);
    }
    cleanups.push(() => io.disconnect());
  }

  if (location.hash && location.hash !== "#top") go(location.hash, { initial: true });

  return {
    destroy() {
      cleanups.forEach((c) => c());
      timers.forEach((t) => clearTimeout(t));
      document.documentElement.classList.remove("palette-open");
      root.replaceChildren();
    },
  };
}

function renderRecipes(content: SiteContent, exists: (id: string) => boolean): HTMLElement {
  return h(
    "section",
    { id: "common-tasks", class: "recipes", "aria-labelledby": "common-tasks--title", tabindex: "-1" },
    h(
      "header",
      { class: "screen-head" },
      h("p", { class: "eyebrow" }, "Guides"),
      h("h2", { id: "common-tasks--title" }, "Common tasks"),
      h("p", { class: "summary" }, "Short recipes for the jobs you do most. Each step links to the screen that does it."),
    ),
    h(
      "div",
      { class: "recipe-grid" },
      ...content.recipes.map((r) =>
        h(
          "article",
          { class: "recipe", id: `common-tasks/${r.id}`, tabindex: "-1" },
          h("h3", {}, r.title),
          h(
            "ol",
            {},
            ...r.steps.map((s) =>
              h(
                "li",
                {},
                s.link && exists(s.link)
                  ? h("a", { href: `#${s.link}` }, h("span", {}, rich(s.text)), icon("arrow", 14))
                  : h("span", {}, rich(s.text)),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}
