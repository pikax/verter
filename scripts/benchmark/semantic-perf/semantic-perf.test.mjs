// Self-tests of the semantic benchmark harness: the answer canonicaliser,
// the counterbalanced schedule, the reference parser, the classifier, and the
// validator's failure conditions on synthetic runs (a wrong answer, zero,
// duplicate and missing records, a failed child, inconsistent repetitions, a
// wrong binary, unconsented sampled containment, a tampered summary and a
// tampered raw record). No cargo build and no tsc process is needed.
//
//   node --test scripts/benchmark/semantic-perf/semantic-perf.test.mjs

import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { classifyVerterAnswer, compactProbeRecord, verdict } from "./analyze.mjs";
import { canonicalDigest, canonicalType } from "./canonical.mjs";
import { MEASURING_SUFFIX, parseMeasurement } from "./measure-expected.mjs";
import { schedule } from "./run.mjs";
import { sha256Text } from "./provenance.mjs";
import { allScenarios, cliSource, moduleText, SETTINGS, tsconfigText } from "./scenarios.mjs";
import { summarize } from "./summary.mjs";
import { resolveSupervisor } from "./supervisor.mjs";
import { rawFileProblems, validateRun } from "./validate.mjs";

test("the canonical form ignores quoting, union order and object member order, and keeps tuple order", () => {
  assert.equal(canonicalType(`'a-2' | 'a-1'`), canonicalType(`"a-1" | "a-2"`));
  assert.equal(canonicalType(`{ b: 2, a: 1 }`), canonicalType(`{ a: 1; b: 2; }`));
  assert.equal(canonicalType(`boolean[]`), canonicalType(`Array<boolean>`));
  assert.equal(canonicalType(`readonly string[]`), canonicalType(`ReadonlyArray<string>`));
  assert.equal(canonicalType(`true | false | 1`), canonicalType(`1 | boolean`));
  assert.notEqual(canonicalType(`[1, 2]`), canonicalType(`[2, 1]`));
  assert.notEqual(canonicalType(`"ok" | undefined`), canonicalType(`"ok"`));
  assert.notEqual(canonicalType(`any`), canonicalType(`unknown`));
  assert.equal(canonicalDigest(`2 | 1`).sha256, canonicalDigest(`1 | 2`).sha256);
});

test("every scenario is one module declaring __BenchInit and __Probe once", () => {
  const ids = new Set();
  for (const s of allScenarios()) {
    assert.ok(!ids.has(s.id), `duplicate scenario id ${s.id}`);
    ids.add(s.id);
    assert.equal(s.source.split("type __BenchInit ").length, 2, s.id);
    assert.equal(s.source.split("type __Probe ").length, 2, s.id);
    assert.ok(s.source.startsWith(moduleText("", "x").split("\n")[0]), s.id);
  }
});

test("the schedule runs warmups first and puts each arm first in half of the measured rounds", () => {
  const plan = schedule(["a", "b"], ["verter", "tsc-api"], 4, 1);
  assert.equal(plan.length, (4 + 1) * 2 * 2);
  assert.ok(plan.slice(0, 4).every((p) => p.warmup));
  const firsts = { verter: 0, "tsc-api": 0 };
  for (let i = 4; i < plan.length; i += 2) firsts[plan[i].arm]++;
  assert.equal(firsts.verter, firsts["tsc-api"]);
  // Scenario order reverses on alternate rounds.
  assert.equal(plan[0].key, "a");
  assert.equal(plan[4].key, "b");
});

