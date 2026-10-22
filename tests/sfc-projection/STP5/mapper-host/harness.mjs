/**
 * STP5 executed mapper-host harness.
 *
 * Materializes the fixture content mapper and a fixture project into a fresh
 * temporary directory per run, runs the selected engine's own `tsc` with
 * `--runExternalCode -p`, and returns the CLI diagnostics together with the
 * requests the fixture mapper received. Nothing is fetched: the mapper is
 * this directory's `fixture-mapper/`, spawned with the current Node.
 */

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const FIXTURE_MAPPER_DIR = path.join(HERE, "fixture-mapper");
export const FIXTURE_PACKAGE = "stp5-fixture-mapper";

/**
 * The `.stp5` source. Built here rather than committed so CRLF line endings
 * and the astral character survive any checkout's line-ending policy: the
 * emoji sits before a type error on the same line, so a mapper that emitted
 * UTF-16 positions under UTF-8 (or the reverse) moves that error.
 */
export const STP5_SOURCE = [
  "# 😀 STP5 content-mapper fixture",
  "let ok: number = 1",
  'let e: string = "😀"; let bad: number = e',
  "@expect",
  'let expected: number = "two"',
  "@expect",
  "let unused: number = 3",
  "@ignore",
  'let ignored: number = "four"',
  "call +",
  "export { ok, e, bad, expected, unused, ignored }",
  "",
].join("\r\n");

/**
 * The parse-diagnostic twin: one unrecognized statement the mapper reports
 * itself. A file with mapper parse diagnostics is not semantically checked
 * (as with any syntax error), so this is a separate run.
 */
export const STP5_PARSE_ERROR_SOURCE = STP5_SOURCE.replace(
  "call +\r\n",
  "call +\r\n!broken statement\r\n",
);

export const MAIN_SOURCE =
  'import { ok } from "./App.stp5";\n\nexport const fromMapped: string = ok;\n';

export const MAPPER_OPTIONS = Object.freeze({ plugins: [{ name: "ok" }] });

/**
 * Options the fixture mapper rejects through `optionDiagnostics`. Option
 * diagnostics, like any config error, stop tsc before semantic checking, so
 * this is a separate run.
 */
export const INVALID_MAPPER_OPTIONS = Object.freeze({ plugins: [{ name: "ok" }, { name: true }] });

/** 1-based line/column of `needle` in the original source, in UTF-16 units. */
export function lineColumnOf(text, needle, from = 0) {
  const index = text.indexOf(needle, from);
  if (index < 0) throw new Error(`needle not found: ${needle}`);
  const before = text.slice(0, index);
  const lines = before.split(/\r\n|\n/);
  return { line: lines.length, column: lines[lines.length - 1].length + 1 };
}

function tsconfigText(options, compilerOverrides = {}) {
  return `${JSON.stringify(
    {
      compilerOptions: {
        strict: true,
        noEmit: true,
        target: "ES2022",
        module: "ESNext",
        moduleResolution: "bundler",
        skipLibCheck: true,
        types: [],
        lib: ["ES2022"],
        ...compilerOverrides,
      },
      contentMappers: [
        {
          package: FIXTURE_PACKAGE,
          extensions: [".stp5"],
          ...(options === undefined ? {} : { options }),
        },
      ],
      include: ["./*.ts", "./*.stp5"],
    },
    null,
    2,
  )}\n`;
}

/**
 * Write one hermetic project: tsconfig, the `.stp5` source, a TypeScript
 * importer, the mapper's dynamic config file, and the fixture mapper package
 * under `node_modules/` with its manifest rewritten for this run.
 */
