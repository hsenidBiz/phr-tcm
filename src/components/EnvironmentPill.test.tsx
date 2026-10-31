import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import EnvironmentPill from "./EnvironmentPill";

// Its label is a block (label-trim): the pill must be a box of its own, or
// inside an inline parent (the screens' "Scripts for" label) its background
// drew as a sliver beside the text. jsdom does no layout, so the class is
// what can be checked here.
test("the pill lays itself out as an inline box around its label", () => {
  render(
    <span>
      <EnvironmentPill name="Templates for Staging" />
    </span>,
  );
  expect(screen.getByTitle("Active environment")).toHaveClass("inline-flex", "items-center");
  expect(screen.getByText("Templates for Staging")).toBeInTheDocument();
});
