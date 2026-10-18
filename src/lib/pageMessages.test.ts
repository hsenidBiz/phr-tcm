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

test("a toast stack reads each toast as one message, title and body together", () => {
  const toast = (kind: string, title: string, body: string) =>
    `<div class="toast toast-${kind}"><div class="toast-title">${title}</div><div class="toast-message">${body}</div></div>`;
  document.body.innerHTML = `<div id="toast-container" class="toast-top-right">${toast("success", "Saved", "Old body")}</div>`;
  const before = read().messages.map((m) => m.text);
  expect(before).toEqual(["Saved Old body"]);
  document.body.innerHTML = `<div id="toast-container" class="toast-top-right">${toast("success", "Saved", "Old body")}${toast("error", "Refused", "Weightage must total 100.")}</div>`;
  const after = read().messages.map((m) => m.text);
  expect(after).toEqual(["Saved Old body", "Refused Weightage must total 100."]);
  expect(after[0]).toBe(before[0]);
});

test("a form group in error says its help text, not its label or field", () => {
  document.body.innerHTML = `<div class="form-group has-error"><label for="n">Name</label><input id="n" value="x"><span class="help-block">Name is required</span></div>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Name is required"]);
});

test("a validated form gives only the feedback it shows", () => {
  document.body.innerHTML = `<form class="needs-validation"><label>Title</label><input class="is-invalid">
    <div class="invalid-feedback">Please choose a title.</div>
    <div class="invalid-feedback" style="display:none">Hidden one</div>
    <button type="submit">Save</button></form>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Please choose a title."]);
});

test("a message at opacity 0, or under one, is not read yet", () => {
  document.body.innerHTML = `<div class="toast" style="opacity:0">Coming</div>
    <div style="opacity:0"><div role="alert">Under it</div></div>
    <div class="toast" style="opacity:1">Here</div>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Here"]);
});

test("a crowd of ordinary rows does not push a toast out of the read", () => {
  const rows = Array.from({ length: 60 }, (_, i) => `<div class="message-row">Row ${i}</div>`).join("");
  document.body.innerHTML = `${rows}<div class="toast">Refused</div>`;
  const got = read().messages.map((m) => m.text);
  expect(got[0]).toBe("Refused");
  expect(got).toHaveLength(61);
});

test("the element right after an invalid field is its message", () => {
  document.body.innerHTML = `<input aria-invalid="true"><span>Required</span>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Required"]);
});

test("a button inside a message is not part of it", () => {
  document.body.innerHTML = `<div class="alert">Save failed <button type="button">&times;</button></div>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Save failed"]);
});

test("the same message twice is read twice", () => {
  document.body.innerHTML = `<div class="toast">Save failed</div><div class="toast">Save failed</div>`;
  expect(read().messages.map((m) => m.text)).toEqual(["Save failed", "Save failed"]);
});
