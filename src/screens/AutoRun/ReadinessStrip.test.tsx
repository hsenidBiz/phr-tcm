// The one line the Test cases tab opens with: where runs go, and anything
// the setup is missing. What is in place is the Setup panel's to show.

import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ReadinessStrip from "./ReadinessStrip";

type Props = Parameters<typeof ReadinessStrip>[0];

const READY: Props = {
  envName: "QA",
  siteHost: "qa.example.com",
  signIn: "saved",
  accountCount: 2,
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

test("a sign-in that is in place is not repeated here, and none warns", () => {
  const { unmount } = render(<ReadinessStrip {...READY} signIn="builtin" />);
  expect(screen.queryByText("Built-in")).not.toBeInTheDocument();
  expect(screen.queryByText(/Sign-in/)).not.toBeInTheDocument();
  unmount();
  const s = strip({ signIn: "none" });
  expect(within(s).getByText("No sign-in")).toHaveClass("text-warning");
});

test("accounts are not counted here, but none at all warns", () => {
  const { unmount } = render(<ReadinessStrip {...READY} accountCount={1} />);
  expect(screen.queryByText(/account/)).not.toBeInTheDocument();
  expect(screen.queryByText(/area/)).not.toBeInTheDocument();
  unmount();
  const s = strip({ accountCount: 0 });
  expect(within(s).getByText("No accounts")).toHaveClass("text-warning");
});

test("a count still being read is left out rather than guessed", () => {
  const s = strip({ signIn: null, accountCount: null });
  expect(within(s).queryByText(/account/)).not.toBeInTheDocument();
  expect(within(s).queryByText(/sign-in/i)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Open setup" })).not.toBeInTheDocument();
});

test("test files warn with how many are missing, and are not counted when all are there", () => {
  const { unmount } = render(<ReadinessStrip {...READY} />);
  expect(screen.queryByText(/test file/)).not.toBeInTheDocument();
  unmount();
  const s = strip({ missingTestFiles: ["cv.txt"] });
  const warn = within(s).getByText("1 test file missing");
  expect(warn).toHaveClass("text-warning");
  // Which one, for a person who hovers.
  expect(warn).toHaveAttribute("title", "Not in the Test files folder: cv.txt");
});

test("several missing test files are counted in the plural", () => {
  const s = strip({ missingTestFiles: ["a.txt", "b.txt"] });
  expect(within(s).getByText("2 test files missing")).toBeInTheDocument();
});

test("with nothing missing the strip is the environment alone, with no Open setup", () => {
  const s = strip();
  expect(s).toHaveTextContent("Environment QA - qa.example.com");
  expect(screen.queryByRole("button", { name: "Open setup" })).not.toBeInTheDocument();
  expect(s.querySelector("svg")).toBeNull();
});

test("Open setup, while something is missing, asks for the Setup panel", () => {
  const onOpenSetup = vi.fn();
  strip({ onOpenSetup, accountCount: 0 });
  fireEvent.click(screen.getByRole("button", { name: "Open setup" }));
  expect(onOpenSetup).toHaveBeenCalledTimes(1);
});
