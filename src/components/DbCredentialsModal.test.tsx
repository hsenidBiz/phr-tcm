import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { DbDatabase } from "../bindings";

const testDb = vi.fn();
const saveDb = vi.fn();
const resetDb = vi.fn();
vi.mock("../bindings", () => ({
  commands: {
    testDbConnection: (...a: unknown[]) => testDb(...a),
    saveDbCredentials: (...a: unknown[]) => saveDb(...a),
    resetDbCredentials: (...a: unknown[]) => resetDb(...a),
  },
}));

import { DbCredentialsModal } from "./DbCredentialsModal";

beforeEach(() => {
  testDb.mockReset();
  saveDb.mockReset();
  resetDb.mockReset();
});

const DEV: DbDatabase = {
  id: "dev-read",
  label: "Dev - read only",
  shipped: true,
  server: "sgdev01db02.cloud",
  port: 1433,
  database: "phrx",
  user: "sgdev01db02_readonly",
  trust_cert: true,
  has_password: true,
  customised: false,
};

const OWN: DbDatabase = {
  id: "own",
  label: "Your own database",
  shipped: false,
  server: "",
  port: null,
  database: "",
  user: "",
  trust_cert: true,
  has_password: false,
  customised: false,
};

function open(database: DbDatabase, onSaved = vi.fn(), onClose = vi.fn()) {
  render(<DbCredentialsModal database={database} onClose={onClose} onSaved={onSaved} />);
  return { onSaved, onClose };
}

test("the dialog is named for the database it edits", () => {
  open(DEV);
  expect(screen.getByRole("dialog", { name: "Credentials for Dev - read only" })).toBeInTheDocument();
});

/// A shipped database's address is the app's, not the person's: shown,
/// not offered for editing. Only who signs in is theirs to change.
test("a shipped database shows its server and database as text", () => {
  open(DEV);
  expect(screen.getByText("sgdev01db02.cloud,1433")).toBeInTheDocument();
  expect(screen.getByText("phrx")).toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Server" })).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Database" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "User" })).toHaveValue("sgdev01db02_readonly");
  expect(screen.getByLabelText("Password")).toHaveAttribute("type", "password");
});

test("the password is never pre-filled; a saved one says so", () => {
  open(DEV);
  const pw = screen.getByLabelText("Password");
  expect(pw).toHaveValue("");
  expect(pw).toHaveAttribute("placeholder", "Saved - leave blank to keep it");
});

test("your own database offers every part of the connection", () => {
  open(OWN);
  expect(screen.getByRole("textbox", { name: "Server" })).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Port" })).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Database" })).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "User" })).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Trust the server certificate" })).toBeInTheDocument();
  // Nothing saved, so no promise that something is.
  expect(screen.getByLabelText("Password")).not.toHaveAttribute(
    "placeholder",
    "Saved - leave blank to keep it",
  );
});

test("Test connection sends the form, a blank password as null, and shows the answer", async () => {
  let answer!: (v: unknown) => void;
  testDb.mockReturnValue(new Promise((r) => (answer = r)));
  open(DEV);

  fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
  expect(await screen.findByRole("button", { name: "Testing" })).toBeDisabled();
  expect(testDb).toHaveBeenCalledWith("dev-read", {
    server: "sgdev01db02.cloud",
    port: 1433,
    database: "phrx",
    user: "sgdev01db02_readonly",
    password: null,
    trust_cert: true,
  });

  answer({ status: "ok", data: "Connected to phrx on sgdev01db02.cloud as sgdev01db02_readonly." });
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("Connected to phrx on sgdev01db02.cloud as sgdev01db02_readonly.");
  expect(status).toHaveClass("text-success");
  expect(screen.getByRole("button", { name: "Test connection" })).not.toBeDisabled();
});

test("a typed password is what gets tested", async () => {
  testDb.mockResolvedValue({ status: "ok", data: "Connected." });
  open(DEV);
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "n3w" } });
  fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
  await waitFor(() => expect(testDb).toHaveBeenCalled());
  expect(testDb.mock.calls[0][1]).toMatchObject({ password: "n3w" });
});

test("a failed test shows the server's reason as an error", async () => {
  testDb.mockResolvedValue({ status: "error", error: "Login failed for user 'sgdev01db02_readonly'." });
  open(DEV);
  fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("Login failed for user 'sgdev01db02_readonly'.");
  expect(status).toHaveClass("text-danger");
});

test("Reset to default is offered only for a shipped database with its own login saved", () => {
  const first = render(
    <DbCredentialsModal database={DEV} onClose={vi.fn()} onSaved={vi.fn()} />,
  );
  expect(screen.queryByRole("button", { name: "Reset to default" })).not.toBeInTheDocument();
  first.unmount();

  const second = render(
    <DbCredentialsModal database={OWN} onClose={vi.fn()} onSaved={vi.fn()} />,
  );
  expect(screen.queryByRole("button", { name: "Reset to default" })).not.toBeInTheDocument();
  second.unmount();

  render(
    <DbCredentialsModal database={{ ...DEV, customised: true }} onClose={vi.fn()} onSaved={vi.fn()} />,
  );
  expect(screen.getByRole("button", { name: "Reset to default" })).toBeInTheDocument();
});

test("Reset to default puts the shipped login back", async () => {
  const back = { ...DEV, customised: false };
  resetDb.mockResolvedValue({ status: "ok", data: back });
  const { onSaved, onClose } = open({ ...DEV, customised: true });
  fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledWith(back));
  expect(resetDb).toHaveBeenCalledWith("dev-read");
  expect(onClose).toHaveBeenCalled();
});

test("Save stores the login, then hands the new view back and closes", async () => {
  const saved = { ...OWN, server: "sql.local", database: "HR", user: "me", has_password: true, customised: true };
  saveDb.mockResolvedValue({ status: "ok", data: saved });
  const { onSaved, onClose } = open(OWN);

  fireEvent.change(screen.getByRole("textbox", { name: "Server" }), { target: { value: "sql.local" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Database" }), { target: { value: "HR" } });
  fireEvent.change(screen.getByRole("textbox", { name: "User" }), { target: { value: "me" } });
  fireEvent.change(screen.getByLabelText("Password"), { target: { value: "pw" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() => expect(onSaved).toHaveBeenCalledWith(saved));
  expect(saveDb).toHaveBeenCalledWith("own", {
    server: "sql.local",
    port: null,
    database: "HR",
    user: "me",
    password: "pw",
    trust_cert: true,
  });
  expect(onClose).toHaveBeenCalled();
});

test("a refused save stays open and says why", async () => {
  saveDb.mockResolvedValue({ status: "error", error: "Could not save the login in Windows Credential Manager." });
  const { onSaved, onClose } = open(DEV);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("Could not save the login in Windows Credential Manager.");
  expect(status).toHaveClass("text-danger");
  expect(onSaved).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});

test("Cancel closes without saving", () => {
  const { onClose } = open(DEV);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(onClose).toHaveBeenCalled();
  expect(saveDb).not.toHaveBeenCalled();
});
