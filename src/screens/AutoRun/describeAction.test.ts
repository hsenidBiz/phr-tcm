// One script action as a plain sentence: every kind, the element names,
// password masking, and addresses as paths only.

import { describe, expect, test } from "vitest";
import type { Action } from "../../bindings";
import { describeAction, describeTarget, pathOnly, sentenceText, UNREADABLE } from "./describeAction";

const say = (a: Action) => sentenceText(describeAction(a));
const SAVE = { role: "button", name: "Save & Continue" };

describe("a sentence for every action kind", () => {
  const cases: [Action, string][] = [
    [{ kind: "navigate", url: "https://hr.example.test/hr/home/index?tab=2#top" }, "Go to /hr/home/index"],
    [{ kind: "click", selector: SAVE }, 'Click the "Save & Continue" button'],
    [
      { kind: "fill", selector: { role: "textbox", name: "Reason" }, value: "Family event" },
      'Type "Family event" into the "Reason" field',
    ],
    [{ kind: "fill", selector: { role: "textbox", name: "Reason" }, value: "" }, 'Clear the "Reason" field'],
    [
      { kind: "wait_for", selector: SAVE, timeout_ms: 10000 },
      'Wait for the "Save & Continue" button to appear (up to 10 s)',
    ],
    [{ kind: "check_text", value: "Step 2 of 6" }, 'Check the page shows "Step 2 of 6"'],
    [{ kind: "check_url", contains: "/PerformanceCycle" }, 'Check the address contains "/PerformanceCycle"'],
    [
      { kind: "expect_visible", selector: { role: "heading", name: "Leave request" } },
      'Check the "Leave request" heading is showing',
    ],
    [
      { kind: "expect_hidden", selector: { role: "dialog", name: "Confirm" }, timeout_ms: 1500 },
      'Check the "Confirm" dialog is not showing (up to 1.5 s)',
    ],
    [
      { kind: "expect_text", selector: { role: "cell", name: "Status" }, equals: "Active" },
      'Check the "Status" cell reads "Active"',
    ],
    [
      { kind: "expect_contains_text", selector: { role: "alert" }, value: "saved" },
      'Check the alert contains "saved"',
    ],
    [{ kind: "expect_count", selector: { role: "row" }, equals: 3 }, "Check the row shows 3 times"],
    [{ kind: "expect_count", selector: { role: "row" }, equals: 1 }, "Check the row shows once"],
    [{ kind: "expect_count", selector: { role: "row" }, equals: 0 }, "Check the row is nowhere on the page"],
    [
      { kind: "expect_attribute", selector: SAVE, name: "aria-disabled", equals: "true" },
      'Check the "Save & Continue" button has aria-disabled set to "true"',
    ],
    [{ kind: "sign_in", account: "hr.admin" }, "Sign in as hr.admin"],
    [
      { kind: "upload", selector: { role: "button", name: "Browse" }, file: "employees.xlsx" },
      'Upload the test file "employees.xlsx" through the "Browse" button',
    ],
    [
      { kind: "expect_response", url_contains: "/Manage?handler=Save&id=42", status: 200 },
      "Check the request to /Manage?handler=Save answers 200",
    ],
    [
      { kind: "expect_response", method: "post", url_contains: "/api/cycles", status: 201, json: { ok: true } },
      "Check the POST request to /api/cycles answers 201 with the expected data",
    ],
    [
      { kind: "api_request", path: "/api/cycles/42", query: { handler: "Load", id: "7" }, expect: { status: 200 } },
      "Ask the site for /api/cycles/42?handler=Load and check it answers 200",
    ],
    [
      { kind: "when_visible", selector: { role: "button", name: "Accept" }, then: [] },
      'If the "Accept" button appears within 2 s:',
    ],
    [{ kind: "reload" }, "Reload the page"],
    [{ kind: "expire_session" }, "End the session"],
    [{ kind: "return_to_area" }, "Return to the case's area"],
    [{ kind: "return_to_area", area: "Common Configurator" }, 'Go to the "Common Configurator" area'],
    [{ kind: "return_to_area", area: "  " }, "Return to the case's area"],
    [{ kind: "press_key", key: "Tab" }, "Press Tab"],
    [{ kind: "press_key", key: "Ctrl+ArrowUp" }, "Press Ctrl+ArrowUp"],
    [
      { kind: "expect_row", table: { role: "grid", name: "Employees" }, cells: { Status: "Active", Name: "Ann" } },
      'Check the "Employees" table has a row with Status "Active" and Name "Ann"',
    ],
    [
      { kind: "expect_no_row", table: { role: "grid", name: "Employees" }, cells: { Name: "Ben" }, exact: true },
      'Check the "Employees" table has no row with Name "Ben"',
    ],
    [
      { kind: "expect_row", table: { css: "#people" }, cells: { A: "1", B: "2", C: "3" }, timeout_ms: 5000 },
      'Check the element #people has a row with A "1", B "2" and C "3" (up to 5 s)',
    ],
    [
      { kind: "expect_sorted", table: { role: "grid", name: "Employees" }, column: "Joined", order: "descending", as: "date" },
      'Check the "Employees" table is sorted by Joined, descending',
    ],
    [{ kind: "expect_row_count", table: { role: "grid", name: "Employees" }, equals: 5 }, 'Check the "Employees" table has 5 rows'],
    [{ kind: "expect_row_count", table: { role: "grid", name: "Employees" }, at_least: 1 }, 'Check the "Employees" table has at least 1 row'],
    [{ kind: "expect_row_count", table: { role: "grid", name: "Employees" }, at_most: 3 }, 'Check the "Employees" table has at most 3 rows'],
    [{ kind: "expect_dialog", text: "Delete this cycle?", answer: "dismiss" }, 'Expect a dialog saying "Delete this cycle?" and press Cancel'],
    [{ kind: "expect_dialog", contains: "saved", answer: "accept" }, 'Expect a dialog containing "saved" and press OK'],
    [{ kind: "expect_dialog", answer: "accept" }, "Expect a dialog and press OK"],
    [
      { kind: "expect_dialog", text: "Your name?", answer: "accept", prompt_text: "Kim", within_ms: 5000 },
      'Expect a dialog saying "Your name?", type "Kim" and press OK (up to 5 s)',
    ],
    [{ kind: "press_key", key: "Ctrl+ArrowUp", times: 1 }, "Press Ctrl+ArrowUp"],
    [{ kind: "press_key", key: "Ctrl+ArrowUp", times: 3 }, "Press Ctrl+ArrowUp 3 times"],
    [
      { kind: "drag", from: { role: "row", name: "Grade C" }, to: { role: "row", name: "Grade A" }, position: "before" },
      'Drag the "Grade C" row before the "Grade A" row',
    ],
    [
      { kind: "drag", from: { role: "row", name: "Grade C" }, to: { role: "row", name: "Grade A" }, position: "after" },
      'Drag the "Grade C" row after the "Grade A" row',
    ],
    [
      { kind: "drag", from: { role: "row", name: "Grade C" }, to: { role: "row", name: "Grade A" } },
      'Drag the "Grade C" row onto the "Grade A" row',
    ],
    [
      { kind: "expect_focused", selector: { role: "textbox", name: "Name" } },
      'Check the "Name" field has the focus',
    ],
    [{ kind: "expect_download", name: "report.pdf" }, 'Check a file named "report.pdf" downloads'],
    [
      {
        kind: "expect_download",
        name: "Template*.xlsx",
        headers: { exact: ["Employee", "Grade"] },
        cells: [{ ref: "B2", text: "A1", match: "contains" }],
      },
      'Check a file named like "Template*.xlsx" downloads, with the headers "Employee", "Grade", with cell B2 containing "A1"',
    ],
    [
      { kind: "expect_download", name: "errors*.csv", within_ms: 30000, contains_text: ["Row 3"] },
      'Check a file named like "errors*.csv" downloads (up to 30 s), containing "Row 3"',
    ],
    [
      {
        kind: "expect_download",
        name: "Payslip*.pdf",
        pdf: { contains: "Ada", pages: { equals: 3 }, on_page: [{ page: -1, contains: "Total" }] },
      },
      'Check a file named like "Payslip*.pdf" downloads, with the text "Ada", 3 pages, "Total" on the last page',
    ],
    [
      {
        kind: "expect_download",
        name: "report.pdf",
        pdf: { contains: ["Net pay", "Grade"], pages: { at_least: 1 }, on_page: [{ page: 2, contains: ["A", "B"] }] },
      },
      'Check a file named "report.pdf" downloads, with the text "Net pay", "Grade", at least 1 page, "A", "B" on page 2',
    ],
    [
      { kind: "expect_download", name: "report.pdf", pdf: { pages: { at_most: 4 } } },
      'Check a file named "report.pdf" downloads, at most 4 pages',
    ],
    [{ kind: "expect_tab", name: "report" }, 'Wait for a new tab and call it "report"'],
    [
      { kind: "expect_tab", name: "report", url_contains: "https://hr.example.com/hr/report?id=7#top" },
      'Wait for a new tab and call it "report", at an address containing "/hr/report"',
    ],
    [
      { kind: "expect_tab", name: "report", url_contains: "/hr/report" },
      'Wait for a new tab and call it "report", at an address containing "/hr/report"',
    ],
    [
      { kind: "open_tab", name: "second", url: "https://hr.example.com/hr/employee/42?token=abc#top" },
      'Open a new tab "second" at /hr/employee/42',
    ],
    [{ kind: "open_tab", name: "second", url: "/hr/home" }, 'Open a new tab "second" at /hr/home'],
    [{ kind: "switch_tab", name: "report" }, 'Switch to the "report" tab'],
    [{ kind: "close_tab", name: "report" }, 'Close the "report" tab'],
    [{ kind: "expect_tab_closed", name: "preview" }, 'Check the "preview" tab closes'],
    [{ kind: "expect_tab", name: "report", url_contains: "id=7&token=abc" }, 'Wait for a new tab and call it "report"'],
    [{ kind: "expect_tab", name: "report", url_contains: "?id=7" }, 'Wait for a new tab and call it "report"'],
  ];

  test.each(cases)("%j", (action, sentence) => {
    expect(say(action)).toBe(sentence);
  });

  test("every kind in the list above is covered", () => {
    const kinds = new Set(cases.map(([a]) => a.kind));
    expect([...kinds].sort()).toEqual(
      [
        "navigate", "click", "fill", "wait_for", "check_text", "check_url", "expect_visible", "expect_hidden",
        "expect_text", "expect_contains_text", "expect_count", "expect_attribute", "sign_in", "upload",
        "expect_response", "api_request", "when_visible", "reload", "expire_session", "return_to_area",
        "press_key", "expect_focused", "expect_download", "expect_tab", "open_tab", "switch_tab", "close_tab",
        "expect_tab_closed", "drag", "expect_dialog", "expect_row", "expect_no_row", "expect_sorted", "expect_row_count",
      ].sort(),
    );
  });

  test("no sentence carries an em or en dash", () => {
    for (const [a] of cases) expect(say(a)).not.toMatch(/[–—]/);
  });
});

