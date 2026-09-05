import { beforeEach, expect, test, vi } from "vitest";
import { GITHUB_OFF_KEY, githubOffSnapshot, setGithubOff, subscribeGithubOff } from "./updatePrefs";

beforeEach(() => localStorage.clear());

test("off by default; on is remembered; off again removes the key", () => {
  expect(githubOffSnapshot()).toBe(false);
  setGithubOff(true);
  expect(githubOffSnapshot()).toBe(true);
  expect(localStorage.getItem(GITHUB_OFF_KEY)).toBe("1");
  setGithubOff(false);
  expect(localStorage.getItem(GITHUB_OFF_KEY)).toBeNull();
});

test("subscribers hear every change until they unsubscribe", () => {
  const heard = vi.fn();
  const stop = subscribeGithubOff(heard);
  setGithubOff(true);
  expect(heard).toHaveBeenCalledTimes(1);
  stop();
  setGithubOff(false);
  expect(heard).toHaveBeenCalledTimes(1);
});
