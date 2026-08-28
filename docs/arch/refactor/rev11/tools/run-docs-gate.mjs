#!/usr/bin/env node
import childProcess from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const toolsRoot = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(toolsRoot, "..");
const repository = childProcess.execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: packageRoot, encoding: "utf8", timeout: 30_000, killSignal: "SIGKILL" }).trim();
const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");
const relative = (file) => path.relative(repository, file).split(path.sep).join("/");
const scripts = fs.readdirSync(toolsRoot, { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".mjs"))
  .map((entry) => path.join(toolsRoot, entry.name)).sort();
const tests = scripts.filter((file) => file.endsWith(".test.mjs"));
const validators = [
  "build-program-dag.mjs --check",
  "build-source-clauses.mjs --check",
  "build-collapse-map.mjs --check",
  "build-conflict-ownership.mjs --check",
  "build-operational-charters.mjs --check",
  "validate-program-dag.mjs --strict",
  "validate-charters.mjs --strict",
  "validate-orchestration-state.mjs --strict",
  "validate-negative-controls.mjs",
  "self-test.mjs",
];
const plan = {
  schema: 1,
  type: "unified-docs-gate-plan",
  package_root: relative(packageRoot),
  syntax_inputs: scripts.map(relative),
  test_inputs: tests.map(relative),
  validators: validators.map((entry) => {
    const [name, ...args] = entry.split(" ");
    return `node ${relative(path.join(toolsRoot, name))}${args.length ? ` ${args.join(" ")}` : ""}`;
  }),
};
plan.discovery_sha256 = sha256(`${JSON.stringify(plan)}\n`);

if (process.argv.includes("--list")) {
  process.stdout.write(`${JSON.stringify(plan, null, 2)}\n`);
  process.exit(0);
}

const startedAt = new Date().toISOString();
const results = [];
function run(label, commandArgs, timeout = 300_000) {
  const start = Date.now();
  const result = childProcess.spawnSync(process.execPath, commandArgs, { cwd: repository, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout, killSignal: "SIGKILL" });
  const stdout = result.stdout || ""; const stderr = result.stderr || "";
  const row = { label, command: [process.execPath, ...commandArgs], status: result.status, signal: result.signal || "", elapsed_ms: Date.now() - start, stdout_sha256: sha256(stdout), stderr_sha256: sha256(stderr) };
  results.push(row);
  if (result.error || result.status !== 0) {
    process.stderr.write(stdout); process.stderr.write(stderr);
    throw new Error(`${label} failed${result.error ? `: ${result.error.message}` : ` with status ${result.status}${result.signal ? ` signal ${result.signal}` : ""}`}`);
  }
}

try {
  for (const file of scripts) run(`syntax:${relative(file)}`, ["--check", file], 30_000);
  for (const entry of validators) {
    const [name, ...args] = entry.split(" ");
    run(`validator:${name}`, [path.join(toolsRoot, name), ...args], 300_000);
  }
  for (const file of tests) run(`test:${relative(file)}`, ["--test", file], 600_000);
  process.stdout.write(`${JSON.stringify({ ...plan, started_at: startedAt, completed_at: new Date().toISOString(), terminal_summary: "PASS", unexpected_skips: 0, results }, null, 2)}\n`);
} catch (error) {
  process.stderr.write(`${JSON.stringify({ ...plan, started_at: startedAt, completed_at: new Date().toISOString(), terminal_summary: "FAIL", unexpected_skips: 0, results, error: error.message }, null, 2)}\n`);
  process.exitCode = 1;
}
