#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync, realpathSync, statSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { run } from "./run.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

/** The environment variable that names the corpus directory. */
export const CORPUS_VARIABLE = "VERTER_PARITY_CORPUS";

/**
 * The environment variable that names a private directory for row-level
 * output: when set, each member's full parity report (every row with its
 * file, position, code and message) is written there as `<member>.json`.
 */
export const ROWS_VARIABLE = "VERTER_PARITY_ROWS";

/** The prefix of each published row report line on stderr. */
export const PUBLISHED_ROWS_PREFIX = "parity-rows ";

/**
 * The corpus members, each a directory of the corpus holding a
 * `tsconfig.json`, and how each is checked: `-b` builds a solution and its
 * references, `-p` checks one project.
 */
export const MEMBERS = [
  { name: "web", mode: "-b" },
  { name: "web-admin", mode: "-p" },
];

const usage = `Usage: ${CORPUS_VARIABLE}=<directory> pnpm run parity:measure
   or: ${CORPUS_VARIABLE}=<directory> node scripts/sfc-projection/parity/measure.mjs [--publish-rows]

Measures verter-tsc's projection route against the stock vue-tsc check over the
${MEMBERS.map((member) => member.name).join(" and ")} members of a corpus directory
that lives outside the Verter tree. Each member is a directory of the corpus
with a tsconfig.json; vue-tsc is found in the nearest node_modules/.bin above it.
The corpus must already be prepared (dependencies installed, generated types in place).

It builds a release verter-tsc with the projection-route-qualification feature,
runs scripts/sfc-projection/parity/run.mjs in stock mode for each member, and
prints one JSON report of aggregate rows only (per member, per code, per cause)
with verter-tsc's own wall time and peak memory. It never prints a file name,
a path or a message from a diagnostic.

Row-level output is private and opt-in. ${ROWS_VARIABLE}=<directory>, outside the
Verter tree, receives each member's full parity report as <member>.json.
--publish-rows also prints each report on stderr, one line per member starting
"${PUBLISHED_ROWS_PREFIX.trim()}", so the run's log carries them; without ${ROWS_VARIABLE} it uses
a temporary directory it removes afterwards. Stdout stays the aggregate report.

The engine is VERTER_TSGO_BIN when set, else the TypeScript native engine
installed in Verter's own node_modules.

Exit 0 means the measurement completed (differences are data, not failure);
exit 2 means it was refused or could not run.
`;

/** A measurement that cannot run. Its message never names a corpus path. */
export class Refusal extends Error {}

/**
 * The real path of `directory`, or of its nearest existing ancestor with the
 * rest of the path appended, so a link or a directory yet to be made compares
 * by where it actually lands.
 */
function landing(directory) {
  const rest = [];
  for (let current = path.resolve(directory); ; current = path.dirname(current)) {
    try {
      return path.join(realpathSync.native(current), ...rest.reverse());
    } catch {}
    if (path.dirname(current) === current) return path.resolve(directory);
    rest.push(path.basename(current));
  }
}

/** Whether `directory` lands inside the Verter tree at `repoRoot`. */
export function insideTree(directory, repoRoot = REPO_ROOT) {
  const relative = path.relative(landing(repoRoot), landing(directory));
  return !(relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative));
}

/**
 * Check the corpus directory and its members; return the member roots. A
 * corpus named through a link is measured at the directory the link points to
 * when it is located, so a link moved during the run cannot mix two corpora.
 */
export function locateCorpus(env, repoRoot = REPO_ROOT) {
  const corpus = env[CORPUS_VARIABLE];
  if (!corpus)
    throw new Refusal(
      `refusing to run: ${CORPUS_VARIABLE} is not set. Supply the corpus directory, ` +
        `which holds the ${MEMBERS.map((member) => member.name).join(" and ")} members, ` +
        "from outside the Verter tree.",
    );
  const root = path.resolve(corpus);
  if (!existsSync(root) || !statSync(root).isDirectory())
    throw new Refusal(
      `refusing to run: the corpus directory named by ${CORPUS_VARIABLE} is absent.`,
    );
  if (insideTree(root, repoRoot))
    throw new Refusal(
      `refusing to run: the corpus directory named by ${CORPUS_VARIABLE} is inside the Verter tree.`,
    );
  const real = realpathSync.native(root);
  const members = MEMBERS.map((member) => ({ ...member, root: path.join(real, member.name) }));
  for (const member of members)
    if (!existsSync(path.join(member.root, "tsconfig.json")))
      throw new Refusal(
        `refusing to run: the corpus has no ${member.name} member (a directory with a tsconfig.json).`,
      );
  return { root: real, members };
}

