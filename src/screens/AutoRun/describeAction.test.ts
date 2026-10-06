// One script action as a plain sentence: every kind, the element names,
// password masking, and addresses as paths only.

import { describe, expect, test } from "vitest";
import type { Action } from "../../bindings";
import { describeAction, describeTarget, pathOnly, sentenceText } from "./describeAction";

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
    [{ kind: "press_key", key: "Tab" }, "Press Tab"],
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
        "press_key", "expect_focused", "expect_download",
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
