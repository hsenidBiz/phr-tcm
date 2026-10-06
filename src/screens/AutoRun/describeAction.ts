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
// and a request keeps only its `handler` name, as the run log does - the
// same for an address inside a text the script checks. No sentence shows a
// password: a fill into a password field, or of the account's password
// placeholder, reads "the account's password".
//
// The describer is total. The JSON box can hold anything that parses, so an
// action or a locator that is not the shape it should be reads as
// `UNREADABLE` rather than throwing - the Script window must never blank
// out over a script it cannot read.

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

/** The line for an action that is not the shape an action should be. */
export const UNREADABLE = "Could not read this action - press Edit script to see it";

/** Thrown by the field readers below; `describeAction` turns it into
 * `UNREADABLE`. */
class Unreadable extends Error {}

const str = (v: unknown): string => {
  if (typeof v !== "string") throw new Unreadable();
  return v;
};
const optStr = (v: unknown): string | undefined => (v == null ? undefined : str(v));
const num = (v: unknown): number => {
  if (typeof v !== "number" || !Number.isFinite(v)) throw new Unreadable();
  return v;
};
const optNum = (v: unknown): number | undefined => (v == null ? undefined : num(v));
const strList = (v: unknown): string[] => {
  if (!Array.isArray(v)) throw new Unreadable();
  return v.map(str);
};
const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** A sentence as one string, the CSS parts as their own text. */
export function sentenceText(s: Sentence): string {
  return s.map((p) => p.text).join("");
}

/** Seconds from milliseconds: whole when it is, else one decimal place. */
function seconds(ms: number): string {
  const s = ms / 1000;
  return Number.isInteger(s) ? String(s) : s.toFixed(1);
}

const upTo = (ms: unknown): string => {
  const n = optNum(ms);
  return n === undefined ? "" : ` (up to ${seconds(n)} s)`;
};

/** The host at the start of an address: after a scheme's `//`, after a bare
 * `//`, a `www.` name, or a dotted name (or localhost) followed by `/`. */
