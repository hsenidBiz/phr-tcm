/**
 * Accessibility / usability gate - axe-core over rendered screens.
 *
 * This is the "is the app understandable" layer the behavioral tests
 * don't cover: every control must have a name a screen reader (or a
 * confused user hovering for a tooltip) can read, headings must nest in
 * order, form fields must be labelled. Color-contrast is excluded ONLY
 * because jsdom does not paint - the token system + the consistency gate
 * carry that concern, and the palettes were chosen against WCAG AA.
 *
 * When this fails: give the control an aria-label/title, or fix the
 * heading order - do not disable the rule.
 */
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, expect, test } from "vitest";
import type { ReactElement } from "react";
import AiBridge from "./screens/AiBridge";
import PrPanel from "./screens/PrPanel";
import Settings from "./screens/Settings";
import SignIn from "./screens/SignIn";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

async function expectAccessible(ui: ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const { container } = render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
  // Let initial queries settle so the real content (not just skeletons)
  // is what gets audited.
  await new Promise((r) => setTimeout(r, 50));
  const results = await axe.run(container, {
    rules: { "color-contrast": { enabled: false } }, // jsdom cannot paint
  });
  const problems = results.violations.map(
    (v) => `${v.id}: ${v.help} -> ${v.nodes.map((n) => n.html.slice(0, 80)).join(" | ")}`,
  );
  expect(problems).toEqual([]);
}

test("SignIn is accessible", async () => {
  await expectAccessible(<SignIn signingIn={false} onSignIn={() => {}} />);
});

test("Settings is accessible", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs") return [];
    if (cmd === "app_log_dir") return "C:\\logs";
    return undefined;
  });
  await expectAccessible(<Settings org="acme" project="Web" />);
});

test("AI Bridge is accessible", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [
        { id: "claude-code", name: "Claude Code", installed: true, registered_servers: ["tcm-testcases"] },
        { id: "vscode", name: "VS Code", installed: true, registered_servers: [] },
      ];
  });
  await expectAccessible(<AiBridge />);
});

test("Pull Requests panel is accessible", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview")
      return {
        awaiting: [
          {
            id: 1, title: "A change", repo: "web", repo_id: "g1", status: "active",
            closed: "", merge_commit: "", author: "Sam", source_branch: "f/x",
            target_branch: "main", created: "2026-07-18T01:00:00Z", description: "",
            is_draft: false, has_conflicts: false, my_vote: 0,
            reviewers: [{ display_name: "Kim", vote: 10 }],
            web_url: "https://example.invalid/pr",
          },
        ],
        mine: [],
      };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "repo_pull_requests") return [];
  });
  await expectAccessible(<PrPanel org="acme" project="Web" />);
});
