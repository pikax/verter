/**
 * STP5 executed content-mapper host proof.
 *
 * Runs the selected engine's own `tsc --runExternalCode` against the fixture
 * mapper and records, per protocol operation, what that build actually did.
 * Each operation is `supported` only when its run produced the observable the
 * final protocol (microsoft/typescript-go#4712 + microsoft/TypeScript#63936)
 * requires, and every reject twin discriminates: a non-conforming reply is
 * refused with TS18069 and its reason.
 *
 * Called only after the engine's mapper-host probe found a host. Without a
 * host there is nothing to execute, and the caller records
 * "no mapper host on <engine>@<version>" instead.
 *
 * The independent `-p` runs execute concurrently; the `-b` sequence and the
 * `--watch` session run beside them. One engine build is proven once per
 * process: repeated calls share the first run's result.
 */

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { MAPPER_HOST_OPERATIONS } from "../protocol.mjs";
import {
  DIAGNOSTIC_SOURCE,
  MAPPER_PARSE_CODE,
  OPTION_DIAGNOSTIC_CODE,
  UNUSED_EXPECT_CODE,
} from "./fixture-mapper/server.mjs";
import {
  INVALID_MAPPER_OPTIONS,
  MAPPER_OPTIONS,
  RUN_ENV,
  STP5_PARSE_ERROR_SOURCE,
  STP5_SOURCE,
  killTree,
  lineColumnOf,
  materializeMapperProject,
  readRequestLog,
  runCommand,
  tscCommand,
  withLocale,
  withProject,
} from "./harness.mjs";

export { MAPPER_HOST_OPERATIONS };

const MAPPER_FAILURE_CODE = 18069;
/** Upper bounds only: every wait returns as soon as its condition holds. */
const WATCH_TIMEOUT_MS = 30000;
const WATCH_EXIT_TIMEOUT_MS = 5000;
/** How long an unwatched change is given to (wrongly) re-open the project. */
const UNWATCHED_SETTLE_MS = 1500;
/** The reason 7.1 prints for any failure while opening the mapper project. */
const PROJECT_REQUEST_FAILURE = /process failed while handling the project request/;

/** Same file, by real path; case-insensitive only where the file system is. */
export function sameFile(a, b) {
  const real = (value) => {
    try {
      return fs.realpathSync.native(value);
    } catch {
      return path.resolve(value);
    }
  };
  const left = real(String(a || ""));
  const right = real(String(b || ""));
  return process.platform === "win32" ? left.toLowerCase() === right.toLowerCase() : left === right;
}

function key(row) {
  return `${row.file}:${row.line}:${row.column}:${row.source}${row.code}`;
}

function at(file, source, code, position) {
  return { file, source, code, line: position.line, column: position.column };
}

function has(rows, expected) {
  return rows.some(
    (row) =>
      row.file === expected.file &&
      row.source === expected.source &&
      row.code === expected.code &&
      row.line === expected.line &&
      row.column === expected.column,
  );
}

function rejected(run, detailPattern) {
  return run.diagnostics.some(
    (row) =>
      row.source === "TS" &&
      row.code === MAPPER_FAILURE_CODE &&
      detailPattern.test(row.detail || ""),
  );
}

function methods(run) {
  return run.requests.map((request) => request.method);
}

function brief(run) {
  return (
    run.diagnostics.map((row) => `${key(row)}${row.detail ? ` (${row.detail})` : ""}`).join("; ") ||
    run.error ||
    run.output.trim()
  );
}

function initializeReply(run) {
  return run.responses.find((entry) => entry.response === "initialize");
}

/**
 * The host refused a reply the fixture actually sent: TS18069 with the
 * project-request reason, the fixture's `initialize` reply carrying the
 * offending value, and no further request. A crash never logs the reply, so
 * it cannot pass for a refusal (see the `exit-on-initialize` control).
 */
function refusedInitializeReply(run, offending) {
  const reply = initializeReply(run);
  return (
    rejected(run, PROJECT_REQUEST_FAILURE) &&
    Boolean(reply?.result) &&
    offending(reply.result) &&
    !methods(run).includes("openProject")
  );
}

