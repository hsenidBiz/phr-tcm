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

/** Options that arrive after the panel opened (PrPanel's repo list is
 * still loading when a person opens the picker): only `options` changes
 * here, `selected` is driven directly so the point of the test - the
 * option-set change - is not entangled with a toggle round-trip. */
function LateOptionsHarness({ options, selected }: { options: string[]; selected: string[] }) {
  return <MultiSelect ariaLabel="Letters" options={options} selected={selected} onChange={() => {}} checkedFirst />;
}

test("options arriving while open join the checked-first groups without reflowing what's already shown", () => {
  const { rerender } = render(<LateOptionsHarness options={["A"]} selected={["C"]} />);
  fireEvent.click(screen.getByLabelText("Letters"));
  // C is not an option yet - the snapshot at open time is just what exists.
  expect(optionLabels()).toEqual(["A"]);

  // B, C, D arrive. A (already shown) keeps its spot; C (newly arrived and
  // selected) joins the checked group; B and D (newly arrived, unselected)
  // join the rest - all in `options` order within their group.
  rerender(<LateOptionsHarness options={["A", "B", "C", "D"]} selected={["C"]} />);
  expect(optionLabels()).toEqual(["C", "A", "B", "D"]);
});

test("a toggle after options arrive still does not reorder", () => {
  function ToggleAfterArrivalHarness() {
    const [options, setOptions] = useState(["A"]);
    const [selected, setSelected] = useState(["C"]);
    return (
      <>
        <button onClick={() => setOptions(["A", "B", "C", "D"])}>Load more options</button>
        <MultiSelect
          ariaLabel="Letters"
          options={options}
          selected={selected}
          onChange={setSelected}
          checkedFirst
        />
      </>
    );
  }
  render(<ToggleAfterArrivalHarness />);
  fireEvent.click(screen.getByLabelText("Letters"));
  fireEvent.click(screen.getByRole("button", { name: "Load more options" }));
  expect(optionLabels()).toEqual(["C", "A", "B", "D"]);

  fireEvent.click(screen.getByText("A"));
  expect(optionLabels()).toEqual(["C", "A", "B", "D"]);
});
