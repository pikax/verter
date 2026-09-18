#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const COMMITTED_ROOT = join(REPO_ROOT, "packages", "proto", "src", "gen");
const COMMITTED_TEMPLATE = join(REPO_ROOT, "buf.gen.yaml");
const COMMITTED_OUT_LINE = /^(\s*)out: packages\/proto\/src\/gen$/gm;

// The scratch regeneration runs the committed template's exact plugin options
// (target, import extension, ...) and redirects only the output directory. A
// hand-copied template would let the two drift and pass a freshness check
// against options the real `pnpm proto:gen` never used.
function scratchTemplate(committed, outputDirectory) {
  const matches = [...committed.matchAll(COMMITTED_OUT_LINE)];
  if (matches.length !== 1) {
    throw new Error(
      `buf.gen.yaml must declare exactly one "out: packages/proto/src/gen" line (found ${matches.length})`,
    );
  }
  return committed.replace(COMMITTED_OUT_LINE, `$1out: ${JSON.stringify(outputDirectory)}`);
}

function runWorkspaceTool(packagePath, args) {
  const result = spawnSync(process.execPath, [join(REPO_ROOT, packagePath), ...args], {
    cwd: REPO_ROOT,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.signal) throw new Error(`${args.join(" ")} was killed by ${result.signal}`);
  if (result.status !== 0) {
    throw new Error(`${packagePath} exited with status ${result.status ?? "unknown"}`);
  }
}

function filesBelow(root, directory = root, rows = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) filesBelow(root, path, rows);
    else if (entry.isFile()) rows.push(relative(root, path).replaceAll("\\", "/"));
  }
  return rows.sort();
}

const scratch = mkdtempSync(join(tmpdir(), "verter-proto-check-"));
try {
  const generatedRoot = join(scratch, "gen");
  mkdirSync(generatedRoot);
  const output = generatedRoot.replaceAll("\\", "/");
  const template = join(scratch, "buf.gen.yaml");
  writeFileSync(template, scratchTemplate(readFileSync(COMMITTED_TEMPLATE, "utf8"), output));
  runWorkspaceTool("node_modules/@bufbuild/buf/bin/buf", ["generate", "--template", template]);
  runWorkspaceTool("node_modules/oxfmt/bin/oxfmt", [generatedRoot]);

  const committed = filesBelow(COMMITTED_ROOT);
  const regenerated = filesBelow(generatedRoot);
  const errors = [];
  if (JSON.stringify(committed) !== JSON.stringify(regenerated)) {
    errors.push(
      `file inventory differs:\ncommitted=${committed.join(", ")}\nregenerated=${regenerated.join(", ")}`,
    );
  }
  for (const file of committed.filter((file) => regenerated.includes(file))) {
    if (!readFileSync(join(COMMITTED_ROOT, file)).equals(readFileSync(join(generatedRoot, file)))) {
      errors.push(`${file} differs from buf generate + oxfmt output`);
    }
  }
  if (errors.length > 0) {
    process.stderr.write(
      `Generated proto bindings are stale. Run \`pnpm proto:gen\`.\n${errors.join("\n")}\n`,
    );
    process.exitCode = 1;
  } else {
    process.stdout.write(`Generated proto bindings are fresh (${committed.length} files).\n`);
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
