import { beforeEach, describe, expect, it } from "vitest";
import { applyLocalStorage, collectLocalStorage } from "./backup";

describe("backup localStorage half", () => {
  beforeEach(() => localStorage.clear());

  it("collects only tcm-v2-* keys", () => {
    localStorage.setItem("tcm-v2-theme", "dark");
    localStorage.setItem("tcm-v2-prefs", "{}");
    localStorage.setItem("someone-elses-key", "x");
    const got = collectLocalStorage();
    expect(got).toEqual({ "tcm-v2-theme": "dark", "tcm-v2-prefs": "{}" });
  });

  it("apply replaces the namespace: missing keys are removed, foreign keys survive", () => {
    localStorage.setItem("tcm-v2-theme", "light");
    localStorage.setItem("tcm-v2-old-draft", "stale");
    localStorage.setItem("someone-elses-key", "x");
    const n = applyLocalStorage({ "tcm-v2-theme": "dark", "tcm-v2-prefs": "{}" });
    expect(n).toBe(2);
    expect(localStorage.getItem("tcm-v2-theme")).toBe("dark");
    expect(localStorage.getItem("tcm-v2-prefs")).toBe("{}");
    // The key the backup didn't have is gone - import means "match that
    // machine", not a merge...
    expect(localStorage.getItem("tcm-v2-old-draft")).toBeNull();
    // ...but keys outside the app's namespace are not ours to touch.
    expect(localStorage.getItem("someone-elses-key")).toBe("x");
  });

  it("apply ignores keys outside the prefix in a crafted backup", () => {
    const n = applyLocalStorage({ evil: "1", "tcm-v2-ok": "2" });
    expect(n).toBe(1);
    expect(localStorage.getItem("evil")).toBeNull();
    expect(localStorage.getItem("tcm-v2-ok")).toBe("2");
  });
});