test("the reference parser reads the probe's members, never, and error-any, and never guesses", () => {
  const source = moduleText("", "1") + MEASURING_SUFFIX;
  const lines = source.split("\n");
  const line = lines.findIndex((l) => l.includes("const __bench_s")) + 1;
  const neverLine = lines.findIndex((l) => l.includes("const __bench_n")) + 1;
  const verdictLine = (answer) =>
    `scenario.ts(${neverLine},7): error TS2322: Type '"${answer}"' is not assignable to type '"never-check"'.\n`;
  const union = parseMeasurement(
    `scenario.ts(${line},7): error TS2322: Type '[["ok"] | [undefined]]' is not assignable to type '[never]'.\n` +
      `  Type '["ok"] | [undefined]' is not assignable to type 'never'.\n` +
      verdictLine("no"),
    source,
  );
  assert.equal(canonicalType(union.text), canonicalType(`"ok" | undefined`));
  assert.equal(union.errorAny, false);
  assert.deepEqual(union.codes, []);
  assert.equal(parseMeasurement(verdictLine("yes"), source).text, "never");
  const errorAny = parseMeasurement(
    `scenario.ts(3,20): error TS2589: Type instantiation is excessively deep and possibly infinite.\n` +
      `scenario.ts(${line},7): error TS2322: Type '[any]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
    source,
  );
  assert.equal(errorAny.text, "any");
  assert.equal(errorAny.errorAny, true);
  assert.deepEqual(errorAny.codes, [2589]);
  // tsc's printer elides a very large type: bare `any` among the tuples is not an answer.
  const elided = parseMeasurement(
    `scenario.ts(${line},7): error TS2322: Type '[["a0-b0"] | ["a0-b1"] | [any] | any | [any]]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
    source,
  );
  assert.equal(elided.truncated, true);
  // Output with neither assignment's verdict (a killed or truncated run) is never read as `never`.
  assert.throws(() => parseMeasurement("", source), /never check/);
  assert.throws(() => parseMeasurement(verdictLine("no"), source), /not never/);
});

const ANSWER = (text, extra = {}) => ({
  end: { kind: "exited", exitCode: 0 },
  outcome: { kind: "value" },
  warmKinds: ["value"],
  digest: canonicalDigest(text),
  observeError: null,
  unknownLeaves: 0,
  unknownSamples: [],
  shape: "literal",
  ...extra,
});
const REF = (text, extra = {}) => ({ digest: canonicalDigest(text), errorAny: false, codes: [], ...extra });

test("a Verter answer is classified, and only an equal answer is matched", () => {
  assert.equal(classifyVerterAnswer(ANSWER("1"), REF("1")).class, "matched");
  assert.equal(classifyVerterAnswer(ANSWER("2"), REF("1")).class, "mismatch");
  assert.equal(classifyVerterAnswer(ANSWER("any"), REF("any", { errorAny: true, codes: [2589] })).class, "mismatch");
  assert.equal(
    classifyVerterAnswer(ANSWER(`"ok"`), REF("any", { errorAny: true, codes: [2589] }), canonicalDigest(`"ok"`)).class,
    "beyond-tsc",
  );
  assert.equal(classifyVerterAnswer(ANSWER("1", { unknownLeaves: 1, unknownSamples: ["semanticMiss"] }), REF("1")).class, "partial");
  assert.equal(classifyVerterAnswer(ANSWER("A<1>"), REF("1"), null, canonicalDigest("A<1>")).class, "partial");
  assert.equal(classifyVerterAnswer({ ...ANSWER("1"), outcome: { kind: "fault", detail: "BudgetExceeded(..)" } }, REF("1")).class, "refusal");
  assert.equal(classifyVerterAnswer({ ...ANSWER("1"), end: { kind: "killed", detail: "memory" } }, REF("1")).class, "killed");
});

test("an elided long answer classifies exactly as its raw text", () => {
  const long = Array.from({ length: 2000 }, (_, i) => `"m${i}"`).join(" | ");
  const record = { tool: "verter", probes: [{ alias: "__Probe", observation: { text: long, error: null } }] };
  const compact = compactProbeRecord(record);
  assert.equal(compact.probes[0].observation.text, null);
  assert.equal(compact.probes[0].observation.textElided, true);
  assert.equal(compact.probes[0].observation.canonical.sha256, canonicalDigest(long).sha256);
  // A short answer is embedded unchanged.
  const short = { tool: "verter", probes: [{ alias: "__Probe", observation: { text: "1" } }] };
  assert.deepEqual(compactProbeRecord(short), short);
});