export function materializeMapperProject({
  encoding = "utf-16",
  twin = null,
  dynamicConfig = true,
  options = MAPPER_OPTIONS,
  source = STP5_SOURCE,
  mainSource = MAIN_SOURCE,
  compilerOverrides = {},
} = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stp5-mapper-host-"));
  const pkgDir = path.join(dir, "node_modules", FIXTURE_PACKAGE);
  fs.mkdirSync(pkgDir, { recursive: true });
  fs.copyFileSync(path.join(FIXTURE_MAPPER_DIR, "server.mjs"), path.join(pkgDir, "server.mjs"));
  const manifest = JSON.parse(
    fs.readFileSync(path.join(FIXTURE_MAPPER_DIR, "package.json"), "utf8"),
  );
  const logFile = path.join(dir, "mapper-requests.jsonl");
  const exec = [process.execPath, "server.mjs", `--encoding=${encoding}`, `--log=${logFile}`];
  if (twin) exec.push(`--twin=${twin}`);
  manifest.typescript.contentMapper.exec = exec;
  manifest.typescript.contentMapper.dynamicConfig = dynamicConfig;
  fs.writeFileSync(path.join(pkgDir, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  fs.writeFileSync(path.join(dir, "tsconfig.json"), tsconfigText(options, compilerOverrides));
  fs.writeFileSync(path.join(dir, "App.stp5"), source);
  fs.writeFileSync(path.join(dir, "main.ts"), mainSource);
  fs.writeFileSync(path.join(dir, "stp5.config.json"), '{ "flavor": "fixture" }\n');
  return {
    dir,
    logFile,
    /** Remove the project. Never throws; returns the failure text, if any. */
    cleanup() {
      try {
        fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
        return null;
      } catch (error) {
        return `could not remove ${dir}: ${error?.message || error}`;
      }
    },
  };
}

/**
 * The command that runs `engine`'s tsc with `args`: the native executable
 * itself, or the JS compiler under the current Node.
 */
export function tscCommand(engine, args) {
  return engine.kind === "native"
    ? { command: engine.executable, args }
    : { command: process.execPath, args: [engine.executable, ...args] };
}

/**
 * Every run pins its message language: `--locale en` on the command line
 * (unless the run is about locale) and a C locale in the environment, so a
 * machine with another UI language cannot change the English messages and
 * `initialize.locale` the proof reads.
 */
export const DEFAULT_LOCALE = "en";
export const RUN_ENV = Object.freeze({ ...process.env, LANG: "C", LC_ALL: "C" });

export function withLocale(args, locale = DEFAULT_LOCALE) {
  return args.includes("--locale") ? args : [...args, "--locale", locale];
}

/**
 * `file(line,col): error CODE: message` — `tsc --pretty false` lines. CODE is
 * `TS1234` for TypeScript and `<diagnosticSource>1234` for mapper-authored
 * diagnostics.
 */
export function parseTscDiagnostics(output) {
  const rows = [];
  const re = /^(.+?)\((\d+),(\d+)\): (error|warning|message) ([A-Za-z][\w-]*?)(\d+): (.*)$/;
  for (const line of String(output).split(/\r?\n/)) {
    const match = re.exec(line);
    if (!match) {
      // Indented continuation lines carry a diagnostic's message chain.
      if (rows.length > 0 && /^\s{2,}\S/.test(line)) {
        const last = rows[rows.length - 1];
        last.detail = last.detail ? `${last.detail}\n${line.trim()}` : line.trim();
      }
      continue;
    }
    rows.push({
      file: path.basename(match[1]),
      line: Number(match[2]),
      column: Number(match[3]),
      category: match[4],
      source: match[5],
      code: Number(match[6]),
      message: match[7],
    });
  }
  return rows;
}

/** Every line the fixture logged: requests (`method`) and its replies (`response`). */
export function readMapperLog(logFile) {
  if (!fs.existsSync(logFile)) return [];
  return fs
    .readFileSync(logFile, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

/** The requests TypeScript sent, in order. */
export function readRequestLog(logFile) {
  return readMapperLog(logFile).filter((entry) => entry.method !== undefined);
}

/** The replies the fixture sent, in order. */
export function readResponseLog(logFile) {
  return readMapperLog(logFile).filter((entry) => entry.response !== undefined);
}

/**
 * Run a command to completion without blocking the event loop, so
 * independent runs proceed concurrently. A run past `timeoutMs` is killed;
 * a spawn failure resolves with `error` instead of throwing.
 */
export function runCommand(command, args, { cwd, timeoutMs = 60000 } = {}) {
  return new Promise((resolve) => {
    let output = "";
    let settled = false;
    const child = spawn(command, args, { cwd, env: RUN_ENV, windowsHide: true });
    const finish = (status, error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ status, error, output });
    };
    const timer = setTimeout(() => {
      killTree(child);
      finish(null, `timed out after ${timeoutMs} ms`);
    }, timeoutMs);
    child.stdout.on("data", (chunk) => (output += chunk));
    child.stderr.on("data", (chunk) => (output += chunk));
    child.on("error", (error) => finish(null, String(error?.message || error)));
    child.on("close", (status) => finish(status, null));
  });
}

/**
 * Kill a process and its descendants. On Windows a plain kill leaves the
 * mapper (tsc's child) running and holding the project directory open.
 */
export function killTree(child) {
  if (!child.pid || child.exitCode !== null) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { windowsHide: true });
  } else {
    child.kill("SIGKILL");
  }
}

/** Run the engine's own tsc over one materialized project. */
export async function runTscWithMapper(engine, project, extraArgs = []) {
  const { command, args } = tscCommand(
    engine,
    withLocale([
      "--runExternalCode",
      "-p",
      path.join(project.dir, "tsconfig.json"),
      "--pretty",
      "false",
      ...extraArgs,
    ]),
  );
  const run = await runCommand(command, args, { cwd: project.dir });
  return {
    status: run.status,
    error: run.error,
    output: run.output,
    diagnostics: parseTscDiagnostics(run.output),
    requests: readRequestLog(project.logFile),
    responses: readResponseLog(project.logFile),
  };
}

/**
 * Materialize, run, observe, clean up. A cleanup failure never replaces the
 * observation; it is returned beside it.
 */
export async function withProject(engine, settings, extraArgs, fn = (run) => run) {
  const project = materializeMapperProject(settings);
  let observed;
  try {
    observed = await fn(await runTscWithMapper(engine, project, extraArgs), project);
  } finally {
    const cleanupError = project.cleanup();
    if (observed && typeof observed === "object") observed.cleanupError = cleanupError;
  }
  return observed;
}