/** Expected diagnostics of the main run, in original-file coordinates. */
export function expectedMainDiagnostics(source = STP5_SOURCE) {
  const secondExpect = source.indexOf("@expect", source.indexOf("@expect") + 1);
  return {
    present: [
      at("App.stp5", "TS", 2322, lineColumnOf(source, "bad")),
      at(
        "App.stp5",
        DIAGNOSTIC_SOURCE,
        UNUSED_EXPECT_CODE,
        lineColumnOf(source, "@expect", secondExpect),
      ),
      at("App.stp5", "TS", 2304, lineColumnOf(source, "+")),
      at("main.ts", "TS", 2322, { line: 3, column: 14 }),
    ],
    suppressedLines: [lineColumnOf(source, "expected").line, lineColumnOf(source, "ignored").line],
  };
}

async function runBuildMode(engine) {
  const project = materializeMapperProject({
    source: "let ok: number = 1\r\nexport { ok }\r\n",
    mainSource: 'import { ok } from "./App.stp5";\nexport const n: number = ok;\n',
    compilerOverrides: { noEmit: false, outDir: "out" },
  });
  const build = async () => {
    fs.writeFileSync(project.logFile, "");
    const { command, args } = tscCommand(
      engine,
      withLocale([
        "-b",
        path.join(project.dir, "tsconfig.json"),
        "--runExternalCode",
        "--verbose",
        "--pretty",
        "false",
      ]),
    );
    const run = await runCommand(command, args, { cwd: project.dir });
    return {
      status: run.status,
      error: run.error,
      upToDate: /is up to date/.test(run.output),
      methods: readRequestLog(project.logFile).map((request) => request.method),
    };
  };
  let result;
  try {
    const first = await build();
    const unchanged = await build();
    fs.writeFileSync(path.join(project.dir, "stp5.config.json"), '{ "flavor": "changed" }\n');
    const changed = await build();
    const settled = await build();
    result = { first, unchanged, changed, settled };
  } finally {
    const cleanupError = project.cleanup();
    if (result) result.cleanupError = cleanupError;
  }
  return result;
}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * One `--watch` session. First an unwatched file changes, and the host gets
 * a settle window in which it must NOT re-open the mapper project (whether
 * it rebuilds at all depends on the platform's directory watcher, so no
 * rebuild is required). Then the watched dynamic-config file changes, and
 * the host must close and re-open the project.
 */
async function runWatchSession(engine) {
  const project = materializeMapperProject({});
  const { command, args } = tscCommand(
    engine,
    withLocale([
      "--runExternalCode",
      "-p",
      path.join(project.dir, "tsconfig.json"),
      "--watch",
      "--pretty",
      "false",
    ]),
  );
  const result = { started: false, unwatchedAfter: [], watchedAfter: [], reopened: false };
  let output = "";
  let spawnError = null;
  const child = spawn(command, args, { cwd: project.dir, env: RUN_ENV, windowsHide: true });
  const exited = new Promise((resolve) => {
    child.on("close", resolve);
    child.on("error", (error) => {
      spawnError = String(error?.message || error);
      resolve(null);
    });
  });
  child.stdout.on("data", (chunk) => (output += chunk));
  child.stderr.on("data", (chunk) => (output += chunk));
  const read = () => readRequestLog(project.logFile).map((request) => request.method);
  const waitFor = async (predicate, timeoutMs = WATCH_TIMEOUT_MS) => {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (spawnError) return false;
      if (predicate()) return true;
      await delay(50);
    }
    return false;
  };
  const watching = () => (output.match(/Watching for file changes/g) || []).length;
  try {
    result.started = await waitFor(() => watching() >= 1 && read().includes("transform"));
    if (result.started) {
      const beforeUnwatched = read().length;
      const cycles = watching();
      fs.writeFileSync(path.join(project.dir, "unwatched.json"), '{ "flavor": "changed" }\n');
      await waitFor(
        () => watching() > cycles || read().slice(beforeUnwatched).includes("openProject"),
        UNWATCHED_SETTLE_MS,
      );
      result.unwatchedAfter = read().slice(beforeUnwatched);
      const beforeWatched = read().length;
      fs.writeFileSync(path.join(project.dir, "stp5.config.json"), '{ "flavor": "changed" }\n');
      result.reopened = await waitFor(() => {
        const after = read().slice(beforeWatched);
        const close = after.indexOf("closeProject");
        return (
          close >= 0 && after.indexOf("openProject", close) > close && after.includes("transform")
        );
      });
      result.watchedAfter = read().slice(beforeWatched);
    }
  } finally {
    killTree(child);
    const exit = await Promise.race([exited, delay(WATCH_EXIT_TIMEOUT_MS).then(() => "timeout")]);
    if (exit === "timeout") {
      killTree(child);
      result.exitTimeout = true;
    }
    result.cleanupError = project.cleanup();
  }
  result.error = spawnError;
  return result;
}

