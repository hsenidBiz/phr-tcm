import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test } from "vitest";
import { define, Term } from "../../src/components/Term";
import { TooltipLayer } from "../../src/components/ui/tooltip";
import { GLOSSARY } from "../../src/lib/glossary";

function show() {
  return render(
    <>
      <p>
        Sign in with the <Term name="recipe">recipe</Term> saved.
      </p>
      <TooltipLayer />
    </>,
  );
}

/** jsdom has no :focus-visible heuristic, so pin the keyboard case. */
function tab(el: HTMLElement) {
  el.matches = ((sel: string) => sel === ":focus-visible") as typeof el.matches;
  el.focus();
  fireEvent.focusIn(el);
}

test("hovering the word shows its definition", async () => {
  show();
  fireEvent.pointerOver(screen.getByText("recipe"), { bubbles: true });
  expect(await screen.findByRole("tooltip")).toHaveTextContent(GLOSSARY.recipe);
});

test("keyboard focus shows it too, and the word is a tab stop described by the definition from the start", async () => {
  show();
  const word = screen.getByText("recipe");
  expect(word).toHaveAttribute("tabindex", "0");
  // A screen reader gets the definition without anything being opened: the
  // description is pointed at from the start, in a hidden element.
  expect(word).toHaveAccessibleDescription(GLOSSARY.recipe);
  expect(screen.getByText(GLOSSARY.recipe)).toHaveAttribute("hidden");

  tab(word);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(GLOSSARY.recipe);
  fireEvent.blur(word);
  fireEvent.focusOut(word);
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
  expect(word).toHaveAccessibleDescription(GLOSSARY.recipe);
});

test("the hidden description is no part of the name of a heading the word is in", () => {
  render(
    <h2>
      Sign-in <Term name="recipe">recipe</Term>
    </h2>,
  );
  expect(screen.getByRole("heading", { name: "Sign-in recipe" })).toBeInTheDocument();
});

test("Escape closes the bubble", async () => {
  show();
  const word = screen.getByText("recipe");
  tab(word);
  await screen.findByRole("tooltip");
  act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  });
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});

test("a word inside a control is not a tab stop and leaves the control's name alone", () => {
  render(
    <button title={define("flow")}>
      <Term name="flow" within>
        Flows
      </Term>{" "}
      3
    </button>,
  );
  const button = screen.getByRole("button", { name: "Flows 3" });
  expect(screen.getByText("Flows")).not.toHaveAttribute("tabindex");
  expect(button).toHaveAccessibleDescription(GLOSSARY.flow);
});

test("it underlines the word with a dotted line from theme tokens, never a fixed colour", () => {
  show();
  const cls = screen.getByText("recipe").className;
  expect(cls).toContain("decoration-dotted");
  expect(cls).toContain("decoration-muted");
  expect(cls).toContain("motion-safe:");
});
