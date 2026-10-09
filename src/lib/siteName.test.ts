import { expect, test } from "vitest";
import { siteName } from "./siteName";

test("an address is reduced to its host", () => {
  expect(siteName("https://hrmmainslqaautom.phrsandbox.dev/hr/home/index")).toBe(
    "hrmmainslqaautom.phrsandbox.dev",
  );
  expect(siteName("http://host.example:8080/a?b=c#d")).toBe("host.example");
});

test("text that is not an address comes back unchanged", () => {
  expect(siteName("Using the sign-in recipe's address")).toBe("Using the sign-in recipe's address");
  expect(siteName("")).toBe("");
});