test("a verdict names a winner only when the repetitions do not overlap", () => {
  assert.equal(verdict([1, 1.2], [5, 6], 1).verdict, "verter");
  assert.equal(verdict([5, 6], [1, 1.2], 1).verdict, "tsc");
  assert.equal(verdict([1, 4], [3, 6], 1).verdict, "overlap");
  assert.equal(verdict([1, 1.2], [1.5, 1.6], 1).verdict, "overlap");
});

// ---------------------------------------------------------------- synthetic runs

const SCENARIO = { id: "synthetic", family: "test", note: "", probe: "1", source: moduleText("", "1") };
const EXPECTED = {
  schema: 1,
  libSha256: "lib-digest",
  scenarios: { synthetic: { sourceSha256: sha256Text(SCENARIO.source), settings: { strict: { digest: canonicalDigest("1"), errorAny: false, codes: [] } } } },
};
const INPUTS = {
  "lib.bench.d.ts": "lib-digest",
  "scenario.ts": sha256Text(SCENARIO.source),
  "tsconfig.json": sha256Text(tsconfigText(SETTINGS[0])),
  "cli/scenario.ts": sha256Text(cliSource(SCENARIO)),
};
const PACKAGES = ["verter_bench", "verter_session", "verter_semantic", "verter_workspace", "verter_audit", "verter_type_expr", "verter_scheduler", "verter_compiler"];

function supervisorRecord(overrides = {}) {
  return {
    schema: 1,
    launched: true,
    wallMs: 10,
    exitCode: 0,
    signal: null,
    killedBy: null,
    memLimitBytes: 64 * 1024 * 1024,
    timeoutMs: 1000,
    peakBytes: 1000,
    peakMetric: "job-peak-committed-bytes",
    containment: "hard",
    backend: "test",
    stdoutPath: "out.log",
    stderrPath: "err.log",
    errors: [],
    ...overrides,
  };
}

function verterProbe(text, arm) {
  return {
    schema: 1,
    tool: "verter",
    instrumented: arm === "verter-counted",
    observability: arm === "verter-obs",
    phases: { setup: 1000, init: 100, teardown: 10 },
    init: { micros: 100, outcome: { kind: "value" } },
    probes: [
      {
        alias: "__Probe",
        cold: { micros: 50, outcome: { kind: "value" } },
        warm: [{ micros: 1, outcome: { kind: "value" } }],
        observeMicros: 5,
        observation: { text, error: null, shape: "literal", unionMembers: null, unknownLeaves: 0, unknownSamples: [], conditionalNodes: 0 },
      },
    ],
    afterRequests: { pid: 1, metric: "private-commit", peakBytes: 2000, currentBytes: 1500, cpuMicros: 1000 },
    afterTeardown: null,
    statsErrors: [],
    retention: {},
  };
}

function tscProbe(text) {
  return {
    schema: 1,
    tool: "tsc",
    serverPid: 2,
    rootFiles: ["/x/synthetic/strict/lib.bench.d.ts", "/x/synthetic/strict/scenario.ts"],
    phases: { spawnMs: 40, setupMs: 5, initMs: 1, teardownMs: 1 },
    init: { roundTripMs: 1, serverMs: 0.5, outcome: { kind: "value" } },
    probes: [
      {
        alias: "__Probe",
        cold: { roundTripMs: 2, serverMs: 1, outcome: { kind: "value" } },
        warm: [{ roundTripMs: 0.2, serverMs: 0, outcome: { kind: "value" } }],
        observeMs: 1,
        observation: { text, error: null, errorType: false, typeFlags: 2048, unionMembers: null },
      },
    ],
    serverAfterRequests: { pid: 2, metric: "private-commit", peakBytes: 3000, currentBytes: 2500, cpuMicros: 1000 },
    diagnostics: [],
    statsErrors: [],
  };
}

