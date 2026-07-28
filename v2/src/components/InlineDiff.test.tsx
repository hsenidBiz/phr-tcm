import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import InlineDiff from "./InlineDiff";

/** The words the renderer actually marked, by kind. */
function marks(container: HTMLElement) {
  const out = { added: [] as string[], removed: [] as string[] };
  for (const el of container.querySelectorAll("span")) {
    const cls = el.className;
    const text = el.textContent?.trim() ?? "";
    if (!text) continue;
    if (cls.includes("text-success")) out.added.push(text);
    else if (cls.includes("line-through")) out.removed.push(text);
  }
  return out;
}

/**
 * The reported bug: editing one word of a title struck the ENTIRE old
 * title and printed the ENTIRE new one as an addition, leaving the reader
 * to spot the difference themselves. Steps were fixed; titles were not.
 */
test("only the words that changed are marked", () => {
  const { container } = render(
    <InlineDiff old="Login as an admin user" next="Log in as an admin user" />,
  );
  const { added, removed } = marks(container);
  expect(removed).toEqual(["Login"]);
  expect(added).toEqual(["Log in"]);
  // The words that did not change are still on screen, unmarked.
  expect(screen.getByText(/as an admin user/)).toBeInTheDocument();
});

test("a word appended to the end marks only that word", () => {
  const { container } = render(
    <InlineDiff old="Reject an expired password" next="Reject an expired password twice" />,
  );
  const { added, removed } = marks(container);
  expect(added).toEqual(["twice"]);
  expect(removed).toEqual([]);
});

test("a completely different value does mark the whole thing", () => {
  const { container } = render(<InlineDiff old="Alpha" next="Omega" />);
  const { added, removed } = marks(container);
  expect(removed).toEqual(["Alpha"]);
  expect(added).toEqual(["Omega"]);
});

/**
 * An empty side is not word-diffed: with nothing to compare against every
 * word is an "addition", which paints the value green and hides the one
 * fact worth showing - that there was nothing there before.
 */
test("an empty side is labelled rather than diffed", () => {
  render(<InlineDiff old="" next="A brand new title" />);
  expect(screen.getByText("(empty)")).toBeInTheDocument();
  expect(screen.getByText("A brand new title")).toBeInTheDocument();
});

test("a cleared value is labelled too, with the old value struck", () => {
  const { container } = render(<InlineDiff old="Was here" next="" />);
  expect(screen.getByText("(empty)")).toBeInTheDocument();
  expect(marks(container).removed).toEqual(["Was here"]);
});

test("the empty label can be named for the field", () => {
  render(<InlineDiff old="" next="Page loads" emptyLabel="(no expected result)" />);
  expect(screen.getByText("(no expected result)")).toBeInTheDocument();
});

test("an unchanged value marks nothing", () => {
  const { container } = render(<InlineDiff old="Same title" next="Same title" />);
  expect(marks(container)).toEqual({ added: [], removed: [] });
});
