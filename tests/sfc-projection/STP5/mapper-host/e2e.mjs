/**
 * STP5 executed content-mapper host proof.
 *
 * Runs the selected engine's own `tsc --runExternalCode` against the fixture
 * mapper and records, per protocol operation, what that build actually did.
 * Each operation is `supported` only when its run produced the observable the
 * final protocol (microsoft/typescript-go#4712 + microsoft/TypeScript#63936)
 * requires, and every reject twin discriminates: a non-conforming reply is
 * refused with TS18069 and the specific reason.
 *
 * Called only after the engine's mapper-host probe found a host. Without a
 * host there is nothing to execute, and the caller records
 * "no mapper host on <engine>@<version>" instead.
 */

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  INVALID_MAPPER_OPTIONS,
  MAPPER_OPTIONS,
  STP5_PARSE_ERROR_SOURCE,
  STP5_SOURCE,
  lineColumnOf,
  materializeMapperProject,
  readRequestLog,
  withProject,
} from "./harness.mjs";
import {
  DIAGNOSTIC_SOURCE,
  MAPPER_PARSE_CODE,
  OPTION_DIAGNOSTIC_CODE,
  UNUSED_EXPECT_CODE,
} from "./fixture-mapper/server.mjs";

/**
 * The operations this proof records per build, in report order. Each maps to
 * a field, method or rule of the final protocol.
 */
export const MAPPER_HOST_OPERATIONS = Object.freeze([
  "initialize",
  "encoding-negotiation",
  "locale",
  "diagnostic-source",
  "openProject",
  "option-diagnostics",
  "dynamic-config-identity",
  "watched-files",
  "transform",
  "virtual-extension",
  "span-mappings",
  "alias-display",
  "synthesized-diagnostics",
  "mapper-diagnostics",
  "diagnostic-directive-ignore",
  "diagnostic-directive-expect",
  "unused-expect-diagnostic",
  "unused-expect-index",
  "closeProject",
]);

const MAPPER_FAILURE_CODE = 18069;
const WATCH_TIMEOUT_MS = 30000;

const normalizePath = (value) =>
  String(value || "")
    .replace(/\\/g, "/")
    .toLowerCase();

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
    run.output.trim()
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

function runBuildMode(engine) {
  const project = materializeMapperProject({
    source: "let ok: number = 1\r\nexport { ok }\r\n",
    mainSource: 'import { ok } from "./App.stp5";\nexport const n: number = ok;\n',
    compilerOverrides: { noEmit: false, outDir: "out" },
  });
  const build = () => {
    fs.writeFileSync(project.logFile, "");
    const args = [
      "-b",
      path.join(project.dir, "tsconfig.json"),
      "--runExternalCode",
      "--verbose",
      "--pretty",
      "false",
    ];
    const spawned =
      engine.kind === "native"
        ? spawnSync(engine.executable, args, {
            encoding: "utf8",
            windowsHide: true,
            timeout: 60000,
            cwd: project.dir,
          })
        : spawnSync(process.execPath, [engine.executable, ...args], {
            encoding: "utf8",
            windowsHide: true,
            timeout: 60000,
            cwd: project.dir,
          });
    const output = `${spawned.stdout || ""}${spawned.stderr || ""}`;
    return {
      status: spawned.status,
      upToDate: /is up to date/.test(output),
      methods: readRequestLog(project.logFile).map((request) => request.method),
    };
  };
  try {
    const first = build();
    const unchanged = build();
    fs.writeFileSync(path.join(project.dir, "stp5.config.json"), '{ "flavor": "changed" }\n');
    const changed = build();
    const settled = build();
    return { first, unchanged, changed, settled };
  } finally {
    project.cleanup();
  }
}

