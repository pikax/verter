#!/usr/bin/env node
import { spawn } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { compare } from "./compare.mjs";
import { normalize, resolveRoot } from "./normalize.mjs";

const usage = `Usage: node scripts/sfc-projection/parity/run.mjs
  --left-file <recording> | --left-command '<JSON argv array>'
  --right-file <recording> | --right-command '<JSON argv array>'
  [--left-root <directory>] [--right-root <directory>]
  [--mode verter|change|stock] [--accepted <JSON file>] [--report <JSON file>]

Commands run directly, without a shell, in their root (default: current directory).
Recording, accepted-list and report paths are relative to the current directory.
Change mode also requires seeded diagnostics on the left. Stock mode ignores
columns in equality keys and permits exact evidenced right-only accepted rows.
Evidence paths are relative to the accepted-list file. Output is a JSON report;
exit 0 means parity, 1 means differences, and 2 means invalid input or run failure.
`;

function parseArgs(args) {
  const allowed = new Set([
    "left-file",
    "right-file",
    "left-command",
    "right-command",
    "left-root",
    "right-root",
    "mode",
    "accepted",
    "report",
  ]);
  const options = {};
  for (let i = 0; i < args.length; i++) {
    const name = args[i].slice(2);
    if (!args[i].startsWith("--") || !allowed.has(name) || Object.hasOwn(options, name))
      throw new Error(`Unknown or repeated option: ${args[i]}`);
    const value = args[++i];
    if (!value || value.startsWith("--")) throw new Error(`Missing value for --${name}`);
    options[name] = value;
  }
  options.mode ??= "verter";
  if (!["verter", "change", "stock"].includes(options.mode))
    throw new Error("Mode must be verter, change or stock");
  if (options.accepted && options.mode !== "stock")
    throw new Error("Accepted rows require stock mode");
  for (const side of ["left", "right"]) {
    if (Boolean(options[`${side}-file`]) === Boolean(options[`${side}-command`]))
      throw new Error(`Provide exactly one --${side}-file or --${side}-command`);
    options[`${side}-root`] = resolveRoot(options[`${side}-root`] ?? ".");
    if (options[`${side}-command`]) {
      const argv = JSON.parse(options[`${side}-command`]);
      if (
        !Array.isArray(argv) ||
        !argv.length ||
        argv.some((arg) => typeof arg !== "string") ||
        !argv[0]
      )
        throw new Error(`--${side}-command must be a nonempty JSON argv array`);
      options[`${side}-command`] = argv;
    }
  }
  return options;
}

/** Capture a command's diagnostic text and completion; spawning is injectable. */
export function runCommand(argv, cwd, spawnCommand = spawn) {
  return new Promise((resolve, reject) => {
    const child = spawnCommand(argv[0], argv.slice(1), { cwd, stdio: ["ignore", "pipe", "pipe"] });
    // Publish complete lines to one ordered sink, keeping partial writes apart.
    const lines = [];
    const buffers = [];
    let sequence = 0;
    const flush = (buffer) => {
      if (buffer.text) lines.push(buffer.text);
      buffer.text = "";
    };
    for (const stream of [child.stdout, child.stderr]) {
      const buffer = { text: "", last: 0 };
      buffers.push(buffer);
      stream.setEncoding("utf8").on("data", (chunk) => {
        buffer.last = sequence++;
        const parts = (buffer.text + chunk).split("\n");
        buffer.text = parts.pop();
        lines.push(...parts);
      });
      stream.on("end", () => flush(buffer));
      stream.on("error", reject);
    }
    child.on("error", reject);
    child.on("close", (exit, signal) => {
      if (signal || exit === null)
        reject(new Error(`Command terminated without an exit status: ${signal}`));
      else {
        buffers.sort((left, right) => left.last - right.last).forEach(flush);
        resolve({ output: lines.join("\n"), exit });
      }
    });
  });
}

