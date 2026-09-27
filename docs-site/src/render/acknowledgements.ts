// The Acknowledgements section at the end of the guide: the team, the
// developer, the testers, and a special thanks. Plain text, no controls.

import type { Acknowledgements } from "../content/acknowledgements";
import { h } from "./dom";

export const ACKNOWLEDGEMENTS_ID = "acknowledgements";

export function renderAcknowledgements(a: Acknowledgements): HTMLElement {
  const title = `${ACKNOWLEDGEMENTS_ID}--title`;
  return h(
    "section",
    { id: ACKNOWLEDGEMENTS_ID, class: "acks", "aria-labelledby": title, tabindex: "-1" },
    h(
      "header",
      { class: "screen-head" },
      h("p", { class: "eyebrow" }, "Credits"),
      h("h2", { id: title }, "Acknowledgements"),
      h("p", { class: "summary" }, a.team),
    ),
    h(
      "div",
      { class: "ack-grid" },
      h(
        "article",
        { class: "ack-card" },
        h("h3", {}, "Developer"),
        h("ul", { class: "ack-names" }, h("li", {}, a.developer)),
      ),
      h(
        "article",
        { class: "ack-card" },
        h("h3", {}, "Testers"),
        h("ul", { class: "ack-names" }, ...a.testers.map((name) => h("li", {}, name))),
      ),
      ...a.thanks.map((t) =>
        h(
          "article",
          { class: "ack-card is-thanks" },
          h("h3", {}, "Special thanks"),
          h("p", {}, h("strong", {}, t.name), `, ${t.for}`),
        ),
      ),
    ),
  );
}