async function runWatchTouch(engine, touchedFile) {
  const project = materializeMapperProject({});
  const args = [
    "--runExternalCode",
    "-p",
    path.join(project.dir, "tsconfig.json"),
    "--watch",
    "--pretty",
    "false",
  ];
  const child =
    engine.kind === "native"
      ? spawn(engine.executable, args, { cwd: project.dir, windowsHide: true })
      : spawn(process.execPath, [engine.executable, ...args], {
          cwd: project.dir,
          windowsHide: true,
        });
  let output = "";
  child.stdout.on("data", (chunk) => (output += chunk));
  child.stderr.on("data", (chunk) => (output += chunk));
  const exited = new Promise((resolve) => child.on("exit", resolve));
  const read = () => readRequestLog(project.logFile).map((request) => request.method);
  const waitFor = async (predicate) => {
    const deadline = Date.now() + WATCH_TIMEOUT_MS;
    while (Date.now() < deadline) {
      if (predicate()) return true;
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    return false;
  };
  const watching = () => (output.match(/Watching for file changes/g) || []).length;
  try {
    const started = await waitFor(() => watching() >= 1 && read().includes("transform"));
    if (!started) return { started, rebuilt: false, after: [], output };
    const before = read().length;
    const cycles = watching();
    fs.writeFileSync(path.join(project.dir, touchedFile), '{ "flavor": "changed" }\n');
    const rebuilt = await waitFor(
      () => watching() > cycles && read().slice(before).includes("transform"),
    );
    return { started, rebuilt, after: read().slice(before), output };
  } finally {
    child.kill();
    await exited;
    project.cleanup();
  }
}

/**
 * Execute every run against `engine` (a resolved verify-node engine:
 * `{ id, version, kind, executable }`) and return one record per operation.
 */
export async function runMapperHostEndToEnd(engine) {
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

  // Main run (UTF-16) and its UTF-8 twin.
  const utf16 = withProject(engine, { encoding: "utf-16" }, [], (run, project) => ({
    ...run,
    project: { dir: project.dir },
  }));
  const utf8 = withProject(engine, { encoding: "utf-8" }, [], (run) => run);

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
  const allPresent = expected.present.every((row) => has(utf16.diagnostics, row));
  record(
    "encoding-negotiation",
    allPresent && JSON.stringify(utf16Keys) === JSON.stringify(utf8Keys),
    `utf-16 [${utf16Keys.join(", ")}] utf-8 [${utf8Keys.join(", ")}]`,
  );

  const open = utf16.requests.find((request) => request.method === "openProject");
  const openOk =
    open &&
    typeof open.params?.projectHandle === "string" &&
    open.params.projectHandle.length > 0 &&
    normalizePath(open.params.configFileName) ===
      normalizePath(path.join(utf16.project.dir, "tsconfig.json")) &&
    JSON.stringify(open.params.options) === JSON.stringify(MAPPER_OPTIONS) &&
    open.params.compilerOptions?.strict === true;
  record(
    "openProject",
    openOk,
    open
      ? `handle ${open.params?.projectHandle}; options ${JSON.stringify(open.params?.options)}; compilerOptions.strict ${open.params?.compilerOptions?.strict}`
      : `no openProject request; ${brief(utf16)}`,
  );

  const transform = utf16.requests.find((request) => request.method === "transform");
  record(
    "transform",
    transform &&
      transform.params?.content === STP5_SOURCE &&
      transform.params?.projectHandle === open?.params?.projectHandle &&
      normalizePath(transform.params?.fileName).endsWith("/app.stp5") &&
      has(utf16.diagnostics, expected.present[3]),
    transform
      ? `fileName ${transform.params?.fileName}; full content ${transform.params?.content === STP5_SOURCE}; cross-file type error ${has(utf16.diagnostics, expected.present[3])}`
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

  const unused = expected.present[1];
  record(
    "unused-expect-diagnostic",
    has(utf16.diagnostics, unused),
    `${DIAGNOSTIC_SOURCE}${UNUSED_EXPECT_CODE} expected at App.stp5(${unused.line},${unused.column}); got ${brief(utf16)}`,
  );

  const leaked = utf16.diagnostics.filter(
    (row) => row.file === "App.stp5" && expected.suppressedLines.includes(row.line),
  );

  // Directive twin: the same lines with the directives commented out must
  // report what the directives suppressed, and no unused-Expect diagnostic.
  const bare = withProject(
    engine,
    { source: STP5_SOURCE.replace(/@expect/g, "#expect").replace(/@ignore/g, "#ignore") },
    [],
    (run) => run,
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
    "unused-expect-diagnostic",
    !bare.diagnostics.some((row) => row.code === UNUSED_EXPECT_CODE),
    "no unused-Expect diagnostic without the directive",
  );

  const ambiguous = withProject(engine, { twin: "ambiguous-unused-index" }, [], (run) => run);
  record(
    "unused-expect-index",
    rejected(ambiguous, /unusedExpectDirectiveIndex/),
    `explicit index 0 accepted in the main run; omitted index with two unused diagnostics: ${brief(ambiguous)}`,
  );

  const atom = withProject(engine, { twin: "atom-call" }, [], (run) => run);
  const atomPlus = atom.diagnostics.find((row) => row.code === 2304);
  record(
    "alias-display",
    atomPlus && /'add_op'/.test(atomPlus.message),
    `Atom twin keeps the virtual name: ${atomPlus?.message ?? brief(atom)}`,
  );

  const synthesized = withProject(engine, { twin: "synthesized-error" }, [], (run) => run);
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

  const parse = withProject(engine, { source: STP5_PARSE_ERROR_SOURCE }, [], (run) => run);
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

  const optionRun = withProject(engine, { options: INVALID_MAPPER_OPTIONS }, [], (run, project) => {
    const text = fs.readFileSync(path.join(project.dir, "tsconfig.json"), "utf8");
    const index = text.indexOf('"name": true') + '"name": '.length;
    return { ...run, expectedAt: lineColumnOf(text, "true", index) };
  });
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

  // Static configuration (with an explicit locale) is accepted; dynamic
  // configuration without an identity, a static mapper returning an identity,
  // and a static mapper returning watched files are each refused.
  const staticRun = withProject(
    engine,
    { twin: "static-config", dynamicConfig: false },
    ["--locale", "fr"],
    (run) => run,
  );
  const staticInit = staticRun.requests.find((request) => request.method === "initialize");
  record(
    "locale",
    staticInit?.params?.locale === "fr" && init && init.params?.locale === undefined,
    `--locale fr sent ${JSON.stringify(staticInit?.params?.locale)}; default sent ${JSON.stringify(init?.params?.locale)}`,
  );
  const missingIdentity = withProject(engine, { twin: "omit-config-identity" }, [], (run) => run);
  const staticIdentity = withProject(engine, { dynamicConfig: false }, [], (run) => run);
  const staticWatched = withProject(
    engine,
    { twin: "omit-config-identity", dynamicConfig: false },
    [],
    (run) => run,
  );
  const staticAccepted = expected.present.every((row) => has(staticRun.diagnostics, row));
  const build = runBuildMode(engine);
  record(
    "dynamic-config-identity",
    staticAccepted &&
      rejected(missingIdentity, /did not return 'configIdentity'/) &&
      rejected(staticIdentity, /returned 'configIdentity'/) &&
      build.first.status === 0 &&
      build.first.methods.includes("transform") &&
      build.unchanged.upToDate &&
      !build.unchanged.methods.includes("transform") &&
      build.unchanged.methods.includes("openProject") &&
      !build.changed.upToDate &&
      build.changed.methods.includes("transform") &&
      build.settled.upToDate,
    `static accepted ${staticAccepted}; missing identity refused ${rejected(missingIdentity, /did not return 'configIdentity'/)}; static identity refused ${rejected(staticIdentity, /returned 'configIdentity'/)}; -b unchanged up-to-date ${build.unchanged.upToDate} [${build.unchanged.methods}]; -b after config change rebuilt ${!build.changed.upToDate} [${build.changed.methods}]`,
  );

  const watched = await runWatchTouch(engine, "stp5.config.json");
  const unwatched = await runWatchTouch(engine, "unwatched.json");
  const reopened = (after) => {
    const close = after.indexOf("closeProject");
    return close >= 0 && after.indexOf("openProject", close) > close;
  };
  record(
    "watched-files",
    rejected(staticWatched, /returned 'watchedFiles'/) &&
      watched.rebuilt &&
      reopened(watched.after) &&
      unwatched.rebuilt &&
      !unwatched.after.includes("openProject"),
    `static watchedFiles refused ${rejected(staticWatched, /returned 'watchedFiles'/)}; --watch after watched-file change [${watched.after}]; after unwatched change [${unwatched.after}]`,
  );

  const reserved = withProject(engine, { twin: "reserved-diagnostic-source" }, [], (run) => run);
  const unoffered = withProject(engine, { twin: "unoffered-encoding" }, [], (run) => run);
  record(
    "diagnostic-source",
    utf16.diagnostics.some((row) => row.source === DIAGNOSTIC_SOURCE) &&
      reserved.diagnostics.some((row) => row.code === MAPPER_FAILURE_CODE) &&
      !methods(reserved).includes("transform"),
    `mapper diagnostics carry '${DIAGNOSTIC_SOURCE}'; diagnosticSource "tsc": ${brief(reserved)}`,
  );
  record(
    "encoding-negotiation",
    unoffered.diagnostics.some((row) => row.code === MAPPER_FAILURE_CODE) &&
      !methods(unoffered).includes("transform"),
    `unoffered encoding: ${brief(unoffered)}`,
  );

  const noExtension = withProject(engine, { twin: "omit-extension" }, [], (run) => run);
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
  return {
    performed: true,
    engineId: engine.id,
    engineVersion: engine.version,
    operations,
    unsupported: operations.filter((row) => !row.ok).map((row) => row.operation),
  };
}
