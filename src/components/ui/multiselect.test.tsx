import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test } from "vitest";
import MultiSelect from "./multiselect";

const OPTIONS = ["A", "B", "C", "D"];

/** A thin stateful host: MultiSelect is controlled, and the checkedFirst
 * order depends on `selected` changing between opens, which only a real
 * onChange round-trip exercises. */
function Harness({
  initialSelected,
  checkedFirst = true,
}: {
  initialSelected: string[];
  checkedFirst?: boolean;
}) {
  const [selected, setSelected] = useState(initialSelected);
  return (
    <MultiSelect
      ariaLabel="Letters"
      options={OPTIONS}
      selected={selected}
      onChange={setSelected}
      checkedFirst={checkedFirst}
    />
  );
}

function optionLabels() {
  return [...document.querySelectorAll(".t-dropdown li")].map((li) => li.textContent);
}

test("checkedFirst snapshots the checked-first order when the panel opens, not live", () => {
  render(<Harness initialSelected={["C"]} />);
  fireEvent.click(screen.getByLabelText("Letters"));
  expect(optionLabels()).toEqual(["C", "A", "B", "D"]);

  // Toggling a row while open must not reorder it out from under the
  // pointer - the snapshot only moves on the next open.
  fireEvent.click(screen.getByText("A"));
  expect(optionLabels()).toEqual(["C", "A", "B", "D"]);
});

test("closing and reopening re-snapshots the order for the new selection", () => {
  render(<Harness initialSelected={["C"]} />);
  const trigger = screen.getByLabelText("Letters");
  fireEvent.click(trigger); // open: selected = [C] -> C,A,B,D
  fireEvent.click(screen.getByText("A")); // selected becomes [C, A], order unchanged while open
  fireEvent.click(trigger); // close
  fireEvent.click(trigger); // reopen: selected = [C, A] -> selected kept in options order
  expect(optionLabels()).toEqual(["A", "C", "B", "D"]);
});

test("without checkedFirst, the option order is left untouched", () => {
  render(<Harness initialSelected={["C"]} checkedFirst={false} />);
  fireEvent.click(screen.getByLabelText("Letters"));
  expect(optionLabels()).toEqual(["A", "B", "C", "D"]);
});