function syntheticRun({ verterText = "1", tscText = "1" } = {}) {
  const options = { arms: ["verter", "tsc-api"], repeat: 2, warmup: 1, warmRepeats: 1, memMb: 64, timeoutMs: 1000, allowSampled: false, settings: "strict", libMode: "root-file" };
  const plan = schedule(["synthetic/strict"], options.arms, options.repeat, options.warmup);
  const invocations = plan.map((p, index) => ({
    index,
    scenario: "synthetic",
    setting: "strict",
    arm: p.arm,
    rep: p.rep,
    warmup: p.warmup,
    command: [p.arm === "verter" ? "/bin/probe" : "node"],
    supervisorOut: "/nonexistent",
    supervisorExit: 0,
    supervisor: supervisorRecord(),
    probeOut: "/nonexistent",
    probe: p.arm === "verter" ? verterProbe(verterText, p.arm) : tscProbe(tscText),
  }));
  const build = {
    packages: Object.fromEntries(PACKAGES.map((p) => [p, { features: [], profile: { opt_level: "3", debug_assertions: false, test: false } }])),
  };
  const run = {
    schema: 1,
    meta: {
      options,
      plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
      tree: { head: "h", diffSha256: "d", untrackedSha256: "u" },
      buildInputs: { head: "h", diffSha256: "d", untrackedSha256: "u" },
      buildInputsAfterBuild: { head: "h", diffSha256: "d", untrackedSha256: "u" },
      harness: { "scripts/benchmark/semantic-perf.mjs": "x" },
      harnessAfter: { "scripts/benchmark/semantic-perf.mjs": "x" },
      typescript: { version: "7.0.2", platformVersion: "7.0.2", versionText: "Version 7.0.2", exeSha256: "t" },
      build,
      binaries: {
        probe: { sha256: "p", pinned: "/bin/probe", identity: { debugAssertions: false, instrumented: false } },
        counted: { sha256: "c", pinned: "/bin/counted", identity: { debugAssertions: false, instrumented: true } },
        supervisor: { sha256: "s", pinned: "/bin/sup" },
      },
      binariesAfter: { probe: "p", counted: "c", supervisor: "s", tsc: "t" },
      scenarios: { "synthetic/strict": { id: "synthetic", family: "test", note: "", setting: "strict", dir: "/x/synthetic/strict", inputs: { ...INPUTS } } },
    },
    invocations,
  };
  run.summary = summarize(run, EXPECTED, [SCENARIO]);
  return run;
}

const validate = (run, opts) => validateRun(run, EXPECTED, [SCENARIO], opts);
const failsWith = (run, pattern, opts) => {
  const result = validate(run, opts);
  assert.equal(result.ok, false, "validation must fail");
  assert.ok(
    result.failures.some((f) => pattern.test(f)),
    `expected a failure matching ${pattern}, got:\n  ${result.failures.join("\n  ")}`,
  );
};
const resummarize = (run) => {
  run.summary = summarize(run, EXPECTED, [SCENARIO]);
  return run;
};

test("a well-formed synthetic run passes and compares the matched row", () => {
  const run = syntheticRun();
  const result = validate(run);
  assert.deepEqual(result.failures, []);
  const cell = run.summary.cells[0];
  assert.equal(cell.arms.verter.class, "matched");
  assert.ok(cell.headline, "a matched row is compared");
});

test("a wrong tsc answer against the measured reference fails validation", () => {
  failsWith(resummarize(syntheticRun({ tscText: "2" })), /wrong answer against the measured reference/);
});

test("a tsc error-type flag that disagrees with the reference fails validation", () => {
  const run = syntheticRun();
  for (const inv of run.invocations) if (inv.arm === "tsc-api") inv.probe.probes[0].observation.errorType = true;
  failsWith(resummarize(run), /error-type flag/);
});

