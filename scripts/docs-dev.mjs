// `npm run docs:dev`: the dev app with WebView2's remote-debugging port
// open, so `npm run docs:shots` can drive it. The variable is set here, in
// the child's environment, rather than in the npm script string, so the
// same command works from Git Bash, PowerShell and cmd.

import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CDP_PORT, NO_SIGN_IN_NOTICE } from "./docs-shots-lib.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Signed out, nothing reaches Azure DevOps - so the assigned-work check,
// which starts at sign-in, never runs and no real data can reach a shot.
console.log(`${NO_SIGN_IN_NOTICE}\n`);

// One command string (not command + args) with a shell: npm is a .cmd on
// Windows, and Node refuses to spawn one without a shell.
const child = spawn("npm run tauri dev", {
  cwd: repo,
  shell: true,
  stdio: "inherit",
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}` },
});

child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
child.on("error", (e) => {
  console.error(`Could not start the app: ${e.message}`);
  process.exit(1);
});