describe("passwords are never shown", () => {
  test("the account's password placeholder reads as the account's password", () => {
    expect(say({ kind: "fill", selector: { role: "textbox", name: "Secret" }, value: "{{password}}" })).toBe(
      'Type the account\'s password into the "Secret" field',
    );
  });

  test("a value typed into a password field is masked", () => {
    const typed = say({ kind: "fill", selector: { css: "#txtpassword" }, value: "Hunter2!" });
    expect(typed).toBe("Type the account's password into the element #txtpassword");
    expect(typed).not.toContain("Hunter2");
    const named = say({ kind: "fill", selector: { role: "textbox", name: "Password" }, value: "Hunter2!" });
    expect(named).not.toContain("Hunter2");
    const legacy = say({ kind: "fill", selector: 'input[type="password"]', value: "Hunter2!" });
    expect(legacy).not.toContain("Hunter2");
  });

  test("the username placeholder reads as the account's username", () => {
    expect(say({ kind: "fill", selector: { role: "textbox", name: "Username" }, value: "{{username}}" })).toBe(
      'Type the account\'s username into the "Username" field',
    );
  });
});

describe("how elements are named", () => {
  test("by role and name, text, or the label CSS gives", () => {
    expect(sentenceText(describeTarget({ role: "textbox", name: "Email" }))).toBe('the "Email" field');
    expect(sentenceText(describeTarget({ role: "article", name: "Goals / KPIs" }))).toBe('the "Goals / KPIs" card');
    expect(sentenceText(describeTarget({ text: "Activate" }))).toBe('the text "Activate"');
    expect(sentenceText(describeTarget({ role: "button", name: "Edit", nth: 1 }))).toBe('the second "Edit" button');
    expect(sentenceText(describeTarget('#form input[placeholder="Annual cycle"]'))).toBe('the "Annual cycle" field');
    expect(sentenceText(describeTarget('button[aria-label="Activate"]'))).toBe('the "Activate" button');
  });

  test("an element known only by CSS is the selector itself, with the full selector as its title", () => {
    const parts = describeTarget({ css: "#btnContinue-button" });
    expect(sentenceText(parts)).toBe("the element #btnContinue-button");
    expect(parts).toContainEqual({ kind: "css", text: "#btnContinue-button", title: "#btnContinue-button" });
    // A legacy string selector reads the same way.
    expect(describeTarget("#btnContinue-button")).toEqual(parts);
  });

  test("a name taken from CSS keeps the full selector as its title", () => {
    const [part] = describeTarget('input[placeholder="Email"]');
    expect(part).toEqual({ kind: "words", text: 'the "Email" field', title: 'input[placeholder="Email"]' });
  });

  test("a chain reads from the element out, through its frame", () => {
    expect(
      sentenceText(
        describeTarget([{ role: "iframe", name: "Editor" }, { role: "dialog", name: "Add rating" }, SAVE]),
      ),
    ).toBe('the "Save & Continue" button inside the "Add rating" dialog inside the "Editor" frame');
    expect(sentenceText(describeTarget([{ css: 'iframe[title="Goals"]' }, { role: "button", name: "Add" }]))).toBe(
      'the "Add" button inside the "Goals" frame',
    );
    expect(sentenceText(describeTarget([{ css: "iframe#goals" }, { text: "Add" }]))).toBe(
      'the text "Add" inside the frame iframe#goals',
    );
  });
});

