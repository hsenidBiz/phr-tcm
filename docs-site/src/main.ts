// Entry point. The built page carries this as one classic inline script in
// <head>, so it runs before <body> is parsed: the theme is applied at once
// (no flash of the wrong theme) and the page is rendered once the DOM is in.

import "./styles.css";
import available from "virtual:help-shots";
import positions from "../shots/positions.json";
import { intro, recipes, screens } from "./content";
import { render } from "./render/layout";
import { initialTheme } from "./render/theme";
import appIcon from "../../src-tauri/icons/64x64.png";
import type { Positions, SiteContent } from "./types";

document.documentElement.dataset.theme = initialTheme();

const favicon = document.createElement("link");
favicon.rel = "icon";
favicon.href = appIcon;
document.head.appendChild(favicon);

async function boot() {
  let content: SiteContent = { screens, recipes, intro, positions: positions as Positions, available };

  // Design preview while there is no content yet: `vite dev` + ?sample.
  // import.meta.env.DEV is false in a build, so this branch and the sample
  // module are dropped from the shipped page.
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("sample")) {
    content = (await import("./dev/sample")).sample;
  }

  const root = document.getElementById("app");
  if (root) render(root, content);
}

if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => void boot(), { once: true });
else void boot();
