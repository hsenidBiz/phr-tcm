// The small scene on each Assistants card row: a pixel character whose pose
// follows what the assistant is doing. jsdom does no animation, so the motion
// itself is checked the way other CSS contracts here are: as text in
// src/index.css.

import { render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, test } from "vitest";
import type { AgentActivity } from "../../bindings";
import AssistantScene, { poseFor, type ScenePose } from "./AssistantScene";

const css = readFileSync(resolve(__dirname, "../../index.css"), "utf8");

const idle: AgentActivity = { kind: "idle" };
const exploring: AgentActivity = { kind: "exploring" };
const replaying: AgentActivity = { kind: "replaying", case_id: 4, step: 2, of: 7 };

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

test.each<[AgentActivity, ScenePose]>([
  [exploring, "walk"],
  [replaying, "type"],
  [idle, "doze"],
])("the scene for %j is decorative and carries its pose", (activity, pose) => {
  const { container } = render(<AssistantScene activity={activity} slot={0} />);
  const root = container.firstElementChild as HTMLElement;
  expect(root).toHaveAttribute("aria-hidden", "true");
  expect(root).toHaveAttribute("data-pose", pose);
  expect(root).toHaveClass("assistant-scene", `assistant-scene--${pose}`);
  expect(root.querySelector(".assistant-scene__bot")).not.toBeNull();
});

test("only the typing pose draws the desk, and only the dozing pose drifts a z", () => {
  const walk = render(<AssistantScene activity={exploring} slot={0} />).container;
  const type = render(<AssistantScene activity={replaying} slot={0} />).container;
  const doze = render(<AssistantScene activity={idle} slot={0} />).container;

  expect(type.querySelector(".assistant-scene__desk")).not.toBeNull();
  expect(walk.querySelector(".assistant-scene__desk")).toBeNull();
  expect(doze.querySelector(".assistant-scene__desk")).toBeNull();

  expect(doze.querySelector(".assistant-scene__z")).not.toBeNull();
  expect(walk.querySelector(".assistant-scene__z")).toBeNull();
  expect(type.querySelector(".assistant-scene__z")).toBeNull();
});

test.each<[number | null, string]>([
  [0, "0"],
  [1, "1"],
  [2, "2"],
  [null, "none"],
])("slot %s is recorded as data-slot %s", (slot, expected) => {
  const { container } = render(<AssistantScene activity={exploring} slot={slot} />);
  expect(container.firstElementChild).toHaveAttribute("data-slot", expected);
});

const hueOf = (slot: number | null) =>
  (render(<AssistantScene activity={exploring} slot={slot} />).container.firstElementChild as HTMLElement).style.getPropertyValue(
    "--assistant-hue",
  );

test("a slot beyond the third still renders, in the third slot's look", () => {
  const { container } = render(<AssistantScene activity={exploring} slot={5} />);
  expect(container.firstElementChild).toHaveAttribute("data-slot", "5");
  expect(hueOf(5)).toBe(hueOf(2));
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

test("the css animates the walk and the typing pose, and stills both under reduced motion", () => {
  const walk = css.match(/\.assistant-scene--walk[^{]*\{[^}]*animation:\s*([\w-]+)/);
  const type = css.match(/\.assistant-scene--type[^{]*\{[^}]*animation:\s*([\w-]+)/);
  expect(walk, ".assistant-scene--walk sets an animation").not.toBeNull();
  expect(type, ".assistant-scene--type sets an animation").not.toBeNull();
  expect(css).toMatch(new RegExp(`@keyframes\\s+${walk![1]}\\b`));
  expect(css).toMatch(new RegExp(`@keyframes\\s+${type![1]}\\b`));
  expect(css).toMatch(/@keyframes\s+assistant-z-drift\b/);
  expect(css).toMatch(/@keyframes\s+assistant-blink\b/);

  const blocks = [...css.matchAll(/@media \(prefers-reduced-motion: reduce\)\s*\{([\s\S]*?)\n\}/g)].map((m) => m[1]);
  const block = blocks.find((b) => b.includes(".assistant-scene"));
  expect(block, "a reduced-motion block names .assistant-scene").toBeDefined();
  expect(block).toMatch(/animation:\s*none/);
  // Every animated part is named, not just the root.
  for (const part of ["__bot", "__eye", "__z"]) {
    expect(block, `reduced motion stops ${part}`).toContain(`.assistant-scene${part}`);
  }
});