/** Run `tasks` (functions returning promises) with at most `limit` in flight. */
async function pool(tasks, limit) {
  const results = new Array(tasks.length);
  let next = 0;
  const worker = async () => {
    while (next < tasks.length) {
      const index = next++;
      results[index] = await tasks[index]();
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, tasks.length) }, worker));
  return results;
}

const CONCURRENCY = Math.max(2, Math.min(8, os.availableParallelism?.() ?? os.cpus().length));

const cache = new Map();

/**
 * Execute every run against `engine` (a resolved verify-node engine:
 * `{ id, version, kind, executable, sha256? }`) and return one record per
 * operation. Memoized per executable build within the process.
 */
export function runMapperHostEndToEnd(engine) {
  const cacheKey = `${path.resolve(engine.executable)}|${engine.sha256 ?? engine.version}`;
  if (!cache.has(cacheKey)) cache.set(cacheKey, executeEndToEnd(engine));
  return cache.get(cacheKey);
}

async function executeEndToEnd(engine) {
  const startedAt = Date.now();
  const outcomes = new Map();
  // An operation is supported only when every run that exercised it passed;
  // its detail keeps each run's observation.
  const record = (operation, ok, detail) => {
    const previous = outcomes.get(operation);
    outcomes.set(operation, {
      operation,
      ok: Boolean(ok) && (previous ? previous.ok : true),
      detail: previous ? `${previous.detail} | ${detail}` : detail,
    });
  };

  const expected = expectedMainDiagnostics();
  const run =
    (settings, extraArgs = [], fn) =>
    () =>
      withProject(engine, settings, extraArgs, fn);
  const [
    [
      utf16,
      utf8,
      bare,
      ambiguous,
      atom,
      synthesized,
      parse,
      optionRun,
      staticRun,
      missingIdentity,
      staticIdentity,
      staticWatched,
      reserved,
      unoffered,
      crashed,
      noExtension,
    ],
    build,
    watch,
  ] = await Promise.all([
    pool(
      [
        // The real-path comparisons need the project, so they run before
        // its cleanup.
        run({ encoding: "utf-16" }, [], (result, project) => ({
          ...result,
          sameConfig: sameFile(
            result.requests.find((request) => request.method === "openProject")?.params
              ?.configFileName ?? "",
            path.join(project.dir, "tsconfig.json"),
          ),
          sameSource: sameFile(
            result.requests.find((request) => request.method === "transform")?.params?.fileName ??
              "",
            path.join(project.dir, "App.stp5"),
          ),
        })),
        run({ encoding: "utf-8" }),
        run({ source: STP5_SOURCE.replace(/@expect/g, "#expect").replace(/@ignore/g, "#ignore") }),
        run({ twin: "ambiguous-unused-index" }),
        run({ twin: "atom-call" }),
        run({ twin: "synthesized-error" }),
        run({ source: STP5_PARSE_ERROR_SOURCE }),
        run({ options: INVALID_MAPPER_OPTIONS }, [], (result, project) => {
          const text = fs.readFileSync(path.join(project.dir, "tsconfig.json"), "utf8");
          const index = text.indexOf('"name": true') + '"name": '.length;
          return { ...result, expectedAt: lineColumnOf(text, "true", index) };
        }),
        run({ twin: "static-config", dynamicConfig: false }, ["--locale", "fr"]),
        run({ twin: "omit-config-identity" }),
        run({ dynamicConfig: false }),
        run({ twin: "omit-config-identity", dynamicConfig: false }),
        run({ twin: "reserved-diagnostic-source" }),
        run({ twin: "unoffered-encoding" }),
        run({ twin: "exit-on-initialize" }),
        run({ twin: "omit-extension" }),
      ],
      CONCURRENCY,
    ),
    runBuildMode(engine),
    runWatchSession(engine),
  ]);

  const init = utf16.requests.find((request) => request.method === "initialize");
  const offered = init?.params?.positionEncodings || [];
  record(
    "initialize",
    init &&
      offered.includes("utf-8") &&
      offered.includes("utf-16") &&
      methods(utf16)[0] === "initialize",
    init ? `offered ${JSON.stringify(offered)}` : `no initialize request; ${brief(utf16)}`,
  );

  const utf16Keys = utf16.diagnostics.map(key).sort();
  const utf8Keys = utf8.diagnostics.map(key).sort();
  const negotiated16 = initializeReply(utf16)?.result?.positionEncoding;
  const negotiated8 = initializeReply(utf8)?.result?.positionEncoding;
  const allPresent = expected.present.every((row) => has(utf16.diagnostics, row));
  record(
    "encoding-negotiation",
    negotiated16 === "utf-16" &&
      negotiated8 === "utf-8" &&
      allPresent &&
      JSON.stringify(utf16Keys) === JSON.stringify(utf8Keys),
    `negotiated ${negotiated16} / ${negotiated8}; utf-16 [${utf16Keys.join(", ")}] utf-8 [${utf8Keys.join(", ")}]`,
  );

  const open = utf16.requests.find((request) => request.method === "openProject");
  record(
    "openProject",
    open &&
      typeof open.params?.projectHandle === "string" &&
      open.params.projectHandle.length > 0 &&
      utf16.sameConfig &&
      JSON.stringify(open.params.options) === JSON.stringify(MAPPER_OPTIONS) &&
      open.params.compilerOptions?.strict === true,
    open
      ? `handle ${open.params?.projectHandle}; configFileName is the tsconfig ${utf16.sameConfig}; options ${JSON.stringify(open.params?.options)}; compilerOptions.strict ${open.params?.compilerOptions?.strict}`
      : `no openProject request; ${brief(utf16)}`,
  );

  const transform = utf16.requests.find((request) => request.method === "transform");
  record(
    "transform",
    transform &&
      transform.params?.content === STP5_SOURCE &&
      transform.params?.projectHandle === open?.params?.projectHandle &&
      utf16.sameSource &&
      has(utf16.diagnostics, expected.present[3]),
    transform
      ? `fileName is App.stp5 ${utf16.sameSource}; full content ${transform.params?.content === STP5_SOURCE}; cross-file type error ${has(utf16.diagnostics, expected.present[3])}`
      : `no transform request; ${brief(utf16)}`,
  );

  record(
    "span-mappings",
    has(utf16.diagnostics, expected.present[0]),
    `TS2322 expected at App.stp5(${expected.present[0].line},${expected.present[0].column}); got ${brief(utf16)}`,
  );

  const plus = utf16.diagnostics.find((row) => row.code === 2304);
  record(
    "alias-display",
    plus && has(utf16.diagnostics, expected.present[2]) && /'\+'/.test(plus.message),
    plus ? plus.message : `no TS2304; ${brief(utf16)}`,
  );
  const atomPlus = atom.diagnostics.find((row) => row.code === 2304);
  record(
    "alias-display",
    atomPlus && /'add_op'/.test(atomPlus.message),
    `Atom twin keeps the virtual name: ${atomPlus?.message ?? brief(atom)}`,
  );

  const unused = expected.present[1];
  record(
    "unused-expect-diagnostic",
    has(utf16.diagnostics, unused),
    `${DIAGNOSTIC_SOURCE}${UNUSED_EXPECT_CODE} expected at App.stp5(${unused.line},${unused.column}); got ${brief(utf16)}`,
  );
  record(
    "unused-expect-diagnostic",
    !bare.diagnostics.some((row) => row.code === UNUSED_EXPECT_CODE),
    "no unused-Expect diagnostic without the directive",
  );

  // Directive twin: the same lines with the directives commented out must
  // report what the directives suppressed.
  const leaked = utf16.diagnostics.filter(
    (row) => row.file === "App.stp5" && expected.suppressedLines.includes(row.line),
  );
  const expectedLine = lineColumnOf(STP5_SOURCE, "expected");
  const ignoredLine = lineColumnOf(STP5_SOURCE, "ignored");
  const bareHas = (position) =>
    bare.diagnostics.some(
      (row) => row.file === "App.stp5" && row.code === 2322 && row.line === position.line,
    );
  record(
    "diagnostic-directive-expect",
    leaked.every((row) => row.line !== expectedLine.line) && bareHas(expectedLine),
    `suppressed with @expect, reported without it: ${bareHas(expectedLine)}`,
  );
  record(
    "diagnostic-directive-ignore",
    leaked.every((row) => row.line !== ignoredLine.line) && bareHas(ignoredLine),
    `suppressed with @ignore, reported without it: ${bareHas(ignoredLine)}`,
  );

  record(
    "unused-expect-index",
    rejected(ambiguous, /unusedExpectDirectiveIndex/),
    `explicit index 0 accepted in the main run; omitted index with two unused diagnostics: ${brief(ambiguous)}`,
  );

  const generated = synthesized.diagnostics.find((row) =>
    /virtual code produced by the content mapper/.test(row.detail || ""),
  );
  record(
    "synthesized-diagnostics",
    Boolean(generated),
    generated
      ? `${key(generated)}: ${generated.detail}`
      : `no generated-code diagnostic; ${brief(synthesized)}`,
  );

  const parseAt = at(
    "App.stp5",
    DIAGNOSTIC_SOURCE,
    MAPPER_PARSE_CODE,
    lineColumnOf(STP5_PARSE_ERROR_SOURCE, "!broken"),
  );
  record(
    "mapper-diagnostics",
    has(parse.diagnostics, parseAt),
    `${DIAGNOSTIC_SOURCE}${MAPPER_PARSE_CODE} expected at App.stp5(${parseAt.line},${parseAt.column}); got ${brief(parse)}`,
  );

  const optionAt = at(
    "tsconfig.json",
    DIAGNOSTIC_SOURCE,
    OPTION_DIAGNOSTIC_CODE,
    optionRun.expectedAt,
  );
  record(
    "option-diagnostics",
    has(optionRun.diagnostics, optionAt) &&
      !utf16.diagnostics.some((row) => row.code === OPTION_DIAGNOSTIC_CODE),
    `path ["plugins",1,"name"] expected at tsconfig.json(${optionAt.line},${optionAt.column}); got ${brief(optionRun)}`,
  );

  // Every run pins `--locale en`; the static-config run asks for French.
  const staticInit = staticRun.requests.find((request) => request.method === "initialize");
  record(
    "locale",
    staticInit?.params?.locale === "fr" && init?.params?.locale === "en",
    `--locale fr sent ${JSON.stringify(staticInit?.params?.locale)}; --locale en sent ${JSON.stringify(init?.params?.locale)}`,
  );

  // Static configuration is accepted; dynamic configuration without an
  // identity, and a static mapper returning an identity, are each refused;
  // `tsc -b` stays up to date until the dynamic configuration changes.
  const staticAccepted = expected.present.every((row) => has(staticRun.diagnostics, row));
  const missingRefused = rejected(missingIdentity, /did not return 'configIdentity'/);
  const staticIdentityRefused = rejected(staticIdentity, /returned 'configIdentity'/);
  record(
    "dynamic-config-identity",
    staticAccepted &&
      missingRefused &&
      staticIdentityRefused &&
      build.first.status === 0 &&
      build.first.methods.includes("transform") &&
      build.unchanged.upToDate &&
      !build.unchanged.methods.includes("transform") &&
      build.unchanged.methods.includes("openProject") &&
      !build.changed.upToDate &&
      build.changed.methods.includes("transform") &&
      build.settled.upToDate,
    `static accepted ${staticAccepted}; missing identity refused ${missingRefused}; static identity refused ${staticIdentityRefused}; -b unchanged up-to-date ${build.unchanged.upToDate} [${build.unchanged.methods}]; -b after config change rebuilt ${!build.changed.upToDate} [${build.changed.methods}]`,
  );

  const staticWatchedRefused = rejected(staticWatched, /returned 'watchedFiles'/);
  record(
    "watched-files",
    staticWatchedRefused &&
      watch.started &&
      !watch.unwatchedAfter.includes("openProject") &&
      watch.reopened,
    `static watchedFiles refused ${staticWatchedRefused}; --watch after unwatched change [${watch.unwatchedAfter}]; after watched-file change [${watch.watchedAfter}]${watch.error ? `; ${watch.error}` : ""}`,
  );

  // A crash logs no reply, so the control must NOT read as a refusal.
  const crashReadsAsRefusal = refusedInitializeReply(crashed, () => true);
  const reservedRefused = refusedInitializeReply(
    reserved,
    (reply) => reply.diagnosticSource === "tsc",
  );
  record(
    "diagnostic-source",
    utf16.diagnostics.some((row) => row.source === DIAGNOSTIC_SOURCE) &&
      reservedRefused &&
      !crashReadsAsRefusal,
    `mapper diagnostics carry '${DIAGNOSTIC_SOURCE}'; diagnosticSource "tsc" replied and refused ${reservedRefused}; crash control reads as refusal ${crashReadsAsRefusal}`,
  );
  const unofferedRefused = refusedInitializeReply(
    unoffered,
    (reply) => reply.positionEncoding === "utf-32",
  );
  record(
    "encoding-negotiation",
    unofferedRefused && !crashReadsAsRefusal,
    `positionEncoding "utf-32" replied and refused ${unofferedRefused}`,
  );

  record(
    "virtual-extension",
    rejected(noExtension, /unsupported virtual extension/) &&
      !utf16.diagnostics.some((row) => row.code === MAPPER_FAILURE_CODE),
    `".ts" accepted; omitted extension: ${brief(noExtension)}`,
  );

  const close = utf16.requests.find((request) => request.method === "closeProject");
  record(
    "closeProject",
    close &&
      close.params?.projectHandle === open?.params?.projectHandle &&
      methods(utf16).at(-1) === "closeProject",
    close
      ? `closeProject(${close.params?.projectHandle}) after transform in -p mode; -b mode requests [${build.first.methods}]`
      : `no closeProject request; requests [${methods(utf16)}]`,
  );

  const operations = MAPPER_HOST_OPERATIONS.map(
    (operation) => outcomes.get(operation) || { operation, ok: false, detail: "not exercised" },
  );
  const cleanupErrors = [
    utf16,
    utf8,
    bare,
    ambiguous,
    atom,
    synthesized,
    parse,
    optionRun,
    staticRun,
    missingIdentity,
    staticIdentity,
    staticWatched,
    reserved,
    unoffered,
    crashed,
    noExtension,
    build,
    watch,
  ]
    .map((row) => row.cleanupError)
    .filter(Boolean);
  return {
    performed: true,
    engineId: engine.id,
    engineVersion: engine.version,
    operations,
    unsupported: operations.filter((row) => !row.ok).map((row) => row.operation),
    cleanupErrors,
    wallMs: Date.now() - startedAt,
  };
}
