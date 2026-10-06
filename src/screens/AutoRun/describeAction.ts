// One script action as a plain sentence, for the Script window's readable
// view (and the demo's run lines).
//
// A sentence is a list of parts: words, and the CSS of an element that has
// nothing better to be called by. The CSS part is kept apart so the view
// can set it in a quieter monospace style and put the full selector in a
// tooltip; `sentenceText` flattens a sentence for anything that wants a
// string.
//
// No sentence names a host or a query string: an address reads as its path,
// and a request keeps only its `handler` name, as the run log does. No
// sentence shows a password: a fill into a password field, or of the
// account's password placeholder, reads "the account's password".

import type { Action, LocatorStep, Target } from "../../bindings";

/** Words, or a selector shown as it stands. Either can carry the full
 * selector as a `title`, for an element named from its CSS. */
export type Part = { kind: "words"; text: string; title?: string } | { kind: "css"; text: string; title: string };
export type Sentence = Part[];

const words = (text: string): Part => ({ kind: "words", text });

/** The placeholders a sign-in recipe's fill fills in for the account
 * (`autorun::recipe`): never typed as they stand, and the password never
 * shown. */
const PASSWORD_PLACEHOLDER = "{{password}}";
const USERNAME_PLACEHOLDER = "{{username}}";

/** A sentence as one string, the CSS parts as their own text. */
export function sentenceText(s: Sentence): string {
  return s.map((p) => p.text).join("");
}

/** Seconds from milliseconds: whole when it is, else one decimal place. */
function seconds(ms: number): string {
  const s = ms / 1000;
  return Number.isInteger(s) ? String(s) : s.toFixed(1);
}

const upTo = (ms: number | null | undefined): string => (typeof ms === "number" ? ` (up to ${seconds(ms)} s)` : "");

/** An address or a part of one as a path only: no host, no fragment, and
 * no query string - except, with `keepHandler`, a `handler` name, which is
 * what tells two Razor Pages requests to the same page apart. */