test("when the measuring tsc -p exhausts resources, an API answer equal to the constructed one becomes the reference", () => {
  const scenario = { ...SCENARIO, beyond: "1" };
  const killedRef = structuredClone(EXPECTED);
  killedRef.scenarios.synthetic.settings.strict = { killed: "memory", codes: [] };
  const run = syntheticRun();
  run.summary = summarize(run, killedRef, [scenario]);
  const cell = run.summary.cells[0];
  assert.equal(cell.arms["tsc-api"].class, "reference-by-construction");
  assert.equal(cell.arms.verter.class, "matched");
  assert.ok(cell.headline);
  assert.deepEqual(validateRun(run, killedRef, [scenario]).failures, []);
  // Without a constructed answer the API's answer cannot be checked: never compared, not a failure.
  const run2 = syntheticRun();
  run2.summary = summarize(run2, killedRef, [SCENARIO]);
  assert.equal(run2.summary.cells[0].arms["tsc-api"].class, "unverified");
  assert.equal(run2.summary.cells[0].headline, null);
  assert.deepEqual(validateRun(run2, killedRef, [SCENARIO]).failures, []);
});

test("a wrong Verter answer is a finding, never a comparison, and fails --require-all-matched", () => {
  const run = resummarize(syntheticRun({ verterText: "2" }));
  assert.deepEqual(validate(run).failures, []);
  assert.equal(run.summary.cells[0].arms.verter.class, "mismatch");
  assert.equal(run.summary.cells[0].headline, null);
  failsWith(run, /require-all-matched/, { requireAllMatched: true });
});

test("a summary claiming a match the raw answers do not support fails validation", () => {
  const run = syntheticRun();
  for (const inv of run.invocations) if (inv.arm === "verter") inv.probe.probes[0].observation.text = "2";
  // The stored summary still says matched.
  failsWith(run, /disagrees with its raw records/);
});

test("zero records for a planned arm fail validation", () => {
  const run = syntheticRun();
  run.invocations = run.invocations.filter((i) => i.arm !== "tsc-api").map((inv, index) => ({ ...inv, index }));
  failsWith(resummarize(run), /zero records/);
});

test("a run with no invocation at all fails validation", () => {
  const run = syntheticRun();
  run.invocations = [];
  failsWith(resummarize(run), /zero records/);
});

test("a duplicate record fails validation", () => {
  const run = syntheticRun();
  run.invocations.push({ ...run.invocations[1], index: run.invocations.length });
  failsWith(resummarize(run), /duplicate record/);
});

test("a missing record fails validation", () => {
  const run = syntheticRun();
  run.invocations = run.invocations.slice(0, -1);
  failsWith(resummarize(run), /missing record/);
});

test("a failed child fails validation", () => {
  const run = syntheticRun();
  run.invocations[2].supervisor = supervisorRecord({ exitCode: 101 });
  run.invocations[2].supervisorExit = 101;
  failsWith(resummarize(run), /failed child/);
});

test("a supervisor that lost containment fails validation", () => {
  const run = syntheticRun();
  run.invocations[2].supervisor = supervisorRecord({ killedBy: "supervisor-error", exitCode: null });
  run.invocations[2].supervisorExit = 125;
  failsWith(resummarize(run), /failed child/);
});

test("a probe record without process statistics fails validation", () => {
  const run = syntheticRun();
  const inv = run.invocations.find((i) => i.arm === "verter");
  inv.probe.afterRequests = null;
  failsWith(resummarize(run), /no process statistics/);
});

test("repetitions that disagree fail validation", () => {
  const run = syntheticRun();
  run.invocations.filter((i) => i.arm === "verter").at(-1).probe = verterProbe("2", "verter");
  failsWith(resummarize(run), /inconsistent repetitions/);
});

test("a binary that changed during the run fails validation", () => {
  const run = syntheticRun();
  run.meta.binariesAfter.probe = "other";
  failsWith(run, /the Verter probe changed/);
});

test("a build input or the harness changing during the run fails validation", () => {
  const run = syntheticRun();
  run.meta.buildInputsAfterBuild.diffSha256 = "other";
  failsWith(run, /build inputs changed/);
  const run2 = syntheticRun();
  run2.meta.harnessAfter = { "scripts/benchmark/semantic-perf.mjs": "y" };
  failsWith(run2, /harness changed/);
});