describe("when_visible", () => {
  test("its heading names the wait, and its own actions stay its own", () => {
    const a: Action = {
      kind: "when_visible",
      selector: { role: "dialog", name: "Another active session" },
      within_ms: 5000,
      then: [{ kind: "click", selector: { role: "button", name: "Continue" } }],
    };
    expect(say(a)).toBe('If the "Another active session" dialog appears within 5 s:');
    expect(a.kind === "when_visible" && a.then.map(say)).toEqual(['Click the "Continue" button']);
  });
});

describe("addresses are paths only", () => {
  test("no host, no query string, no fragment", () => {
    expect(pathOnly("https://hr.example.test:8443/a/b?x=1#y")).toBe("/a/b");
    expect(pathOnly("/a/b?x=1")).toBe("/a/b");
    expect(pathOnly("https://hr.example.test")).toBe("/");
  });

  test("a request keeps its handler name and nothing else of the query", () => {
    expect(pathOnly("https://hr.example.test/Manage?id=4&handler=Save", true)).toBe("/Manage?handler=Save");
    expect(pathOnly("/Manage?id=4", true)).toBe("/Manage");
  });

  test("navigate and check_url never show a host", () => {
    expect(say({ kind: "navigate", url: "https://secret-host.example.test/x?token=abc" })).toBe("Go to /x");
    expect(say({ kind: "check_url", contains: "https://secret-host.example.test/x?y=1" })).toBe(
      'Check the address contains "/x"',
    );
  });
});

