// The shared office on the Assistants card: three corners (Test lab, Map room,
// Lounge) and a pixel character per assistant, standing in the corner its
// activity belongs to. jsdom does no animation, so the motion itself is checked
// the way other CSS contracts here are: as text in src/index.css.

import { act, render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, expect, test, vi } from "vitest";
import type { AgentActivity, AgentView } from "../../bindings";
import AssistantOffice, {
  bubbleFor,
  cornerFor,
  deskNumber,
  hueFor,
  poseFor,
  spotLeft,
  type Corner,
  type ScenePose,
} from "./AssistantScene";

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

afterEach(() => vi.useRealTimers());

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

test.each<[AgentActivity, Corner]>([
  [replaying, "lab"],
  [{ kind: "working" }, "lab"],
  [{ kind: "discovering", area: null }, "map"],
  [{ kind: "mapping", modules: ["Leave"] }, "map"],
  [exploring, "map"],
  [idle, "lounge"],
])("cornerFor maps %j to %s", (activity, corner) => expect(cornerFor(activity)).toBe(corner));

test("spotLeft puts an occupant in its corner's third, 2.75rem from the edges (3.5rem in the Map room)", () => {
  expect(spotLeft("lab", 0, 1)).toContain("16.6");
  expect(spotLeft("lounge", 0, 1)).toContain("83.3");
  expect(new Set([0, 1, 2].map((k) => spotLeft("lab", k, 3))).size).toBe(3);
  for (const corner of ["lab", "map", "lounge"] as const) {
    for (const [k, m] of [
      [0, 1],
      [0, 3],
      [2, 3],
    ]) {
      const spot = spotLeft(corner, k, m);
      const edge = corner === "map" ? "3.5rem" : "2.75rem";
      const other = corner === "map" ? "2.75rem" : "3.5rem";
      expect(spot.match(new RegExp(edge.replace(".", "\\."), "g"))).toHaveLength(2);
      expect(spot).not.toContain(other);
    }
  }
});

test("the office is decorative, with three named corners in order, lit only when occupied", () => {
  const c = office([agent("Assistant 1", 0, replaying), agent("Assistant 2", 1, idle)]);
  const root = c.firstElementChild as HTMLElement;
  expect(root).toHaveAttribute("aria-hidden", "true");
  expect(root).toHaveClass("assistant-scene");
  const corners = c.querySelectorAll<HTMLElement>(".assistant-scene__corner");
  expect([...corners].map((e) => e.dataset.corner)).toEqual(["lab", "map", "lounge"]);
  expect([...corners].map((e) => e.textContent)).toEqual(["Test lab", "Map room", "Lounge"]);
  expect([...corners].map((e) => e.dataset.empty)).toEqual(["false", "true", "false"]);
});

test("each walker has its corner, pose, slot, spot, name tag and (only when there is one) bubble", () => {
  const c = office([agent("Assistant 1", 0, replaying), agent("Assistant 2", 1, idle)]);
  const walkers = c.querySelectorAll<HTMLElement>(".assistant-scene__walker");
  expect(walkers).toHaveLength(2);
  expect(walkers[0]).toHaveAttribute("data-corner", "lab");
  expect(walkers[0]).toHaveAttribute("data-pose", "type");
  expect(walkers[0]).toHaveAttribute("data-slot", "0");
  expect(walkers[0].style.getPropertyValue("--spot")).toBe(spotLeft("lab", 0, 1));
  expect(walkers[1].style.getPropertyValue("--spot")).toBe(spotLeft("lounge", 0, 1));
  const tag = walkers[0].querySelector(".assistant-scene__tag")!;
  expect(tag).toHaveTextContent("Assistant 1");
  expect(tag).toHaveClass("text-ellipsis");
  expect(tag).not.toHaveClass("truncate");
  expect(walkers[0].querySelector(".assistant-scene__bubble")).toHaveTextContent("Step 2 of 7");
  expect(walkers[1].querySelector(".assistant-scene__bubble")).toBeNull();
});

test("the bubble is in flow directly above the character, never absolutely positioned", () => {
  const c = one({ kind: "discovering", area: "Profile" }, 1, "Assistant 2");
  const bubble = c.querySelector<HTMLElement>(".assistant-scene__bubble")!;
  const stack = bubble.parentElement!;
  expect(stack).toHaveClass("flex", "flex-col");
  const column = stack.parentElement!;
  expect(column).toHaveClass("flex", "flex-col");
  expect(column.closest(".assistant-scene__walker")).not.toBeNull();
  expect(bubble.className).not.toMatch(/\babsolute\b|\bfixed\b|\bbottom-/);
  expect(bubble.nextElementSibling!.querySelector(".assistant-scene__bot")).not.toBeNull();
  // The tag follows the character in the column, outside what paces.
  expect(column.lastElementChild).toHaveClass("assistant-scene__tag");
  expect(stack.querySelector(".assistant-scene__tag")).toBeNull();
  // The character is sized in rem like everything around it, not fixed px.
  const bot = c.querySelector(".assistant-scene__bot")!;
  expect(bot.getAttribute("width")).toBeNull();
  expect(bot).toHaveClass("h-[2.125rem]", "w-7");
});

