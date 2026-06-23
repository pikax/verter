#!/usr/bin/env node
// Cross-platform dispatcher for the Verter Lapce local-install helper.
//
// package.json scripts run under the user's shell, which differs per OS, so this
// tiny Node shim picks the right platform script and spawns it:
//   * Windows (win32) -> install-local.ps1 (via PowerShell)
//   * macOS / Linux   -> install-local.sh  (via bash)
//
// Any extra CLI args are forwarded verbatim (e.g. a channel override). The shim
// exits with the child's exit code so a failed install fails the npm script.

import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const forwarded = process.argv.slice(2);

let command;
let args;

if (process.platform === "win32") {
  const ps1 = path.join(scriptDir, "install-local.ps1");
  // Windows PowerShell (always present on Windows); the .ps1 targets 5.1.
  command = "powershell.exe";
  args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1, ...forwarded];
} else {
  const sh = path.join(scriptDir, "install-local.sh");
  command = "bash";
  args = [sh, ...forwarded];
}

const child = spawn(command, args, { stdio: "inherit" });

child.on("error", (err) => {
  console.error(`failed to launch the Lapce install helper (${command}): ${err.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    console.error(`Lapce install helper terminated by signal ${signal}`);
    process.exit(1);
  }
  process.exit(code ?? 1);
});
