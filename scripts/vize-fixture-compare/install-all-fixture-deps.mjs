#!/usr/bin/env node
/**
 * After `git submodule update --init` on Vize's tests/_fixtures/_git, run the
 * appropriate package-manager install in every populated project root.
 *
 * Usage:
 *   node scripts/vize-fixture-compare/install-all-fixture-deps.mjs \
 *     [--vize-root ../vize] [--dry-run] [--jobs 2] [--skip-existing]
 *
 * Detects pnpm-lock.yaml / yarn.lock / package-lock.json / bun.lockb.
 * Never runs without a package.json. Logs to target/vize-fixture-install/.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(__dirname, "../..");

const args = process.argv.slice(2);
function getArg(name) {
  const i = args.indexOf(name);
  return i !== -1 ? args[i + 1] : null;
}
const dryRun = args.includes("--dry-run");
const skipExisting = args.includes("--skip-existing");
const jobs = Math.max(1, parseInt(getArg("--jobs") || "2", 10));
const vizeRoot = path.resolve(
  getArg("--vize-root") || process.env.VIZE_ROOT || path.join(REPO, "../vize"),
);
const gitRoot = path.join(vizeRoot, "tests/_fixtures/_git");
const outDir = path.join(REPO, "target", "vize-fixture-install");
fs.mkdirSync(outDir, { recursive: true });

function detectPm(dir) {
  if (fs.existsSync(path.join(dir, "pnpm-lock.yaml"))) return "pnpm";
  if (fs.existsSync(path.join(dir, "yarn.lock"))) return "yarn";
  if (fs.existsSync(path.join(dir, "bun.lockb")) || fs.existsSync(path.join(dir, "bun.lock")))
    return "bun";
  if (fs.existsSync(path.join(dir, "package-lock.json"))) return "npm";
  if (fs.existsSync(path.join(dir, "package.json"))) return "npm";
  return null;
}

function findInstallRoots(projectDir) {
  // Prefer project root package.json; also monorepo roots only (not nested packages)
  const roots = [];
  if (fs.existsSync(path.join(projectDir, "package.json"))) {
    roots.push(projectDir);
    return roots;
  }
  // Some fixtures nest the app one level down
  try {
    for (const e of fs.readdirSync(projectDir, { withFileTypes: true })) {
      if (!e.isDirectory() || e.name.startsWith(".")) continue;
      const sub = path.join(projectDir, e.name);
      if (fs.existsSync(path.join(sub, "package.json"))) roots.push(sub);
    }
  } catch {
    /* ignore */
  }
  return roots;
}

if (!fs.existsSync(gitRoot)) {
  console.error(`Missing ${gitRoot}`);
  process.exit(2);
}

const projects = fs
  .readdirSync(gitRoot, { withFileTypes: true })
  .filter((e) => e.isDirectory() && !e.name.startsWith("."))
  .map((e) => e.name)
  .sort();

const tasks = [];
for (const id of projects) {
  const dir = path.join(gitRoot, id);
  // skip empty checkouts (no files beyond .git)
  let hasContent = false;
  try {
    hasContent = fs.readdirSync(dir).some((n) => n !== ".git");
  } catch {
    continue;
  }
  if (!hasContent) continue;
  for (const root of findInstallRoots(dir)) {
    const pm = detectPm(root);
    if (!pm) continue;
    if (skipExisting && fs.existsSync(path.join(root, "node_modules"))) continue;
    tasks.push({ id, root, pm });
  }
}

console.log(`Found ${tasks.length} install root(s) under ${gitRoot}`);
if (dryRun) {
  for (const t of tasks) console.log(`[dry] ${t.pm} install @ ${path.relative(gitRoot, t.root)}`);
  process.exit(0);
}

const results = [];
let i = 0;

function runOne(task) {
  const rel = path.relative(gitRoot, task.root);
  const logFile = path.join(outDir, `${task.id.replace(/[^\w.-]+/g, "_")}.log`);
  const cmd =
    task.pm === "pnpm"
      ? ["pnpm", "install", "--frozen-lockfile"]
      : task.pm === "yarn"
        ? ["yarn", "install", "--frozen-lockfile"]
        : task.pm === "bun"
          ? ["bun", "install", "--frozen-lockfile"]
          : ["npm", "ci"];
  // fallback if frozen fails is handled by retry without frozen
  console.log(`[${++i}/${tasks.length}] ${cmd.join(" ")} @ ${rel}`);
  let r = spawnSync(cmd[0], cmd.slice(1), {
    cwd: task.root,
    encoding: "utf8",
    env: { ...process.env, CI: "1" },
    timeout: 15 * 60 * 1000,
  });
  if (r.status !== 0) {
    const fallback =
      task.pm === "pnpm"
        ? ["pnpm", "install"]
        : task.pm === "yarn"
          ? ["yarn", "install"]
          : task.pm === "bun"
            ? ["bun", "install"]
            : ["npm", "install"];
    console.log(`  frozen failed (${r.status}); retry ${fallback.join(" ")}`);
    r = spawnSync(fallback[0], fallback.slice(1), {
      cwd: task.root,
      encoding: "utf8",
      env: { ...process.env, CI: "1" },
      timeout: 15 * 60 * 1000,
    });
  }
  const log = `cmd: ${cmd.join(" ")}\nstatus: ${r.status}\n\nSTDOUT:\n${r.stdout || ""}\n\nSTDERR:\n${r.stderr || ""}\n`;
  fs.writeFileSync(logFile, log);
  results.push({
    id: task.id,
    root: rel,
    pm: task.pm,
    status: r.status ?? 1,
    logFile,
  });
  if (r.status !== 0) console.log(`  FAIL status=${r.status} log=${logFile}`);
  else console.log(`  OK`);
}

// Sequential by default (jobs>1 can OOM on monorepos). jobs reserved for later.
for (const t of tasks) runOne(t);

const summary = {
  gitRoot,
  total: results.length,
  ok: results.filter((r) => r.status === 0).length,
  fail: results.filter((r) => r.status !== 0).length,
  results,
};
fs.writeFileSync(path.join(outDir, "summary.json"), JSON.stringify(summary, null, 2));
console.log(`\nDone: ${summary.ok} ok / ${summary.fail} fail / ${summary.total} total`);
console.log(`Summary: ${path.join(outDir, "summary.json")}`);
process.exit(summary.fail ? 1 : 0);
