#!/usr/bin/env node
// Runs the pre-commit checks from `.lintstagedrc.cjs` against the working changes,
// for checkouts where the husky hook never runs. A Gin workspace hands work over with
// `gin publish` instead of a commit, so run this before publishing.
//
//   node scripts/precheck.mjs            # changed files
//   node scripts/precheck.mjs <path>...  # only these files
//
// The changed set is `git diff --name-only HEAD` plus `git ls-files --others
// --exclude-standard`; in a Gin workspace with no Git attached it is `gin diff --names`.
// Each rule runs exactly as lint-staged would run it: a string rule gets the matching
// files appended, a function rule is called with them and returns its commands.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// Windows caps a command line at 8191 characters; leave room for the command itself.
const MAX_FILE_ARGS_CHARS = process.platform === "win32" ? 6000 : 100_000;

/** Expands one level of `{a,b}` alternatives, then converts `*`, `**` and `?` to a RegExp. */
export function globToRegExp(glob) {
  const open = glob.indexOf("{");
  const close = open === -1 ? -1 : glob.indexOf("}", open);
  if (open !== -1 && close !== -1) {
    const head = glob.slice(0, open);
    const tail = glob.slice(close + 1);
    const sources = glob
      .slice(open + 1, close)
      .split(",")
      .map((alt) => globToRegExp(head + alt + tail).source.slice(1, -1));
    return new RegExp(`^(?:${sources.join("|")})$`);
  }
  let source = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      i++;
      if (glob[i + 1] === "/") {
        i++;
        source += "(?:.*/)?";
      } else {
        source += ".*";
      }
    } else if (c === "*") {
      source += "[^/]*";
    } else if (c === "?") {
      source += "[^/]";
    } else {
      source += c.replace(/[.+^$()|[\]\\]/g, "\\$&");
    }
  }
  return new RegExp(`^${source}$`);
}

/**
 * Pairs every rule of a lint-staged config with the files it matches. Like lint-staged,
 * a pattern without `/` matches a file's base name, any other pattern its whole path.
 */
export function matchRules(config, files) {
  const rules = [];
  for (const [pattern, rule] of Object.entries(config)) {
    const re = globToRegExp(pattern);
    const byBase = !pattern.includes("/");
    const matched = files.filter((file) => re.test(byBase ? path.posix.basename(file) : file));
    if (matched.length > 0) rules.push({ pattern, rule, files: matched });
  }
  return rules;
}

/** Splits `files` into chunks whose joined length stays under `limit` characters. */
export function chunkFiles(files, limit) {
  const chunks = [];
  let current = [];
  let length = 0;
  for (const file of files) {
    if (current.length > 0 && length + file.length + 1 > limit) {
      chunks.push(current);
      current = [];
      length = 0;
    }
    current.push(file);
    length += file.length + 1;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

/** The commands one matched rule runs, each an argv array. */
export function commandsFor(rule, absoluteFiles) {
  const asArgv = (command) => command.trim().split(/\s+/);
  if (typeof rule === "function") {
    const result = rule(absoluteFiles);
    return (Array.isArray(result) ? result : [result]).map(asArgv);
  }
  const commands = Array.isArray(rule) ? rule : [rule];
  return chunkFiles(absoluteFiles, MAX_FILE_ARGS_CHARS).flatMap((chunk) =>
    commands.map((command) => [...asArgv(command), ...chunk]),
  );
}

function listFiles(program, args) {
  const result = spawnSync(program, args, { cwd: ROOT, encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${program} ${args.join(" ")} failed: ${result.error?.message ?? result.stderr.trim()}`,
    );
  }
  return result.stdout.split(/\r?\n/).filter(Boolean);
}

function changedFiles() {
  let files;
  try {
    // A Git checkout: changes against HEAD, plus untracked files.
    files = [
      ...listFiles("git", ["diff", "--name-only", "HEAD"]),
      ...listFiles("git", ["ls-files", "--others", "--exclude-standard"]),
    ];
  } catch (gitError) {
    // A Gin workspace with no Git attached: every file changed since it was created.
    try {
      files = listFiles("gin", ["diff", "--names"]);
    } catch (ginError) {
      throw new Error(`${gitError.message}\n${ginError.message}`);
    }
  }
  // Deleted files are listed but have nothing to check.
  return [...new Set(files)].filter((file) => fs.existsSync(path.join(ROOT, file))).sort();
}

function quoteForCmd(arg) {
  return /^[\w./:\\=-]+$/.test(arg) ? arg : `"${arg}"`;
}

function runCommand(argv) {
  const env = {
    ...process.env,
    PATH: `${path.join(ROOT, "node_modules", ".bin")}${path.delimiter}${process.env.PATH ?? ""}`,
  };
  // Windows needs a shell to start the `.cmd` shims in node_modules/.bin.
  const result =
    process.platform === "win32"
      ? spawnSync(argv.map(quoteForCmd).join(" "), {
          cwd: ROOT,
          env,
          stdio: "inherit",
          shell: true,
        })
      : spawnSync(argv[0], argv.slice(1), { cwd: ROOT, env, stdio: "inherit" });
  return result.status === 0;
}

function main() {
  const args = process.argv.slice(2);
  const absolute =
    args.length > 0
      ? args.map((file) => path.resolve(file))
      : changedFiles().map((file) => path.join(ROOT, file));
  const relative = absolute.map((file) => path.relative(ROOT, file).split(path.sep).join("/"));
  if (relative.length === 0) {
    process.stdout.write("precheck: no changed files\n");
    return;
  }
  const config = createRequire(import.meta.url)(path.join(ROOT, ".lintstagedrc.cjs"));
  let failed = 0;
  for (const { pattern, rule, files } of matchRules(config, relative)) {
    for (const argv of commandsFor(
      rule,
      files.map((file) => path.join(ROOT, file)),
    )) {
      const line = argv.join(" ");
      const shown = line.length > 160 ? `${line.slice(0, 157)}...` : line;
      process.stdout.write(`precheck: ${pattern}: ${shown}\n`);
      if (!runCommand(argv)) {
        failed++;
        process.stderr.write(`precheck: FAILED: ${shown}\n`);
      }
    }
  }
  if (failed > 0) {
    process.stderr.write(`precheck: ${failed} check(s) failed\n`);
    process.exit(1);
  }
  process.stdout.write("precheck: all checks passed\n");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`precheck: ${error.message}\n`);
    process.exit(1);
  }
}
