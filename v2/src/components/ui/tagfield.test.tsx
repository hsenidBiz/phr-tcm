import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test } from "vitest";
import TagField, { joinTags, splitTags } from "./tagfield";

test("splitTags / joinTags round-trip the model's semicolon string", () => {
  expect(splitTags("a; b ;;c")).toEqual(["a", "b", "c"]);
  expect(joinTags(["a", "b"])).toBe("a; b");
});

function Harness({ suggestions }: { suggestions: string[] }) {
  const [v, setV] = useState("smoke");
  return (
    <>
      <TagField value={v} onChange={setV} suggestions={suggestions} />
      <output data-testid="val">{v}</output>
    </>
  );
}

test("chips render, suggestions filter, selecting adds, x removes", () => {
  render(<Harness suggestions={["smoke", "regression", "sanity"]} />);
  // Existing tag renders as a removable chip.
  expect(screen.getByLabelText("Remove smoke")).toBeInTheDocument();

  // Type to filter; "regression" matches, already-selected "smoke" excluded.
  const input = screen.getByLabelText("Tags");
  fireEvent.change(input, { target: { value: "reg" } });
  fireEvent.click(screen.getByRole("button", { name: "regression" }));
  expect(screen.getByTestId("val").textContent).toBe("smoke; regression");

  // Remove the first chip.
  fireEvent.click(screen.getByLabelText("Remove smoke"));
  expect(screen.getByTestId("val").textContent).toBe("regression");
});

test("Enter adds a brand-new tag not in the suggestions", () => {
  render(<Harness suggestions={["smoke"]} />);
  const input = screen.getByLabelText("Tags");
  fireEvent.change(input, { target: { value: "custom-tag" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(screen.getByTestId("val").textContent).toBe("smoke; custom-tag");
});
