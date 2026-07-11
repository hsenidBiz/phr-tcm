import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// vitest runs without injected globals, so RTL's automatic cleanup never
// registers - without this, each test's DOM leaks into the next.
afterEach(() => cleanup());
