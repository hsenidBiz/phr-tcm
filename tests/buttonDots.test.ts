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

test("catches dots inside a child element of the button", () => {
  expect(labels(`const a = <button><span className="x">{busy ? "Loading…" : "Load more"}</span></button>;`)).toEqual(["Loading…"]);
  expect(labels(`const a = <Button><b><i>Wait...</i></b></Button>;`)).toEqual(["Wait..."]);
});

test("catches && and ?? and || branches", () => {
  expect(labels(`const a = <Button>{busy && "Working…"}</Button>;`)).toEqual(["Working…"]);
  expect(labels(`const a = <Button>{name ?? "Pick…"}</Button>;`)).toEqual(["Pick…"]);
  expect(labels(`const a = <Button>{name || "Pick..."}</Button>;`)).toEqual(["Pick..."]);
});

test("follows a label held in a same-file const", () => {
  expect(labels(`const LABEL = "Open…"; const a = <Button>{LABEL}</Button>;`)).toEqual(["Open…"]);
  expect(labels(`const LABEL = busy ? "Open…" : "Open"; const a = <Button>{LABEL}</Button>;`)).toEqual(["Open…"]);
  expect(labels(`const LABEL = "Open…"; const a = <p>{LABEL}</p>;`)).toEqual([]);
});

test("catches role=button and role=menuitem elements", () => {
  expect(labels(`const a = <span role="button">Go…</span>;`)).toEqual(["Go…"]);
  expect(labels(`const a = <div role="menuitem">Go...</div>;`)).toEqual(["Go..."]);
  expect(labels(`const a = <span role="status">Loading…</span>;`)).toEqual([]);
});