/**
 * The private row directory named by `ROWS_VARIABLE`, or null when the
 * option is off. Like the corpus, it must lie outside the Verter tree.
 */
export function locateRows(env, repoRoot = REPO_ROOT) {
  const rows = env[ROWS_VARIABLE];
  if (!rows) return null;
  const root = path.resolve(rows);
  if (insideTree(root, repoRoot))
    throw new Refusal(
      `refusing to run: the row directory named by ${ROWS_VARIABLE} is inside the Verter tree.`,
    );
  return root;
}

/** The nearest `node_modules/.bin/<name>` at or above `directory`. */
export function findBin(directory, name) {
  for (let dir = directory; ; dir = path.dirname(dir)) {
    const candidate = path.join(dir, "node_modules", ".bin", name);
    if (existsSync(candidate)) return candidate;
    if (path.dirname(dir) === dir) return null;
  }
}

/**
 * The TypeScript native engine installed in Verter's own node_modules: the
 * platform package the `typescript` package depends on, resolved from it.
 */
export function verterEngine(env, repoRoot = REPO_ROOT) {
  if (env.VERTER_TSGO_BIN) return env.VERTER_TSGO_BIN;
  const pkg = `@typescript/typescript-${process.platform}-${process.arch}`;
  const executable = process.platform === "win32" ? "tsc.exe" : "tsc";
  try {
    const fromRoot = createRequire(path.join(repoRoot, "package.json"));
    const fromTypescript = createRequire(fromRoot.resolve("typescript/package.json"));
    const found = path.join(
      path.dirname(fromTypescript.resolve(`${pkg}/package.json`)),
      "lib",
      executable,
    );
    if (existsSync(found)) return found;
  } catch {}
  throw new Refusal(
    "cannot run: no TypeScript native engine in Verter's node_modules; install the " +
      "dependencies (pnpm install --frozen-lockfile) or set VERTER_TSGO_BIN.",
  );
}

/**
 * Build release verter-tsc with the projection route and return its
 * executable, as cargo reports it. Cargo's progress goes to stderr.
 */
export function buildVerterTsc(repoRoot = REPO_ROOT, spawnCommand = spawn) {
  const args = [
    "build",
    "--release",
    "--locked",
    "-p",
    "verter_tsc",
    "--features",
    "projection-route-qualification",
    "--message-format=json-render-diagnostics",
  ];
  return new Promise((resolve, reject) => {
    const child = spawnCommand("cargo", args, {
      cwd: repoRoot,
      stdio: ["ignore", "pipe", "inherit"],
    });
    let pending = "";
    let executable = null;
    const read = (line) => {
      if (!line.startsWith("{")) return;
      const message = JSON.parse(line);
      if (
        message.reason === "compiler-artifact" &&
        message.target?.name === "verter-tsc" &&
        message.executable
      )
        executable = message.executable;
    };
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      const lines = (pending + chunk).split("\n");
      pending = lines.pop();
      lines.forEach(read);
    });
    child.on("error", reject);
    child.on("close", (exit, signal) => {
      read(pending);
      if (exit !== 0) reject(new Error(`cargo build exited ${exit ?? signal}`));
      else if (!executable) reject(new Error("cargo build reported no verter-tsc executable"));
      else resolve(executable);
    });
  });
}

/**
 * Peak resident memory of one process image, from `/proc/<pid>/status`:
 * `VmHWM` when the process runs `name`, else null (not started, a different
 * image before exec, or gone).
 */
export async function procPeak(pid, name) {
  try {
    const status = await readFile(`/proc/${pid}/status`, "utf8");
    const image = /^Name:\s*(.*)$/m.exec(status)?.[1];
    const peak = /^VmHWM:\s*(\d+)\s*kB$/m.exec(status)?.[1];
    // The kernel truncates a process name to 15 bytes.
    if (image !== name.slice(0, 15) || peak === undefined) return null;
    return Number(peak) * 1024;
  } catch {
    return null;
  }
}

