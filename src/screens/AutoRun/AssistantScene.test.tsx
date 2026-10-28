// The shared office on the Assistants card: a desk and a pixel character per
// assistant, whose pose follows what the assistant is doing. jsdom does no
// animation, so the motion itself is checked the way other CSS contracts here
// are: as text in src/index.css.

import { render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import type { AgentActivity, AgentView } from "../../bindings";
import AssistantOffice, { bubbleFor, deskLeft, deskNumber, hueFor, poseFor, type ScenePose } from "./AssistantScene";

const css = readFileSync(resolve(__dirname, "../../index.css"), "utf8");

function agent(label: string, slot: number | null, activity: AgentActivity): AgentView {
  return {
    label,
    client: "claude-code",
    slot,
    own_browser: false,
    browser: null,
    account: null,
    activity,
    idle_minutes: 0,
    changed_for_testing: false,
    saves_blocked: false,
    can_stop: false,
  };
}

const idle: AgentActivity = { kind: "idle" };
const exploring: AgentActivity = { kind: "exploring" };
const replaying: AgentActivity = { kind: "replaying", case_id: 4, step: 2, of: 7 };

const office = (agents: AgentView[]) => render(<AssistantOffice agents={agents} />).container;
const one = (a: AgentActivity, slot: number | null = 0, label = "Assistant 1") => office([agent(label, slot, a)]);

test.each<[AgentActivity, ScenePose]>([
  [{ kind: "discovering", area: null }, "walk"],
  [{ kind: "mapping", modules: ["Leave"] }, "walk"],
  [exploring, "walk"],
  [replaying, "type"],
  [{ kind: "working" }, "type"],
  [idle, "doze"],
])("poseFor maps %j to %s", (activity, pose) => {
  expect(poseFor(activity)).toBe(pose);
});

test.each<[AgentActivity, string | null]>([
  [{ kind: "replaying", case_id: 4521, step: 3, of: 7 }, "Step 3 of 7"],
  [{ kind: "replaying", case_id: 4521, step: 0, of: 0 }, "Starting"],
  [{ kind: "discovering", area: "Profile" }, "Profile"],
  [{ kind: "discovering", area: null }, null],
  [{ kind: "mapping", modules: ["Leave"] }, "Menus"],
  [{ kind: "exploring" }, null],
  [{ kind: "working" }, null],
  [{ kind: "idle" }, null],
])("bubbleFor %o is %s", (a, text) => expect(bubbleFor(a)).toBe(text));

test.each([
  ["Assistant 2", "2"],
  ["Assistant 12", "12"],
  ["Assistant", null],
  ["", null],
])("deskNumber(%s) is %s", (l, n) => expect(deskNumber(l)).toBe(n));

test.each([
  [0, 1, "50%"],
  [0, 2, "25%"],
  [1, 2, "75%"],
])("deskLeft(%i, %i) is %s", (i, n, left) => expect(deskLeft(i, n)).toBe(left));

test("the office is decorative, with one desk per assistant at its share of the width and its number on it", () => {
  const c = office([agent("Assistant 1", 0, replaying), agent("Assistant 2", 1, idle)]);
  const root = c.firstElementChild as HTMLElement;
  expect(root).toHaveAttribute("aria-hidden", "true");
  expect(root).toHaveClass("assistant-scene");
  const desks = c.querySelectorAll<HTMLElement>(".assistant-scene__desk");
  expect(desks).toHaveLength(2);
  expect(desks[0].style.left).toBe("25%");
  expect(desks[1].style.left).toBe("75%");
  expect(desks[0].querySelector(".assistant-scene__number")).toHaveTextContent("1");
  expect(desks[1].querySelector(".assistant-scene__number")).toHaveTextContent("2");
});

test.each<[AgentActivity, ScenePose]>([
  [exploring, "walk"],
  [replaying, "type"],
  [idle, "doze"],
])("the walker for %j carries its pose and slot", (activity, pose) => {
  const walker = one(activity, 1).querySelector(".assistant-scene__walker")!;
  expect(walker).toHaveAttribute("data-pose", pose);
  expect(walker).toHaveAttribute("data-slot", "1");
  expect(walker.querySelector(".assistant-scene__bot")).not.toBeNull();
});

test("only typing shows the screen, walking fades the desk, and only dozing drifts a z", () => {
  const walk = one(exploring);
  const type = one(replaying);
  const doze = one(idle);
  expect(walk.querySelector(".assistant-scene__desk")).toHaveClass("assistant-scene__desk--empty");
  expect(type.querySelector(".assistant-scene__desk")).not.toHaveClass("assistant-scene__desk--empty");
  expect(doze.querySelector(".assistant-scene__desk")).not.toHaveClass("assistant-scene__desk--empty");
  expect(type.querySelector(".assistant-scene__screen")).not.toBeNull();
  expect(walk.querySelector(".assistant-scene__screen")).toBeNull();
  expect(doze.querySelector(".assistant-scene__screen")).toBeNull();
  expect(doze.querySelector(".assistant-scene__z")).not.toBeNull();
  expect(walk.querySelector(".assistant-scene__z")).toBeNull();
  expect(type.querySelector(".assistant-scene__z")).toBeNull();
});

test("a sitting assistant stands just left of its desk", () => {
  const walkers = office([agent("Assistant 1", 0, replaying), agent("Assistant 2", 1, idle)]).querySelectorAll<HTMLElement>(
    ".assistant-scene__walker",
  );
  expect(walkers[0].style.getPropertyValue("--seat")).toBe("clamp(0.5rem, calc(25% - 3.5rem), calc(100% - 2.25rem))");
  expect(walkers[1].style.getPropertyValue("--seat")).toBe("clamp(0.5rem, calc(75% - 3.5rem), calc(100% - 2.25rem))");
});

test("two walkers start at different points of the walk", () => {
  const walkers = office([agent("Assistant 1", 0, exploring), agent("Assistant 2", 1, exploring)]).querySelectorAll<HTMLElement>(
    ".assistant-scene__walker",
  );
  expect(walkers[0].style.animationDelay).not.toBe(walkers[1].style.animationDelay);
  expect(walkers[1].style.animationDelay).toBe("-2.7s");
});

test("a walker paces around its own desk, kept inside the room", () => {
  const walkers = office([agent("Assistant 1", 0, exploring), agent("Assistant 2", 1, exploring)]).querySelectorAll<HTMLElement>(
    ".assistant-scene__walker",
  );
  const room = "calc(100% - 2.25rem)";
  expect(walkers[0].style.getPropertyValue("--walk-from")).toBe(`clamp(0.5rem, calc(25% - 5rem), ${room})`);
  expect(walkers[0].style.getPropertyValue("--walk-to")).toBe(`clamp(0.5rem, calc(25% + 3rem), ${room})`);
  expect(walkers[1].style.getPropertyValue("--walk-from")).toBe(`clamp(0.5rem, calc(75% - 5rem), ${room})`);
  // Stilled walkers stay at the start of their walk, not piled at the edge.
  expect(css).toMatch(/\.assistant-scene__walker\s*\{[^}]*left:\s*var\(--seat,\s*var\(--walk-from/);
  // The svg keeps its walker's timing so the flip lines up with the walk.
  for (const w of walkers) {
    expect(w.querySelector<SVGElement>(".assistant-scene__bot")!.style.animationDelay).toBe(w.style.animationDelay);
  }
});

test("the bubble rides inside its walker and ellipsises a long line", () => {
  const c = one({ kind: "discovering", area: "Leave Encashment Approvals" }, 1, "Assistant 2");
  const bubble = c.querySelector(".assistant-scene__walker .assistant-scene__bubble")!;
  expect(bubble).toHaveTextContent("Leave Encashment Approvals");
  expect(bubble).toHaveClass("text-ellipsis", "max-w-[8rem]");
});

test("no bubble or number when there is nothing to say", () => {
  const c = one(idle, 0, "Assistant");
  expect(c.querySelector(".assistant-scene__bubble")).toBeNull();
  expect(c.querySelector(".assistant-scene__number")).toBeNull();
});

const hueOf = (slot: number | null) =>
  (one(exploring, slot).querySelector(".assistant-scene__walker") as HTMLElement).style.getPropertyValue("--assistant-hue");

test("a slot beyond the third still renders, in the third slot's look", () => {
  expect(one(exploring, 5).querySelector(".assistant-scene__walker")).toHaveAttribute("data-slot", "5");
  expect(hueOf(5)).toBe(hueOf(2));
  expect(hueOf(5)).toBe(hueFor(5));
});

test("no browser is recorded as data-slot none", () => {
  expect(one(exploring, null).querySelector(".assistant-scene__walker")).toHaveAttribute("data-slot", "none");
});

test("every slot colour is a theme token, never danger or warning, and the first three differ", () => {
  const hues = [0, 1, 2, null].map(hueOf);
  for (const hue of hues) {
    expect(hue).toMatch(/var\(--color-/);
    expect(hue).not.toMatch(/#[0-9a-f]{3,8}\b|rgb\(|hsl\(/i);
    expect(hue).not.toMatch(/danger|warning/);
  }
  expect(new Set(hues).size).toBe(4);
  expect(hues[3]).toBe("var(--color-muted)");
});

test("the css walks the wrapper across the room, flips only the svg, and stills it all under reduced motion", () => {
  const walkFrames = css.match(/@keyframes\s+assistant-walk\b[\s\S]*?\n\}/);
  expect(walkFrames![0]).toContain("var(--walk-from");
  expect(walkFrames![0]).toContain("var(--walk-to");
  expect(walkFrames![0]).not.toContain("scaleX");
  expect(css).toMatch(/@keyframes\s+assistant-flip\b[\s\S]*?scaleX\(-1\)/);
  expect(css).toMatch(/\.assistant-scene__walker\[data-pose="walk"\]\s*\{\s*animation:\s*assistant-walk/);
  expect(css).toMatch(/\[data-pose="walk"\] \.assistant-scene__bot\s*\{\s*animation:\s*assistant-flip/);
  expect(css).toMatch(/@keyframes\s+assistant-bob\b/);
  expect(css).toMatch(/@keyframes\s+assistant-z-drift\b/);
  expect(css).toMatch(/@keyframes\s+assistant-blink\b/);

  const blocks = [...css.matchAll(/@media \(prefers-reduced-motion: reduce\)\s*\{([\s\S]*?)\n\}/g)].map((m) => m[1]);
  const block = blocks.find((b) => b.includes(".assistant-scene"));
  expect(block, "a reduced-motion block names .assistant-scene").toBeDefined();
  expect(block).toMatch(/animation:\s*none/);
  for (const part of ["__walker", "__bot", "__eye", "__z"]) {
    expect(block, `reduced motion stops ${part}`).toContain(`.assistant-scene${part}`);
  }
});
