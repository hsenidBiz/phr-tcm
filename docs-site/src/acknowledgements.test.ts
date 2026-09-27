// The guide closes with its credits: the team, the developer, every tester
// and the special thanks, reachable from the last link in the contents.

import { expect, test } from "vitest";
import { acknowledgements } from "./content/acknowledgements";
import { screens, recipes, intro } from "./content";
import { ACKNOWLEDGEMENTS_ID, renderAcknowledgements } from "./render/acknowledgements";
import { renderSidebar } from "./render/sidebar";

test("the credits name the team, the developer, every tester and the special thanks", () => {
  const section = renderAcknowledgements(acknowledgements);
  expect(section.id).toBe(ACKNOWLEDGEMENTS_ID);
  expect(section.querySelector("h2")?.textContent).toBe("Acknowledgements");
  expect(section.querySelector(".summary")?.textContent).toBe("Built by the Innovation Team.");
  const cards = [...section.querySelectorAll(".ack-card")];
  expect(cards.map((c) => c.querySelector("h3")?.textContent)).toEqual(["Developer", "Testers", "Special thanks"]);
  expect([...cards[0].querySelectorAll("li")].map((li) => li.textContent)).toEqual(["Avin Alwis"]);
  expect([...cards[1].querySelectorAll("li")].map((li) => li.textContent)).toEqual([
    "Ishani Dasanayake",
    "Naveen Warnakulasuriya",
    "Sachila Manamperi",
    "Vishwa Warnakulasuriya",
    "Hansani Gunasekara",
  ]);
  expect(cards[2].textContent).toBe(
    "Special thanksAyub Sourjah, for the feedback, and for the opportunity to bring Test Case Manager to the teams.",
  );
});

test("the last link in the contents goes to the credits", () => {
  const nav = renderSidebar({ screens, recipes, intro, available: [] } as never);
  const links = [...nav.querySelectorAll<HTMLAnchorElement>("a.nav-link")];
  const last = links[links.length - 1];
  expect(last.getAttribute("href")).toBe(`#${ACKNOWLEDGEMENTS_ID}`);
  expect(last.textContent).toBe("Acknowledgements");
});
