/**
 * The message read (src-tauri/src/browser/page_messages.js): one JavaScript
 * expression the app evaluates in the page under test. Loaded here as
 * source text and run in jsdom against a small document.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, test } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(resolve(here, "../../src-tauri/src/browser/page_messages.js"), "utf8");

type Read = { messages: { text: string; where: string }[]; step: string | null };
const read = (): Read => new Function("return " + src)() as Read;

afterEach(() => {
  document.body.innerHTML = "";
});

test("a toast and a field message are read, a hidden one is not", () => {
  document.body.innerHTML = `
    <div class="toast toast-error">Weightage must total 100.</div>
    <input aria-invalid="true" aria-describedby="m"><span id="m">Name is required</span>
    <div class="alert" style="display:none">old</div>`;
  const got = read();
  expect(got.messages.map((m) => m.text)).toEqual(["Weightage must total 100.", "Name is required"]);
  expect(got.messages.map((m) => m.where)).toEqual(["page", "page"]);
  expect(got.step).toBeNull();
});

test("a message inside a same-origin iframe names its frame", () => {
  document.body.innerHTML = `<iframe title="Eval Rules"></iframe>`;
  const frame = document.querySelector("iframe") as HTMLIFrameElement;
  frame.contentDocument!.body.innerHTML = `<div role="alert">At least one assessment component must be enabled.</div>`;
  const got = read();
  expect(got.messages).toEqual([
    { text: "At least one assessment component must be enabled.", where: "frame Eval Rules" },
  ]);
});

test("the wizard's active step is named", () => {
  document.body.innerHTML = `<ul class="wizard-steps"><li>Details</li><li class="active">Assessment Components</li></ul>`;
  expect(read().step).toBe("Assessment Components");
});

test("a nested match is reported once, as the outermost", () => {
  document.body.innerHTML = `<div class="notification"><span class="error-text">Saved twice.</span></div>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Saved twice."]);
});

test("a message under a hidden ancestor is not read", () => {
  document.body.innerHTML = `<div hidden><div role="alert">Behind a hidden parent</div></div>
    <div style="visibility:hidden"><div role="status">Invisible</div></div>`;
  expect(read().messages).toEqual([]);
});

test("aria-current names the step before any class does", () => {
  document.body.innerHTML = `<nav><a>One</a><a aria-current="step">Two</a></nav>`;
  expect(read().step).toBe("Two");
});

test("an iframe the page cannot reach is passed over", () => {
  document.body.innerHTML = `<iframe title="Elsewhere"></iframe><div role="alert">Here</div>`;
  const frame = document.querySelector("iframe") as HTMLIFrameElement;
  Object.defineProperty(frame, "contentDocument", {
    get() {
      throw new Error("blocked");
    },
  });
  expect(read().messages.map((m) => m.text)).toEqual(["Here"]);
});
