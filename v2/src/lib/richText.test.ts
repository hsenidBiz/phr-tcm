import { expect, test } from "vitest";
import { htmlToMd } from "./richText";

test("real rich HTML converts normally (escaping intact)", () => {
  const md = htmlToMd("<div><b>bold</b> and a 2*3 product<ul><li>item</li></ul></div>");
  expect(md).toContain("**bold**");
  expect(md).toMatch(/-\s+item/);
  // A lone literal asterisk in genuine rich text stays escaped.
  expect(md).toContain("2\\*3");
});

test("markdown source pasted into ADO keeps tokens and line breaks", () => {
  // One div, raw newlines, literal markdown - how the org's RCA fields are
  // authored. HTML would collapse this to a single run-on paragraph.
  const html =
    "<div>### What is the Issue?\nOn **Cycle Setup**, clicking **Save** threw `SqlException (547)`.\n- no validation\n- raw 500</div>";
  const md = htmlToMd(html);
  expect(md).toContain("### What is the Issue?");
  expect(md).not.toContain("\\*\\*");
  expect(md).toContain("**Cycle Setup**");
  expect(md).toContain("`SqlException (547)`");
  // Lines survive as separate lines (marked breaks:true renders them).
  expect(md.split("\n").filter(Boolean).length).toBeGreaterThanOrEqual(4);
  expect(md).toMatch(/\n- no validation/);
});

test("HTML tables convert to GFM pipe tables (not stacked cells)", () => {
  const html =
    "<table><thead><tr><th>Task</th><th>Hours</th></tr></thead>" +
    "<tbody><tr><td>Spec review</td><td>0.5</td></tr>" +
    "<tr><td>TC Seed</td><td>0.75</td></tr></tbody></table>";
  const md = htmlToMd(html);
  expect(md).toContain("| Task | Hours |");
  expect(md).toMatch(/\| ?-+ ?\| ?-+ ?\|/); // header separator row
  expect(md).toContain("| Spec review | 0.5 |");
  // Cells stay on their table row, not one-per-line.
  expect(md).not.toMatch(/^Task$/m);
});

test("empty and whitespace html is empty markdown", () => {
  expect(htmlToMd("")).toBe("");
  expect(htmlToMd("  <div> </div> ")).toBe("");
});
