// The one line the Test cases tab opens with: where runs go, and whether
// the setup a run needs is in place. Every tick or warning says what it is
// about in words, so none rests on a colour or an icon alone.

import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ReadinessStrip from "./ReadinessStrip";

type Props = Parameters<typeof ReadinessStrip>[0];

const READY: Props = {
  envName: "QA",
  siteHost: "qa.example.com",
  signIn: "saved",
  accountCount: 2,
  areaCount: 3,
  testFileCount: 1,
  missingTestFiles: [],
  unreadable: [],
  onOpenSetup: () => {},
};

const strip = (over: Partial<Props> = {}) => {
  render(<ReadinessStrip {...READY} {...over} />);
  return screen.getByRole("group", { name: "Readiness" });
};

test("names the environment and the host runs go to", () => {
  const s = strip();
  expect(within(s).getByText("QA - qa.example.com").parentElement).toHaveTextContent(
    "Environment QA - qa.example.com",
  );
});

test("with no environment it says where runs go, and says plainly when there is no address", () => {
  const s = strip({ envName: null, siteHost: null });
  expect(s).toHaveTextContent("Runs against no site set yet");
  // A run cannot go without an address, so it is flagged like the other gaps.
  expect(within(s).getByText("no site set yet")).toHaveClass("text-warning");
});

test("an address that could not be read is not called missing", () => {
  const s = strip({ siteHost: undefined, unreadable: ["The environments could not be read"] });
  expect(within(s).queryByText("no site set yet")).not.toBeInTheDocument();
  expect(within(s).getByText("The environments could not be read")).toHaveClass("text-warning");
});

test("a read that failed is said in words, as a warning, beside what is known", () => {
  const s = strip({
    accountCount: null,
    unreadable: ["The accounts could not be read", "The test files could not be read"],
  });
  expect(within(s).getByText("The accounts could not be read")).toHaveClass("text-warning");
  expect(within(s).getByText("The test files could not be read")).toHaveClass("text-warning");
  expect(within(s).getByText("QA - qa.example.com")).toBeInTheDocument();
});

test("sign-in says Saved, or Built-in when the built-in recipe is in effect, or warns when there is none", () => {
  const { unmount } = render(<ReadinessStrip {...READY} signIn="builtin" />);
  expect(screen.getByText("Built-in")).toBeInTheDocument();
  unmount();
  const s = strip({ signIn: "none" });
  expect(within(s).getByText("No sign-in")).toHaveClass("text-warning");
});

test("counts accounts and areas, warning when there are no accounts", () => {
  const { unmount } = render(<ReadinessStrip {...READY} accountCount={1} areaCount={1} />);
  expect(screen.getByText("1 account")).toBeInTheDocument();
  expect(screen.getByText("1 area")).toBeInTheDocument();
  unmount();
  const s = strip({ accountCount: 0, areaCount: 0 });
  expect(within(s).getByText("No accounts")).toHaveClass("text-warning");
  // Areas are not needed to run, so none is said quietly.
  expect(within(s).getByText("No areas")).not.toHaveClass("text-warning");
});

test("a count still being read is left out rather than guessed", () => {
  const s = strip({ signIn: null, accountCount: null, areaCount: null, testFileCount: null });
  expect(within(s).queryByText(/account/)).not.toBeInTheDocument();
  expect(within(s).queryByText(/area/)).not.toBeInTheDocument();
  expect(within(s).queryByText(/test file/)).not.toBeInTheDocument();
  expect(within(s).queryByText(/sign-in/i)).not.toBeInTheDocument();
});

test("test files tick when all are there, and warn with how many are missing", () => {
  const { unmount } = render(<ReadinessStrip {...READY} testFileCount={2} />);
  expect(screen.getByText("2 test files")).toBeInTheDocument();
  unmount();
  const s = strip({ testFileCount: 1, missingTestFiles: ["cv.txt"] });
  const warn = within(s).getByText("1 test file missing");
  expect(warn).toHaveClass("text-warning");
  // Which one, for a person who hovers.
  expect(warn).toHaveAttribute("title", "Not in the Test files folder: cv.txt");
});

test("several missing test files are counted in the plural", () => {
  const s = strip({ missingTestFiles: ["a.txt", "b.txt"] });
  expect(within(s).getByText("2 test files missing")).toBeInTheDocument();
});

test("Open setup asks for the Setup panel", () => {
  const onOpenSetup = vi.fn();
  strip({ onOpenSetup });
  fireEvent.click(screen.getByRole("button", { name: "Open setup" }));
  expect(onOpenSetup).toHaveBeenCalledTimes(1);
});
