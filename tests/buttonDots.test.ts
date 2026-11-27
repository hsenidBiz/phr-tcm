import { expect, test } from "vitest";
import { buttonDots } from "./buttonDots";

const labels = (src: string) => buttonDots(src).map((h) => h.label);

test("catches a button label ending in an ellipsis or three dots", () => {
  expect(labels(`const a = <Button>Foo…</Button>;`)).toEqual(["Foo…"]);
  expect(labels(`const a = <button type="button">Foo...</button>;`)).toEqual(["Foo..."]);
});

test("catches dots behind an icon, in a ternary and in a self-closing aria-label", () => {
  expect(labels(`const a = <Button><Icon />Import scripts…</Button>;`)).toEqual(["Import scripts…"]);
  expect(labels(`const a = <Button>{busy ? "Running…" : "Run"}</Button>;`)).toEqual(["Running…"]);
  expect(labels(`const a = <Button aria-label="Open file…" />;`)).toEqual(["Open file…"]);
});

test("passes status text, placeholders and clean buttons", () => {
  expect(labels(`const a = <p>Loading…</p>;`)).toEqual([]);
  expect(labels(`const a = <span className="x">{busy ? "Saving…" : "Saved"}</span>;`)).toEqual([]);
  expect(labels(`const a = <Select placeholder="Choose an environment…" />;`)).toEqual([]);
  expect(labels(`const a = <Button>Foo</Button>;`)).toEqual([]);
  expect(labels(`const a = <Button>{busy ? "Running" : "Run"}</Button>;`)).toEqual([]);
});
