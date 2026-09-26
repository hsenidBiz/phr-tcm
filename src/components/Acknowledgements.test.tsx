import { render, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import Acknowledgements from "./Acknowledgements";

test("credits the team, the developer, every tester and the special thanks", () => {
  render(<Acknowledgements />);
  const section = screen.getByRole("region", { name: "Acknowledgements" });
  expect(within(section).getByText("Built by the Innovation Team.")).toBeInTheDocument();
  expect(within(section).getByText("Avin Alwis")).toBeInTheDocument();
  expect(within(section).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "Ishani Dasanayake",
    "Naveen Warnakulasuriya",
    "Sachila Manamperi",
    "Vishwa Warnakulasuriya",
    "Hansani Gunasekara",
  ]);
  expect(within(section).getByText("Special thanks")).toBeInTheDocument();
  expect(within(section).getByText("Ayub Sourjah")).toBeInTheDocument();
});