describe("an action that is not the right shape reads as one plain line, never a throw", () => {
  const odd = (a: unknown) => say(a as Action);

  test("the line has a plain hyphen", () => {
    expect(UNREADABLE).toBe("Could not read this action - press Edit script to see it");
  });

  test.each([
    ["a null action", null],
    ["a number", 3],
    ["an array", []],
    ["no kind", { selector: "#a" }],
    ["an unknown kind", { kind: "teleport" }],
    ["a missing field", { kind: "navigate" }],
    ["a field of the wrong type", { kind: "check_text", value: 4 }],
    ["a null selector", { kind: "click", selector: null }],
    ["a null step in a locator chain", { kind: "click", selector: [{ role: "dialog" }, null] }],
    ["an empty locator chain", { kind: "click", selector: [] }],
    ["a locator with nothing to find by", { kind: "click", selector: { nth: 1 } }],
    ["a role that is not text", { kind: "click", selector: { role: 7 } }],
    ["download headers that are not a list", { kind: "expect_download", name: "a.xlsx", headers: { exact: "A" } }],
    ["a pdf page count with two counts", { kind: "expect_download", name: "a.pdf", pdf: { pages: { equals: 1, at_most: 2 } } }],
    ["a pdf page that is not a number", { kind: "expect_download", name: "a.pdf", pdf: { on_page: [{ page: "last", contains: "x" }] } }],
    ["a when_visible wait that is not a number", { kind: "when_visible", selector: "#a", within_ms: "soon", then: [] }],
    ["a press_key times that is not a number", { kind: "press_key", key: "Tab", times: "twice" }],
    ["a drag position it does not know", { kind: "drag", from: "#a", to: "#b", position: "beside" }],
    ["a drag with no to", { kind: "drag", from: "#a" }],
    ["a dialog answer it does not know", { kind: "expect_dialog", answer: "maybe" }],
    ["a dialog with no answer", { kind: "expect_dialog", text: "x" }],
    ["a row with no cells", { kind: "expect_row", table: "#t", cells: {} }],
    ["a row count with two counts", { kind: "expect_row_count", table: "#t", equals: 1, at_most: 2 }],
    ["a sort order it does not know", { kind: "expect_sorted", table: "#t", column: "A", order: "up" }],
  ])("%s", (_, action) => {
    expect(odd(action)).toBe(UNREADABLE);
  });

  test("describeTarget on its own says it could not read the selector", () => {
    expect(sentenceText(describeTarget(null))).toBe("an element it could not read");
    expect(sentenceText(describeTarget([null] as never))).toBe("an element it could not read");
  });
});