test("each corner has its props: a lit screen per lab occupant, a z per sleeper, one live pin", () => {
  const c = office([
    agent("Assistant 1", 0, replaying),
    agent("Assistant 2", 1, { kind: "working" }),
    agent("Assistant 3", 2, exploring),
    agent("Assistant 4", 3, idle),
  ]);
  expect(c.querySelectorAll(".assistant-scene__screen")).toHaveLength(2);
  expect(c.querySelectorAll(".assistant-scene__z")).toHaveLength(1);
  expect(c.querySelectorAll(".assistant-scene__pin--live")).toHaveLength(1);
});

test("three replaying assistants each get their own seat", () => {
  const walkers = office([1, 2, 3].map((n) => agent(`Assistant ${n}`, n - 1, replaying))).querySelectorAll<HTMLElement>(
    ".assistant-scene__walker",
  );
  expect(new Set([...walkers].map((w) => w.style.getPropertyValue("--spot"))).size).toBe(3);
});

test("two Map room pacers are out of step", () => {
  const paces = office([agent("Assistant 1", 0, exploring), agent("Assistant 2", 1, exploring)]).querySelectorAll<HTMLElement>(
    ".assistant-scene__pace",
  );
  expect(paces).toHaveLength(2);
  expect(paces[0].style.animationDelay).not.toBe(paces[1].style.animationDelay);
  for (const p of paces) {
    expect(p.querySelector<SVGElement>(".assistant-scene__bot")!.style.animationDelay).toBe(p.style.animationDelay);
  }
});

test("only the Map room paces", () => {
  expect(one(replaying).querySelector(".assistant-scene__pace")).toBeNull();
  expect(one(idle).querySelector(".assistant-scene__pace")).toBeNull();
});

test("an assistant that changes corner walks, facing the way it goes, until the walk ends", () => {
  vi.useFakeTimers();
  const discovering: AgentActivity = { kind: "discovering", area: "Profile" };
  const { container, rerender } = render(<AssistantOffice agents={[agent("Assistant 1", 0, discovering)]} />);
  const walker = () => container.querySelector<HTMLElement>(".assistant-scene__walker")!;
  expect(walker()).not.toHaveAttribute("data-moving");
  rerender(<AssistantOffice agents={[agent("Assistant 1", 0, replaying)]} />);
  expect(walker()).toHaveAttribute("data-moving", "true");
  expect(walker()).toHaveAttribute("data-facing", "left");
  act(() => {
    vi.advanceTimersByTime(2600);
  });
  expect(walker()).not.toHaveAttribute("data-moving");
  rerender(<AssistantOffice agents={[agent("Assistant 1", 0, idle)]} />);
  expect(walker()).toHaveAttribute("data-facing", "right");
});

test("a different assistant arriving in the same place does not inherit a walk", () => {
  vi.useFakeTimers();
  const { container, rerender } = render(<AssistantOffice agents={[agent("Assistant 1", 0, replaying)]} />);
  rerender(<AssistantOffice agents={[agent("Assistant 2", 0, replaying)]} />);
  expect(container.querySelector(".assistant-scene__walker")).not.toHaveAttribute("data-moving");
});

test("a long bubble wraps onto more lines instead of being cut off", () => {
  const c = one({ kind: "discovering", area: "Performance & Evaluation Approvals" }, 1, "Assistant 2");
  const bubble = c.querySelector(".assistant-scene__walker .assistant-scene__bubble")!;
  expect(bubble).toHaveTextContent("Performance & Evaluation Approvals");
  expect(bubble).toHaveClass("max-w-[8rem]", "whitespace-normal", "break-words", "text-center");
  expect(bubble.className).not.toMatch(/\btext-ellipsis\b|\bwhitespace-nowrap\b|\btruncate\b|\boverflow-/);
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

test("the css walks the wrapper across the floor, paces by translateX, and stills it all under reduced motion", () => {
  expect(css).toMatch(/\.assistant-scene__walker\s*\{[^}]*left:\s*var\(--spot\)[^}]*transition:\s*left 2\.5s/);
  const pace = css.match(/@keyframes\s+assistant-pace\b[\s\S]*?\n\}/)![0];
  expect(pace).toContain("translateX");
  expect(pace).toContain("2.5rem");
  expect(pace).not.toContain("left:");
  expect(css).toMatch(/@keyframes\s+assistant-flip\b[\s\S]*?scaleX\(-1\)/);
  expect(css).toMatch(/@keyframes\s+assistant-bob\b/);
  expect(css).toMatch(/@keyframes\s+assistant-z-drift\b/);
  expect(css).toMatch(/@keyframes\s+assistant-blink\b/);

  const blocks = [...css.matchAll(/@media \(prefers-reduced-motion: reduce\)\s*\{([\s\S]*?)\n\}/g)].map((m) => m[1]);
  const block = blocks.find((b) => b.includes(".assistant-scene"));
  expect(block, "a reduced-motion block names .assistant-scene").toBeDefined();
  expect(block).toMatch(/\.assistant-scene__walker\s*\{\s*transition:\s*none/);
  expect(block).toMatch(/animation:\s*none/);
  for (const part of ["__walker", "__pace", "__bot", "__eye", "__z"]) {
    expect(block, `reduced motion stops ${part}`).toContain(`.assistant-scene${part}`);
  }
});
