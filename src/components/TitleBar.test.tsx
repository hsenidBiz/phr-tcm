// The window's title bar: the title, and on a beta build a Beta pill
// beside it so every screen says which build this is.

import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { isBetaVersion } from "../lib/changelog";
import { activeEnvironmentLabel } from "../lib/environments";
import TitleBar from "./TitleBar";

test("a stable build shows the title alone", () => {
  render(<TitleBar title="Test Case Manager" />);
  expect(screen.getByText("Test Case Manager")).toBeInTheDocument();
  expect(screen.queryByText("Beta")).not.toBeInTheDocument();
});

test("a beta build shows the Beta pill beside the title", () => {
  render(<TitleBar title="Test Case Manager" beta />);
  const pill = screen.getByText("Beta");
  // In the same group as the title, inside the drag region.
  expect(pill.parentElement).toBe(screen.getByText("Test Case Manager").parentElement);
});

test("the environment pill names the active environment, beside the title", () => {
  render(<TitleBar title="Test Case Manager" environment="QA" />);
  const pill = screen.getByText("QA");
  expect(pill.parentElement).toBe(screen.getByText("Test Case Manager").parentElement);
});

test("no environment, no pill", () => {
  render(<TitleBar title="Test Case Manager" />);
  expect(screen.queryByTitle("Active environment")).not.toBeInTheDocument();
});

test("the title bar names the environment only when there are two or more", () => {
  const env = (id: string, name: string) => ({
    id, name, start_url: "", allowed_origins: [], db_id: "dev-read", test_environment: false, has_default_password: false,
  });
  const one = { active: "env-1", environments: [env("env-1", "Default")] };
  const two = { active: "env-2", environments: [env("env-1", "Default"), env("env-2", "QA")] };
  expect(activeEnvironmentLabel(undefined)).toBeNull();
  expect(activeEnvironmentLabel(one)).toBeNull();
  expect(activeEnvironmentLabel(two)).toBe("QA");
});

test("the pill follows the version the app is running", () => {
  // What App passes as `beta`.
  expect(isBetaVersion("2.0.4-beta.2")).toBe(true);
  expect(isBetaVersion("2.0.3")).toBe(false);
  expect(isBetaVersion("dev")).toBe(false);
});