describe("no host or query string in any sentence", () => {
  test("a protocol-relative navigate", () => {
    expect(say({ kind: "navigate", url: "//cdn.example.test/a/b?token=1" })).toBe("Go to /a/b");
  });

  test("check_text, with an address inside other words", () => {
    expect(say({ kind: "check_text", value: "Open https://hr.example.test/a?b=1 now" })).toBe(
      'Check the page shows "Open /a now"',
    );
    expect(say({ kind: "check_text", value: "Version 1.5/2 of 6" })).toBe('Check the page shows "Version 1.5/2 of 6"');
  });

  test("expect_text", () => {
    expect(say({ kind: "expect_text", selector: { role: "link" }, equals: "www.example.test/help?x=1" })).toBe(
      'Check the link reads "/help"',
    );
  });

  test("expect_attribute", () => {
    expect(
      say({ kind: "expect_attribute", selector: { role: "link", name: "Help" }, name: "href", equals: "https://h.example.test/x?y=1" }),
    ).toBe('Check the "Help" link has href set to "/x"');
    expect(
      say({ kind: "expect_attribute", selector: { role: "link", name: "Help" }, name: "href", equals: "hr.example.test/docs" }),
    ).toBe('Check the "Help" link has href set to "/docs"');
  });

  test("check_url: a host followed by a path, a query, and a handler", () => {
    expect(say({ kind: "check_url", contains: "hr.example.test/hr/home?x=1" })).toBe('Check the address contains "/hr/home"');
    expect(say({ kind: "check_url", contains: "PerformanceCycle?mode=Create" })).toBe(
      'Check the address contains "PerformanceCycle"',
    );
    expect(say({ kind: "check_url", contains: "/Manage?id=3&handler=Save" })).toBe(
      'Check the address contains "/Manage?handler=Save"',
    );
    expect(say({ kind: "check_url", contains: "?mode=Create" })).toBe("Check the address has the expected query");
  });
});
