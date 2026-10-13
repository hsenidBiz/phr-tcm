// The guide shows what the app shows. The app opens it with `?advanced=1`
// while Enable Advanced Features is on (src-tauri/src/help.rs); without
// that, or with anything else, the Auto Run, API Templates and Playwright
// sections are left out, with their entries in the contents, the search and
// the hero, and so are the AI Bridge cards the app shows only with that setting on.

import type { SiteContent } from "./types";

/** The screens shown only while Enable Advanced Features is on. */
export const ADVANCED_SCREENS = ["auto-run", "api-templates", "playwright"] as const;

/** Per screen, the groups (cards) shown only while it is on. */
export const ADVANCED_GROUPS: Record<string, readonly string[]> = {
  "ai-bridge": ["environment", "api-templates"],
};

/** The query parameter the app opens the guide with, and its one value. */
export const ADVANCED_PARAM = "advanced";

/** Whether the guide was opened with Enable Advanced Features on. Unknown
 *  (no parameter, or any other value) is off. */
export function advancedFrom(search: string): boolean {
  try {
    return new URLSearchParams(search).get(ADVANCED_PARAM) === "1";
  } catch {
    return false;
  }
}

/** `content` as it is shown: whole when `advanced`, otherwise without the
 *  advanced screens and cards, the controls in those cards, the shots only
 *  they used, and any recipe step or quick-start link into them. */
export function forFeatures(content: SiteContent, advanced: boolean): SiteContent {
  if (advanced) return content;
  const hiddenScreens = new Set<string>(ADVANCED_SCREENS);
  const screens = content.screens
    .filter((s) => !hiddenScreens.has(s.id))
    .map((s) => {
      const groups = new Set(ADVANCED_GROUPS[s.id] ?? []);
      if (groups.size === 0) return s;
      const controls = s.controls.filter((c) => !(c.group && groups.has(c.group)));
      const dropped = s.controls.filter((c) => c.group && groups.has(c.group));
      const stillShown = new Set(controls.map((c) => c.shot));
      const onlyHidden = new Set(dropped.map((c) => c.shot).filter((shot) => !stillShown.has(shot)));
      return {
        ...s,
        controls,
        groups: s.groups?.filter((g) => !groups.has(g.id)),
        shots: s.shots.filter((shot) => !onlyHidden.has(shot.id)),
      };
    });
  const hiddenLink = (link: string | undefined) => {
    if (!link) return false;
    const [screen, part] = link.split("/");
    return hiddenScreens.has(screen) || (part !== undefined && (ADVANCED_GROUPS[screen] ?? []).includes(part));
  };
  const recipes = content.recipes
    .map((r) => ({ ...r, steps: r.steps.map((st) => (hiddenLink(st.link) ? { text: st.text } : st)) }));
  const intro = { ...content.intro, quickStart: content.intro.quickStart.filter((q) => !hiddenLink(q.link)) };
  return { ...content, screens, recipes, intro };
}