/**
 * A spawn that records each started process's wall time and, for the
 * process whose full argv is `watched`, its own peak memory, sampled every
 * `intervalMs` until it exits. Only that process is sampled: the engine
 * processes it starts are not counted.
 */
export function measuringSpawn({
  watched,
  spawnCommand = spawn,
  samplePeak = procPeak,
  now = () => performance.now(),
  intervalMs = 10,
}) {
  const records = [];
  const spawnMeasured = (command, args, options) => {
    const started = now();
    const child = spawnCommand(command, args, options);
    const record = {
      watched: JSON.stringify([command, ...args]) === JSON.stringify(watched),
      wallMs: null,
      peakBytes: null,
    };
    let sampling = Promise.resolve();
    let timer = null;
    const sample = () => {
      sampling = sampling.then(async () => {
        if (child.pid === undefined) return;
        const peak = await samplePeak(child.pid, path.basename(command));
        if (peak !== null) record.peakBytes = Math.max(record.peakBytes ?? 0, peak);
      });
    };
    if (record.watched) {
      sample();
      timer = setInterval(sample, intervalMs);
    }
    record.done = new Promise((resolve) => {
      const finish = () => {
        if (timer) clearInterval(timer);
        sampling.then(resolve);
      };
      child.on("exit", () => {
        record.wallMs = now() - started;
        finish();
      });
      child.on("error", finish);
    });
    records.push(record);
    return child;
  };
  return { spawnMeasured, records };
}

/**
 * Aggregate one member's parity report: counts per code and cause, where the
 * cause is the comparator's class (`right-only`: verter-tsc reports it and
 * the stock check does not; `left-only`: the reverse). No row's file, line or
 * message survives.
 */
export function aggregate(member, report) {
  const counts = new Map();
  for (const delta of report.differences) {
    if (delta.class !== "right-only" && delta.class !== "left-only") continue;
    const code = (delta.right[0] ?? delta.left[0]).code;
    const key = JSON.stringify([code, delta.class]);
    counts.set(key, (counts.get(key) ?? 0) + delta.count);
  }
  return [...counts]
    .map(([key, count]) => {
      const [code, cause] = JSON.parse(key);
      return { member, code, cause, count };
    })
    .sort(
      (left, right) =>
        left.cause.localeCompare(right.cause) ||
        right.count - left.count ||
        left.code.localeCompare(right.code),
    );
}

/**
 * The stock and verter-tsc argv for one member, relative to its directory.
 * `verterTsc` and `stockCheck` are argv prefixes.
 */
export function memberCommands(member, { verterTsc, stockCheck, engine }) {
  const project = [member.mode, "tsconfig.json", ...(member.mode === "-p" ? ["--noEmit"] : [])];
  return {
    stock: [...stockCheck, ...project, "--pretty", "false"],
    verter: [...verterTsc, ...project, "--projection-route", "--tsgo-bin", engine],
  };
}

/**
 * Measure every member in turn and return the aggregate report. The
 * verter-tsc process is the one whose wall time and peak memory are
 * reported; the stock check's wall time is reported beside it. With
 * `rowsDirectory`, each member's full parity report is written there as
 * `<member>.json`; nothing row-level reaches the returned report.
 */
export async function measure({
  members,
  verterTsc,
  engine,
  stockCheckFor,
  spawnCommand = spawn,
  samplePeak = procPeak,
  now,
  intervalMs,
  rowsDirectory = null,
}) {
  if (rowsDirectory) await mkdir(rowsDirectory, { recursive: true });
  const summaries = [];
  const rows = [];
  for (const member of members) {
    const stockCheck = stockCheckFor(member);
    const commands = memberCommands(member, { verterTsc, stockCheck, engine });
    const { spawnMeasured, records } = measuringSpawn({
      watched: commands.verter,
      spawnCommand,
      samplePeak,
      now,
      intervalMs,
    });
    const report = await run(
      [
        "--mode",
        "stock",
        "--left-command",
        JSON.stringify(commands.stock),
        "--left-root",
        member.root,
        "--right-command",
        JSON.stringify(commands.verter),
        "--right-root",
        member.root,
      ],
      { spawnCommand: spawnMeasured },
    );
    await Promise.all(records.map((record) => record.done));
    if (rowsDirectory)
      await writeFile(
        path.join(rowsDirectory, `${member.name}.json`),
        `${JSON.stringify(report, null, 2)}
`,
      );
    const verter = records.find((record) => record.watched);
    const stock = records.find((record) => !record.watched);
    if (verter.peakBytes === null)
      throw new Error(`verter-tsc's peak memory was not observed for the ${member.name} member`);
    summaries.push({
      member: member.name,
      pass: report.pass,
      gates: report.gates,
      stock: { rows: report.left.rows.length, exit: report.left.exit, wallMs: round(stock.wallMs) },
      verter: {
        rows: report.right.rows.length,
        exit: report.right.exit,
        wallMs: round(verter.wallMs),
        peakBytes: verter.peakBytes,
      },
    });
    rows.push(...aggregate(member.name, report));
  }
  return { route: "projection", against: "stock", members: summaries, rows };
}

