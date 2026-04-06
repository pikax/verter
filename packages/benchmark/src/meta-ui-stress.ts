/**
 * Thin wrapper over the shared corpus trace runner.
 *
 * Keeps the existing benchmark entrypoint but delegates timeout ownership
 * and per-component isolation to `scripts/benchmark/trace-component-corpus.mjs`.
 */

import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");
const runnerPath = resolve(repoRoot, "scripts", "benchmark", "trace-component-corpus.mjs");

const child = spawn(process.execPath, [runnerPath, ...process.argv.slice(2)], {
  cwd: repoRoot,
  stdio: "inherit",
  env: process.env,
});

child.once("error", (error) => {
  console.error(error instanceof Error ? (error.stack ?? error.message) : String(error));
  process.exit(1);
});

child.once("exit", (code, signal) => {
  if (signal) {
    console.error(`meta-ui-stress runner exited with signal ${signal}`);
    process.exit(1);
  }
  process.exit(code ?? 1);
});