const HOST = /^(?:[a-z][a-z0-9+.-]*:)?\/\/[^/?#]*|^www\.[^/?#]*|^(?:localhost|(?:[a-z0-9-]+\.)+[a-z]{2,})(?::\d+)?(?=\/)/i;

/** Whether a piece of text is an address: it starts with a scheme, `//` or
 * `www.`, or with a host followed by `/`. */
export function looksLikeAddress(text: string): boolean {
  const t = text.trim();
  return /^(?:[a-z][a-z0-9+.-]*:\/\/|\/\/|www\.)/i.test(t) || HOST.test(t);
}

/** An address split into its path (no host, no fragment) and its
 * `handler` query value, if it has one. */
function splitAddress(address: string): { path: string; handler: string | null } {
  let rest = address.trim();
  const host = HOST.exec(rest);
  if (host) rest = rest.slice(host[0].length);
  const [beforeHash] = rest.split("#");
  const [path, query = ""] = beforeHash.split("?");
  return { path, handler: new URLSearchParams(query).get("handler") };
}

/** An address or a part of one as a path only: no host, no fragment, and
 * no query string - except, with `keepHandler`, a `handler` name, which is
 * what tells two Razor Pages requests to the same page apart. */
export function pathOnly(address: string, keepHandler = false): string {
  const { path, handler } = splitAddress(address);
  const shown = path || "/";
  return keepHandler && handler ? `${shown}?handler=${handler}` : shown;
}

/** A text the script checks, with every address in it cut to its path. */
export function withoutHosts(text: string): string {
  return text
    .split(/(\s+)/)
    .map((piece) => (looksLikeAddress(piece) ? pathOnly(piece) : piece))
    .join("");
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

/** One locator step in words; `Unreadable` when it is not a locator. */
function describeStep(raw: unknown): Sentence {
  if (!isRecord(raw)) throw new Unreadable();
  const step = raw as LocatorStep;
  const nth = optNum(step.nth);
  const pick = nth === undefined ? "" : `${ordinal(nth)} `;
  const role = optStr(step.role);
  const name = optStr(step.name);
  const text = optStr(step.text);
  const css = optStr(step.css);
  if (role) {
    const noun = ROLE_NOUN[role] ?? role;
    return [words(name ? `the ${pick}"${name}" ${noun}` : `the ${pick}${noun}`)];
  }
  if (text) return [words(`the ${pick}text "${text}"`)];
  if (css) return describeCss(css, pick);
  throw new Unreadable();
}

/** A CSS selector: by the name it gives, else as the selector itself. */
function describeCss(css: string, pick = ""): Sentence {
  const named = cssName(css);
  if (named) return [{ kind: "words", text: `the ${pick}"${named.name}" ${named.noun}`, title: css }];
  const tag = /^[a-z]+/i.exec(lastCompound(css))?.[0].toLowerCase();
  const noun = tag === "iframe" || tag === "frame" ? "frame" : "element";
  return [words(`the ${pick}${noun} `), { kind: "css", text: css, title: css }];
}

/** What an action points at, in words; `Unreadable` when it is not a
 * selector. A chain reads from the element out: "the "Save" button inside
 * the "Add rating" dialog inside the "Editor" frame". */
function targetWords(target: unknown): Sentence {
  if (typeof target === "string") {
    if (!target.trim()) throw new Unreadable();
    return describeCss(target);
  }
  if (Array.isArray(target)) {
    if (target.length === 0) throw new Unreadable();
    const out: Sentence = [];
    [...target].reverse().forEach((step, i) => {
      if (i > 0) out.push(words(" inside "));
      out.push(...describeStep(step));
    });
    return out;
  }
  return describeStep(target);
}

/** What an action points at, in words. Total: a selector that is not one
 * reads "an element it could not read". */
export function describeTarget(target: Target | null | undefined): Sentence {
  try {
    return targetWords(target);
  } catch {
    return [words("an element it could not read")];
  }
}

/** Whether a fill goes into a password field (the target already read). */
function isPasswordField(target: unknown): boolean {
  const steps: unknown[] = Array.isArray(target) ? target : [target];
  const last = steps[steps.length - 1];
  const said = typeof last === "string" ? last : isRecord(last) ? [last.name, last.css, last.text].join(" ") : "";
  return /password/i.test(said);
}

const quoted = (list: readonly string[]) => list.map((s) => `"${s}"`).join(", ");

/** The details an `expect_download` reads in the file, as a clause each. */
function downloadDetails(a: Extract<Action, { kind: "expect_download" }>): string {
  const parts: string[] = [];
  const sheet = optStr(a.sheet);
  if (sheet) parts.push(`on the sheet "${sheet}"`);
  if (a.headers != null) {
    if (!isRecord(a.headers)) throw new Unreadable();
    if (a.headers.exact != null) parts.push(`with the headers ${quoted(strList(a.headers.exact))}`);
    else if (a.headers.contains != null) parts.push(`with headers that include ${quoted(strList(a.headers.contains))}`);
  }
  if (a.cells != null) {
    if (!Array.isArray(a.cells)) throw new Unreadable();
    for (const c of a.cells as unknown[]) {
      if (!isRecord(c)) throw new Unreadable();
      parts.push(`with cell ${str(c.ref)} ${c.match === "contains" ? "containing" : "reading"} "${str(c.text)}"`);
    }
  }
  if (a.contains_text != null) {
    const texts = strList(a.contains_text);
    if (texts.length) parts.push(`containing ${quoted(texts)}`);
  }
  return parts.length ? `, ${parts.join(", ")}` : "";
}

/** An `expect_tab`'s address text: `, at an address containing "<path>"`,
 * with any host or query cut away, or nothing when none is set. */
function tabAddress(contains: unknown): string {
  const text = typeof contains === "string" ? contains.trim() : "";
  if (!text) return "";
  // A query alone (`id=7&token=abc`) can carry a token, and names no
  // place a person would recognise: the sentence leaves it out.
  if (text.includes("=") && !splitAddress(text).path.includes("/")) return "";
  const shown = looksLikeAddress(text) || text.includes("/") || text.includes("?") ? pathOnly(text) : text;
  return `, at an address containing "${shown}"`;
}

/** `check_url`'s part of an address: a path, a handler, or neither (a
 * query only), never a host or the rest of a query string. */
function urlPart(contains: string): Sentence {
  const { path, handler } = splitAddress(contains);
  const shown = handler ? `${path}?handler=${handler}` : path;
  return shown ? [words(`Check the address contains "${shown}"`)] : [words("Check the address has the expected query")];
}

/** One action as a sentence. A `when_visible` is its heading only ("If ...
 * appears within 2 s:"); the actions under it are the caller's to list.
 * Total: anything that is not an action this app knows, in the shape it
 * should be, reads as `UNREADABLE`. */
export function describeAction(a: Action): Sentence {
  try {
    if (!isRecord(a) || typeof a.kind !== "string") throw new Unreadable();
    return describeKnown(a);
  } catch {
    return [words(UNREADABLE)];
  }
}

/** `describeAction` for a value already known to be an object with a kind.
 * Throws `Unreadable` for a field that is missing or the wrong type. */
function describeKnown(a: Action): Sentence {
  const describeTarget = targetWords;
  switch (a.kind) {
    case "navigate":
      return [words(`Go to ${pathOnly(str(a.url))}`)];
    case "click":
      return [words("Click "), ...describeTarget(a.selector)];
    case "fill": {
      const into = describeTarget(a.selector);
      str(a.value);
      if (a.value === PASSWORD_PLACEHOLDER || isPasswordField(a.selector))
        return [words("Type the account's password into "), ...into];
      if (a.value === USERNAME_PLACEHOLDER) return [words("Type the account's username into "), ...into];
      if (a.value === "") return [words("Clear "), ...into];
      return [words(`Type "${a.value}" into `), ...into];
    }
    case "wait_for":
      return [words("Wait for "), ...describeTarget(a.selector), words(` to appear${upTo(a.timeout_ms)}`)];
    case "check_text":
      return [words(`Check the page shows "${withoutHosts(str(a.value))}"`)];
    case "check_url":
      return urlPart(str(a.contains));
    case "expect_visible":
      return [words("Check "), ...describeTarget(a.selector), words(` is showing${upTo(a.timeout_ms)}`)];
    case "expect_hidden":
      return [words("Check "), ...describeTarget(a.selector), words(` is not showing${upTo(a.timeout_ms)}`)];
    case "expect_text":
      return [words("Check "), ...describeTarget(a.selector), words(` reads "${withoutHosts(str(a.equals))}"${upTo(a.timeout_ms)}`)];
    case "expect_contains_text":
      return [words("Check "), ...describeTarget(a.selector), words(` contains "${str(a.value)}"${upTo(a.timeout_ms)}`)];
    case "expect_count": {
      num(a.equals);
      const times = a.equals === 0 ? " is nowhere on the page" : a.equals === 1 ? " shows once" : ` shows ${a.equals} times`;
      return [words("Check "), ...describeTarget(a.selector), words(`${times}${upTo(a.timeout_ms)}`)];
    }
    case "expect_attribute":
      return [
        words("Check "),
        ...describeTarget(a.selector),
        words(` has ${str(a.name)} set to "${withoutHosts(str(a.equals))}"${upTo(a.timeout_ms)}`),
      ];
    case "sign_in":
      return [words(`Sign in as ${str(a.account)}`)];
    case "upload":
      return [words(`Upload the test file "${str(a.file)}" through `), ...describeTarget(a.selector)];
    case "expect_response": {
      const method = optStr(a.method);
      const json = a.json != null ? " with the expected data" : "";
      return [
        words(
          `Check the ${method ? `${method.toUpperCase()} ` : ""}request to ${pathOnly(str(a.url_contains), true)} answers ${optNum(a.status) ?? 200}${json}`,
        ),
      ];
    }
    case "api_request": {
      if (a.query != null && !isRecord(a.query)) throw new Unreadable();
      if (a.expect != null && !isRecord(a.expect)) throw new Unreadable();
      const handlerName = optStr(a.query?.handler);
      const handler = handlerName ? `?handler=${handlerName}` : "";
      const json = a.expect?.json != null ? " with the expected data" : "";
      return [
        words(`Ask the site for ${pathOnly(str(a.path))}${handler} and check it answers ${optNum(a.expect?.status) ?? 200}${json}`),
      ];
    }
    case "when_visible":
      return [words("If "), ...describeTarget(a.selector), words(` appears within ${seconds(optNum(a.within_ms) ?? 2000)} s:`)];
    case "reload":
      return [words("Reload the page")];
    case "expire_session":
      return [words("End the session")];
    case "return_to_area": {
      const area = optStr(a.area)?.trim();
      return [words(area ? `Go to the "${area}" area` : "Return to the case's area")];
    }
    case "press_key": {
      const times = optNum(a.times);
      return [words(`Press ${str(a.key).trim()}${times !== undefined && times > 1 ? ` ${times} times` : ""}`)];
    }
    case "drag": {
      const position = optStr(a.position) ?? "onto";
      if (!["before", "after", "onto"].includes(position)) throw new Unreadable();
      return [words("Drag "), ...describeTarget(a.from), words(` ${position} `), ...describeTarget(a.to)];
    }
    case "expect_focused":
      return [words("Check "), ...describeTarget(a.selector), words(` has the focus${upTo(a.timeout_ms)}`)];
    case "expect_download": {
      const named = str(a.name).includes("*") ? `named like "${a.name}"` : `named "${a.name}"`;
      return [words(`Check a file ${named} downloads${upTo(a.within_ms)}${downloadDetails(a)}`)];
    }
    case "expect_tab":
      return [words(`Wait for a new tab and call it "${str(a.name)}"${tabAddress(a.url_contains)}`)];
    case "open_tab":
      return [words(`Open a new tab "${str(a.name)}" at ${pathOnly(str(a.url))}`)];
    case "switch_tab":
      return [words(`Switch to the "${str(a.name)}" tab`)];
    case "close_tab":
      return [words(`Close the "${str(a.name)}" tab`)];
    case "expect_tab_closed":
      return [words(`Check the "${str(a.name)}" tab closes`)];
    default: {
      // A new action kind fails the type check here until it is described;
      // one the app does not know (a typo in the JSON) is unreadable.
      const unknown: never = a;
      throw new Unreadable(String((unknown as { kind?: unknown }).kind));
    }
  }
}
