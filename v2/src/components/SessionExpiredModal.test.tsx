// The one prompt an expired session produces: sign in again, or keep
// reading cached data and be asked again on the next failure.

import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import SessionExpiredModal from "./SessionExpiredModal";

test("offers re-sign-in and a way out", () => {
  const onSignIn = vi.fn();
  const onDismiss = vi.fn();
  render(<SessionExpiredModal signingIn={false} onSignIn={onSignIn} onDismiss={onDismiss} />);

  expect(screen.getByText("Session expired")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Sign in again/ }));
  expect(onSignIn).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: /Not now/ }));
  expect(onDismiss).toHaveBeenCalledTimes(1);
});

test("both buttons lock while the Microsoft flow is open", () => {
  render(<SessionExpiredModal signingIn onSignIn={() => {}} onDismiss={() => {}} />);
  expect(screen.getByRole("button", { name: /Waiting for Microsoft/ })).toBeDisabled();
  expect(screen.getByRole("button", { name: /Not now/ })).toBeDisabled();
});