test("TypeScript other than 7.0.2 fails validation", () => {
  const run = syntheticRun();
  run.meta.typescript.version = "7.0.1";
  failsWith(run, /wrong binary: TypeScript/);
});

test("a probe built with a test-only feature or without optimisation fails validation", () => {
  const run = syntheticRun();
  run.meta.build.packages.verter_session.features = ["test-support"];
  failsWith(run, /non-production feature test-support/);
  const run2 = syntheticRun();
  run2.meta.build.packages.verter_session.profile.opt_level = "0";
  failsWith(run2, /opt-level 0/);
});

test("a reference measured on another source, or inputs that are not the catalog's, fail validation", () => {
  const stale = structuredClone(EXPECTED);
  stale.scenarios.synthetic.sourceSha256 = "other";
  const run = syntheticRun();
  const result = validateRun(run, stale, [SCENARIO]);
  assert.ok(result.failures.some((f) => /reference is stale/.test(f)), result.failures.join("; "));
  const run2 = syntheticRun();
  run2.meta.scenarios["synthetic/strict"].inputs["tsconfig.json"] = "other";
  failsWith(run2, /tsconfig.json is not the catalog/);
});

test("a probe run that is not the pinned binary fails validation", () => {
  const run = syntheticRun();
  run.invocations.find((i) => i.arm === "verter").command = ["/somewhere/else"];
  failsWith(run, /not the pinned probe/);
});

test("sampled containment without consent fails validation", () => {
  const run = syntheticRun();
  run.invocations[0].supervisor.containment = "sampled";
  failsWith(run, /without consent/);
});

test("after a warmup killed at the memory cap the rest of that arm may be skipped, and only then", () => {
  const run = syntheticRun();
  run.meta.options.skipAfterKill = true;
  const warmup = run.invocations.find((i) => i.arm === "tsc-api" && i.warmup);
  warmup.supervisor = supervisorRecord({ killedBy: "memory", exitCode: null });
  warmup.supervisorExit = 137;
  warmup.probe = null;
  for (const inv of run.invocations) {
    if (inv.arm === "tsc-api" && !inv.warmup) {
      for (const key of ["command", "supervisorOut", "supervisorExit", "supervisor", "probeOut", "probe"]) delete inv[key];
      inv.skipped = { after: warmup.index, reason: "a warmup of this scenario and arm was killed at the memory cap" };
    }
  }
  resummarize(run);
  assert.deepEqual(validate(run).failures, []);
  assert.equal(run.summary.cells[0].arms["tsc-api"].class, "killed");
  assert.equal(run.summary.cells[0].headline, null);
  // A skip that no memory-killed warmup justifies fails validation.
  const run2 = syntheticRun();
  run2.meta.options.skipAfterKill = true;
  const inv = run2.invocations.find((i) => i.arm === "tsc-api" && !i.warmup);
  inv.skipped = { after: 0, reason: "made up" };
  failsWith(resummarize(run2), /skipped without a warmup/);
});

test("a raw record on disk that differs from results.json is reported", () => {
  const dir = mkdtempSync(join(tmpdir(), "semantic-perf-test-"));
  const run = syntheticRun();
  const inv = run.invocations[0];
  inv.supervisorOut = join(dir, "sup.json");
  inv.probeOut = join(dir, "probe.json");
  writeFileSync(inv.supervisorOut, JSON.stringify(inv.supervisor));
  writeFileSync(inv.probeOut, JSON.stringify(inv.probe));
  run.invocations = [inv];
  assert.deepEqual(rawFileProblems(run), []);
  writeFileSync(inv.probeOut, JSON.stringify({ ...inv.probe, tool: "tsc" }));
  assert.ok(rawFileProblems(run).some((p) => /differs from results.json/.test(p)));
});

test("without a supervisor the harness refuses to run", () => {
  const empty = mkdtempSync(join(tmpdir(), "semantic-perf-root-"));
  assert.throws(() => resolveSupervisor(empty, null), /no process supervisor/);
  assert.throws(() => resolveSupervisor(empty, join(empty, "missing")), /does not exist/);
});