async function input(options, side, spawnCommand) {
  const root = options[`${side}-root`];
  const file = options[`${side}-file`];
  const result = file
    ? { output: await readFile(file, "utf8") }
    : await runCommand(options[`${side}-command`], root, spawnCommand);
  const rows = normalize(result.output, root);
  if (result.exit !== undefined && result.exit !== 0 && !rows.length)
    throw new Error(`${side} command exited ${result.exit} without parseable diagnostics`);
  return result.exit === undefined ? { rows } : { rows, exit: result.exit };
}

async function acceptedDifferences(file, differences) {
  if (!file) return [];
  const rows = JSON.parse(await readFile(file, "utf8"));
  if (!Array.isArray(rows)) throw new Error("Accepted list must be a JSON array");
  const seen = new Set();
  for (const row of rows) {
    const fields = ["file", "code", "class", "justification", "evidence", "fixedBy"];
    if (
      !row ||
      fields.some((field) => typeof row[field] !== "string" || !row[field].trim()) ||
      !Number.isSafeInteger(row.line) ||
      row.line < 1 ||
      !Number.isSafeInteger(row.count) ||
      row.count < 1 ||
      !["correction", "engine"].includes(row.class) ||
      !/^(TS|VTER)\d+$/.test(row.code) ||
      /[\\*?\[\]{}]/.test(row.file) ||
      path.posix.isAbsolute(row.file) ||
      /^[A-Za-z]:/.test(row.file) ||
      path.posix.normalize(row.file) !== row.file
    ) {
      throw new Error(
        "Accepted row requires exact file, line, code, count, class, justification, evidence and fixedBy",
      );
    }
    const id = JSON.stringify([row.file, row.line, row.code]);
    if (seen.has(id)) throw new Error(`Duplicate accepted row: ${id}`);
    seen.add(id);
    const evidence = await readFile(path.resolve(path.dirname(file), row.evidence), "utf8");
    if (!new RegExp(`\\b${row.code}\\b`).test(evidence))
      throw new Error(`Evidence does not contain ${row.code}: ${row.evidence}`);
    const difference = differences.find(
      (delta) =>
        delta.class === "right-only" &&
        delta.right[0].file === row.file &&
        delta.right[0].line === row.line &&
        delta.right[0].code === row.code,
    );
    if (!difference || difference.count !== row.count)
      throw new Error(`Accepted row does not match a current right-only count: ${id}`);
  }
  return rows;
}

/**
 * Execute a parity run. Exact accepted rows never alter the strict comparator.
 * `spawnCommand` (default `child_process.spawn`) starts both sides' commands,
 * so a caller can observe the processes it runs.
 */
export async function run(args, { spawnCommand } = {}) {
  const options = parseArgs(args);
  const left = await input(options, "left", spawnCommand);
  const right = await input(options, "right", spawnCommand);
  if (options.mode === "change" && !left.rows.length)
    throw new Error("Change detection requires seeded left diagnostics");
  const comparison = compare(left.rows, right.rows, {
    stock: options.mode === "stock",
    leftExit: left.exit,
    rightExit: right.exit,
  });
  const accepted = await acceptedDifferences(options.accepted, comparison.differences);
  const remaining = comparison.differences.filter(
    (delta) =>
      delta.class !== "right-only" ||
      !accepted.some(
        (row) =>
          row.file === delta.right[0].file &&
          row.line === delta.right[0].line &&
          row.code === delta.right[0].code,
      ),
  );
  const pass = remaining.length === 0 && comparison.gates.exit !== false;
  const report = { mode: options.mode, pass, ...comparison, left, right, accepted };
  if (options.report) await writeFile(options.report, `${JSON.stringify(report, null, 2)}\n`);
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length === 3 && process.argv[2] === "--help") process.stdout.write(usage);
  else
    run(process.argv.slice(2))
      .then((report) => {
        process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
        process.exitCode = report.pass ? 0 : 1;
      })
      .catch((error) => {
        process.stderr.write(`parity: ${error.message}\n`);
        process.exitCode = 2;
      });
}
