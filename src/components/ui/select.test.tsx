import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { Select } from "./select";

/** A disabled <option> is shown greyed out and cannot be picked, by mouse
 * or keyboard - Run Tests relies on this for an unreadable suggested order. */
test("a disabled option is listed but cannot be chosen", () => {
  const onChange = vi.fn();
  render(
    <Select aria-label="Order" value="spec" onChange={onChange}>
      <option value="suggested" disabled>
        Suggested run order
      </option>
      <option value="spec">Spec order</option>
    </Select>,
  );
  const trigger = screen.getByRole("combobox", { name: "Order" });

  fireEvent.click(trigger);
  const off = screen.getByRole("option", { name: "Suggested run order" });
  expect(off).toBeDisabled();
  fireEvent.click(off);
  expect(onChange).not.toHaveBeenCalled();

  // Keyboard: the list opens on the current value (Spec); ArrowUp lands on
  // the disabled one and Enter does nothing.
  fireEvent.keyDown(trigger, { key: "ArrowUp" });
  fireEvent.keyDown(trigger, { key: "Enter" });
  expect(onChange).not.toHaveBeenCalled();

  // An enabled option still commits.
  fireEvent.click(screen.getByRole("option", { name: "Spec order" }));
  expect(onChange).toHaveBeenCalledWith({ target: { value: "spec" } });
});