const round = (ms) => Math.round(ms);

/**
 * The paths a failure message must not reveal: the corpus root, the variable's
 * raw value, and the directory each stock checker was found under. A checker
 * hoisted above the corpus root (`<dir>/node_modules/.bin/vue-tsc`) names a
 * private directory the corpus root does not cover.
 */
export function privatePaths(corpusRoot, variable, bins = []) {
  const owners = bins.map((bin) => path.dirname(path.dirname(path.dirname(bin))));
  return [corpusRoot, variable, ...owners];
}

/** Replace every occurrence of a private path in a message. */
export function redact(message, paths) {
  return paths
    .filter(Boolean)
    .sort((left, right) => right.length - left.length)
    .reduce((text, value) => text.replaceAll(value, "<corpus>"), message);
}

/**
 * Run the measurement for `env` and the command-line `args` and return the
 * exit status. `io` stands in for the release build (`verterTsc`, an argv
 * prefix), the installed stock checks (`stockCheckFor`), the peak sampler and
 * the output streams.
 */
export async function main(env, args = [], io = {}) {
  const {
    repoRoot = REPO_ROOT,
    stdout = process.stdout,
    stderr = process.stderr,
    verterTsc = null,
    stockCheckFor = null,
    samplePeak,
    spawnCommand,
  } = io;
  const publish = args.includes("--publish-rows");
  let corpus = null;
  let temporary = null;
  const bins = [];
  try {
    corpus = locateCorpus(env, repoRoot);
    let rowsDirectory = locateRows(env, repoRoot);
    const engine = verterEngine(env, repoRoot);
    const stockChecks = new Map();
    if (!stockCheckFor)
      for (const member of corpus.members) {
        const bin = findBin(member.root, "vue-tsc");
        if (!bin)
          throw new Refusal(
            `refusing to run: no vue-tsc is installed for the ${member.name} member; prepare the corpus first.`,
          );
        bins.push(bin);
        stockChecks.set(member.name, [bin]);
      }
    if (publish && !rowsDirectory) {
      temporary = await mkdtemp(path.join(os.tmpdir(), "verter-parity-rows-"));
      if (insideTree(temporary, repoRoot))
        throw new Refusal("refusing to run: the temporary directory is inside the Verter tree.");
      rowsDirectory = temporary;
    }
    const report = await measure({
      members: corpus.members,
      verterTsc: verterTsc ?? [await buildVerterTsc(repoRoot)],
      engine,
      stockCheckFor: stockCheckFor ?? ((member) => stockChecks.get(member.name)),
      samplePeak,
      spawnCommand,
      rowsDirectory,
    });
    stdout.write(`${JSON.stringify(report, null, 2)}
`);
    if (publish)
      for (const member of corpus.members) {
        const rows = JSON.parse(
          await readFile(path.join(rowsDirectory, `${member.name}.json`), "utf8"),
        );
        stderr.write(`${PUBLISHED_ROWS_PREFIX}${JSON.stringify({ member: member.name, report: rows })}
`);
      }
    return 0;
  } catch (error) {
    const paths = privatePaths(corpus?.root, env[CORPUS_VARIABLE], bins);
    stderr.write(`measure: ${redact(error.message, paths)}
`);
    return 2;
  } finally {
    if (temporary) await rm(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === "--help") process.stdout.write(usage);
  else if (args.length > 1 || (args.length === 1 && args[0] !== "--publish-rows")) {
    process.stderr.write(usage);
    process.exitCode = 2;
  } else main(process.env, args).then((code) => (process.exitCode = code));
}