export function pathOnly(address: string, keepHandler = false): string {
  let rest = address.trim();
  const host = /^[a-z][a-z0-9+.-]*:\/\/[^/?#]*/i.exec(rest);
  if (host) rest = rest.slice(host[0].length);
  const [beforeHash] = rest.split("#");
  const [path, query = ""] = beforeHash.split("?");
  const handler = keepHandler ? new URLSearchParams(query).get("handler") : null;
  const shown = path || "/";
  return handler ? `${shown}?handler=${handler}` : shown;
}

/** What each role is called in a sentence. A role not here reads as itself. */
const ROLE_NOUN: Record<string, string> = {
  button: "button",
  link: "link",
  textbox: "field",
  searchbox: "search field",
  spinbutton: "field",
  combobox: "dropdown",
  listbox: "list",
  option: "option",
  checkbox: "checkbox",
  radio: "option",
  switch: "switch",
  tab: "tab",
  tabpanel: "tab panel",
  dialog: "dialog",
  alertdialog: "dialog",
  heading: "heading",
  menu: "menu",
  menuitem: "menu item",
  row: "row",
  cell: "cell",
  gridcell: "cell",
  columnheader: "column header",
  table: "table",
  grid: "table",
  img: "image",
  region: "section",
  article: "card",
  group: "group",
  iframe: "frame",
  navigation: "menu",
};

/** What a CSS selector's last element is, when its tag says so. */
const TAG_NOUN: Record<string, string> = {
  button: "button",
  a: "link",
  input: "field",
  textarea: "field",
  select: "dropdown",
  iframe: "frame",
  frame: "frame",
  table: "table",
  img: "image",
  dialog: "dialog",
  h1: "heading",
  h2: "heading",
  h3: "heading",
  h4: "heading",
};

const ORDINAL = ["first", "second", "third", "fourth", "fifth"];
const ordinal = (nth: number): string => ORDINAL[nth] ?? `number ${nth + 1}`;

/** The last compound of a selector: what it finally points at. */
function lastCompound(css: string): string {
  const parts = css.trim().split(/\s*[>+~]\s*|\s+(?![^[]*\])/);
  return parts[parts.length - 1] ?? css;
}

/** The name a CSS selector gives its element, when it gives one in words:
 * an aria-label, a placeholder or a title, on the element it ends at. */
function cssName(css: string): { noun: string; name: string } | null {
  const last = lastCompound(css);
  const attr = /\[(aria-label|placeholder|title)\s*[*^$~|]?=\s*["']([^"']+)["']\s*i?\]/i.exec(last);
  if (!attr) return null;
  const tag = /^[a-z][a-z0-9]*/i.exec(last)?.[0].toLowerCase();
  const noun =
    (tag && TAG_NOUN[tag]) ??
    (/^placeholder$/i.test(attr[1]) ? "field" : /\[role=["']?([a-z]+)/i.exec(last)?.[1] ?? "element");
  return { noun: ROLE_NOUN[noun] ?? noun, name: attr[2] };
}

/** One locator step in words. */
function describeStep(step: LocatorStep): Sentence {
  const pick = typeof step.nth === "number" ? `${ordinal(step.nth)} ` : "";
  if (step.role) {
    const noun = ROLE_NOUN[step.role] ?? step.role;
    return [words(step.name ? `the ${pick}"${step.name}" ${noun}` : `the ${pick}${noun}`)];
  }
  if (step.text) return [words(`the ${pick}text "${step.text}"`)];
  if (step.css) return describeCss(step.css, pick);
  return [words("an element")];
}

/** A CSS selector: by the name it gives, else as the selector itself. */
function describeCss(css: string, pick = ""): Sentence {
  const named = cssName(css);
  if (named) return [{ kind: "words", text: `the ${pick}"${named.name}" ${named.noun}`, title: css }];
  const tag = /^[a-z]+/i.exec(lastCompound(css))?.[0].toLowerCase();
  const noun = tag === "iframe" || tag === "frame" ? "frame" : "element";
  return [words(`the ${pick}${noun} `), { kind: "css", text: css, title: css }];
}

/** What an action points at, in words. A chain reads from the element out:
 * "the "Save" button inside the "Add rating" dialog inside the "Editor"
 * frame". */
export function describeTarget(target: Target | null | undefined): Sentence {
  if (typeof target === "string") return target.trim() ? describeCss(target) : [words("an element")];
  if (Array.isArray(target)) {
    if (target.length === 0) return [words("an element")];
    const out: Sentence = [];
    [...target].reverse().forEach((step, i) => {
      if (i > 0) out.push(words(" inside "));
      out.push(...describeStep(step));
    });
    return out;
  }
  if (target && typeof target === "object") return describeStep(target);
  return [words("an element")];
}

/** Whether a fill goes into a password field. */
function isPasswordField(target: Target | null | undefined): boolean {
  const steps: (LocatorStep | string)[] = Array.isArray(target) ? target : target ? [target] : [];
  const last = steps[steps.length - 1];
  if (last === undefined) return false;
  const said = typeof last === "string" ? last : [last.name, last.css, last.text].filter(Boolean).join(" ");
  return /password/i.test(said);
}

const quoted = (list: readonly string[]) => list.map((s) => `"${s}"`).join(", ");

/** The details an `expect_download` reads in the file, as a clause each. */
function downloadDetails(a: Extract<Action, { kind: "expect_download" }>): string {
  const parts: string[] = [];
  if (a.sheet) parts.push(`on the sheet "${a.sheet}"`);
  if (a.headers) {
    if ("exact" in a.headers && a.headers.exact) parts.push(`with the headers ${quoted(a.headers.exact)}`);
    else if ("contains" in a.headers && a.headers.contains)
      parts.push(`with headers that include ${quoted(a.headers.contains)}`);
  }
  for (const c of a.cells ?? []) {
    parts.push(`with cell ${c.ref} ${c.match === "contains" ? "containing" : "reading"} "${c.text}"`);
  }
  if (a.contains_text?.length) parts.push(`containing ${quoted(a.contains_text)}`);
  return parts.length ? `, ${parts.join(", ")}` : "";
}

/** One action as a sentence. A `when_visible` is its heading only ("If ...
 * appears within 2 s:"); the actions under it are the caller's to list. */
export function describeAction(a: Action): Sentence {
  switch (a.kind) {
    case "navigate":
      return [words(`Go to ${pathOnly(a.url)}`)];
    case "click":
      return [words("Click "), ...describeTarget(a.selector)];
    case "fill": {
      const into = describeTarget(a.selector);
      if (a.value === PASSWORD_PLACEHOLDER || isPasswordField(a.selector))
        return [words("Type the account's password into "), ...into];
      if (a.value === USERNAME_PLACEHOLDER) return [words("Type the account's username into "), ...into];
      if (a.value === "") return [words("Clear "), ...into];
      return [words(`Type "${a.value}" into `), ...into];
    }
    case "wait_for":
      return [words("Wait for "), ...describeTarget(a.selector), words(` to appear${upTo(a.timeout_ms)}`)];
    case "check_text":
      return [words(`Check the page shows "${a.value}"`)];
    case "check_url":
      return [words(`Check the address contains "${/^[a-z]+:\/\//i.test(a.contains) ? pathOnly(a.contains) : a.contains}"`)];
    case "expect_visible":
      return [words("Check "), ...describeTarget(a.selector), words(` is showing${upTo(a.timeout_ms)}`)];
    case "expect_hidden":
      return [words("Check "), ...describeTarget(a.selector), words(` is not showing${upTo(a.timeout_ms)}`)];
    case "expect_text":
      return [words("Check "), ...describeTarget(a.selector), words(` reads "${a.equals}"${upTo(a.timeout_ms)}`)];
    case "expect_contains_text":
      return [words("Check "), ...describeTarget(a.selector), words(` contains "${a.value}"${upTo(a.timeout_ms)}`)];
    case "expect_count": {
      const times = a.equals === 0 ? " is nowhere on the page" : a.equals === 1 ? " shows once" : ` shows ${a.equals} times`;
      return [words("Check "), ...describeTarget(a.selector), words(`${times}${upTo(a.timeout_ms)}`)];
    }
    case "expect_attribute":
      return [
        words("Check "),
        ...describeTarget(a.selector),
        words(` has ${a.name} set to "${a.equals}"${upTo(a.timeout_ms)}`),
      ];
    case "sign_in":
      return [words(`Sign in as ${a.account}`)];
    case "upload":
      return [words(`Upload the test file "${a.file}" through `), ...describeTarget(a.selector)];
    case "expect_response": {
      const method = a.method ? `${a.method.toUpperCase()} ` : "";
      const json = a.json != null ? " with the expected data" : "";
      return [words(`Check the ${method}request to ${pathOnly(a.url_contains, true)} answers ${a.status ?? 200}${json}`)];
    }
    case "api_request": {
      const handler = a.query?.handler ? `?handler=${a.query.handler}` : "";
      const json = a.expect?.json != null ? " with the expected data" : "";
      return [
        words(`Ask the site for ${pathOnly(a.path)}${handler} and check it answers ${a.expect?.status ?? 200}${json}`),
      ];
    }
    case "when_visible":
      return [words("If "), ...describeTarget(a.selector), words(` appears within ${seconds(a.within_ms ?? 2000)} s:`)];
    case "reload":
      return [words("Reload the page")];
    case "expire_session":
      return [words("End the session")];
    case "return_to_area":
      return [words("Return to the case's area")];
    case "press_key":
      return [words(`Press ${a.key}`)];
    case "expect_focused":
      return [words("Check "), ...describeTarget(a.selector), words(` has the focus${upTo(a.timeout_ms)}`)];
    case "expect_download": {
      const named = a.name.includes("*") ? `named like "${a.name}"` : `named "${a.name}"`;
      return [words(`Check a file ${named} downloads${upTo(a.within_ms)}${downloadDetails(a)}`)];
    }
    default: {
      // A new action kind fails the type check here until it is described.
      const unknown: never = a;
      return [words(`Do "${String((unknown as { kind?: unknown }).kind)}"`)];
    }
  }
}
