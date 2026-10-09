// Self-tests of the semantic benchmark harness: the answer canonicaliser,
// the counterbalanced schedule, the reference measurement and its
// interpretation, the classifier, and the validator's failure conditions on
// synthetic runs (a wrong answer, zero, duplicate and missing records, a
// failed child, inconsistent repetitions, a failed or different warm answer,
// missing or invalid times, a wrong binary, mixed architectures, tuning,
// unconsented sampled containment, a tampered summary and a tampered raw
// record). No cargo build and no tsc process is needed.
//
//   node --test scripts/benchmark/semantic-perf/semantic-perf.test.mjs

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { classifyVerterAnswer, compactProbeRecord, parseCli, verdict } from "./analyze.mjs";
import { canonicalDigest, canonicalType } from "./canonical.mjs";
import { MEASURING_SUFFIX, parseMeasurement } from "./measure-expected.mjs";
import { buildProblems, sha256Text, toolchainPin } from "./provenance.mjs";
import { interpretMeasurement } from "./reference.mjs";
import { renderMarkdown } from "./report.mjs";
import {
  LIB_FILE,
  sameArchitecture,
  schedule,
  scheduleBalanceProblems,
  tuningEnvironment,
  ROOT,
  TIER_DEFAULTS,
} from "./run.mjs";
import {
  allScenarios,
  cliSource,
  moduleText,
  scenariosForTier,
  SETTINGS,
  TIERS,
  tsconfigText,
} from "./scenarios.mjs";
import {
  classifyMeta,
  requiredStateComparison,
  sessionArmsOf,
  sessionInputs,
} from "./session-analyze.mjs";
import {
  allSessions,
  INCREMENTAL_FACILITY,
  SESSION_TIERS,
  sessionProblems,
  sessionsFor,
  tscFiles,
} from "./sessions.mjs";
import { summarize, timerResolution } from "./summary.mjs";
import { resolveSupervisor } from "./supervisor.mjs";
import { rawFileProblems, validateRun } from "./validate.mjs";

// ---------------------------------------------------------------- canonical form

test("the canonical form ignores quoting, union and property order, and keeps what the type system observes", () => {
  const same = [
    [`'a-2' | 'a-1'`, `"a-1" | "a-2"`],
    [`{ b: 2, a: 1 }`, `{ a: 1; b: 2; }`],
    [`{ x: 1 | 2 }`, `{ x: 2 | 1 }`],
    [`boolean[]`, `Array<boolean>`],
    [`Array<number>[]`, `Array<Array<number>>`],
    [`readonly string[]`, `ReadonlyArray<string>`],
    [`(string | number)[]`, `Array<number | string>`],
    [`true | false | 1`, `1 | boolean`],
    [`(x: "a") => void`, `(y: 'a') => void`],
    [`[a: string, b?: number]`, `[string, number?]`],
    ["`a${number}-b0`", "`a${ number }-b0`"],
    [`1.0`, `1`],
  ];
  for (const [a, b] of same) assert.equal(canonicalType(a), canonicalType(b), `${a} ≡ ${b}`);
  const different = [
    [`() => 1 | 2`, `(() => 1) | 2`],
    [`{ f(x: 1): 1; f(x: 2): 2 }`, `{ f(x: 2): 2; f(x: 1): 1 }`],
    [`{ (x: 1): 1; (x: 2): 2 }`, `{ (x: 2): 2; (x: 1): 1 }`],
    [`[1, 2]`, `[2, 1]`],
    [`A & B`, `B & A`],
    [`"ok" | undefined`, `"ok"`],
    [`any`, `unknown`],
    [`{ readonly a: 1 }`, `{ a: 1 }`],
    [`{ a?: 1 }`, `{ a: 1 }`],
    [`(x: unknown, y: unknown) => x is string`, `(y: unknown, x: unknown) => x is string`],
    [`{ [s]: 1 }`, `{ "[s]": 1 }`],
  ];
  // Renaming a parameter everywhere, a predicate's target included, is the same type.
  assert.equal(
    canonicalType(`(x: unknown) => x is string`),
    canonicalType(`(y: unknown) => y is string`),
  );
  // A numeric key and its string spelling are one property.
  assert.equal(canonicalType(`{ 0: 1 }`), canonicalType(`{ "0": 1 }`));
  for (const [a, b] of different)
    assert.notEqual(canonicalType(a), canonicalType(b), `${a} ≢ ${b}`);
  assert.equal(canonicalDigest(`2 | 1`).sha256, canonicalDigest(`1 | 2`).sha256);
});

test("binders are positions: consistent renaming is one type, swapped or free names are not", () => {
  const same = [
    [`<T>(x: T) => T`, `<U>(y: U) => U`],
    [`(a: string, b: typeof a) => void`, `(x: string, y: typeof x) => void`],
    [
      `T extends [infer A, infer B] ? [B, A] : never`,
      `T extends [infer X, infer Y] ? [Y, X] : never`,
    ],
    [`{ [K in "a" | "b"]: K }`, `{ [P in "b" | "a"]: P }`],
    [`<T, U extends T>(t: T, u: U) => U`, `<A, B extends A>(t: A, u: B) => B`],
  ];
  for (const [a, b] of same) assert.equal(canonicalType(a), canonicalType(b), `${a} ≡ ${b}`);
  const different = [
    [`<T, U>(x: T, y: U) => T`, `<T, U>(x: T, y: U) => U`],
    [`<T, U>(x: T) => U`, `<U, T>(x: T) => U`],
    [`(a: string, b: string) => typeof a`, `(a: string, b: string) => typeof b`],
    [`T extends [infer A, infer B] ? A : never`, `T extends [infer A, infer B] ? B : never`],
    // A bound name is not the free name it shadows.
    [`<T>(x: T) => T`, `<U>(x: U) => T`],
    [`{ [K in "a"]: K }`, `{ [K in "a"]: J }`],
  ];
  for (const [a, b] of different)
    assert.notEqual(canonicalType(a), canonicalType(b), `${a} ≢ ${b}`);
  // An infer name is a type only in the true branch: in the extends clause a
  // plain reference resolves outside, and so does one in the false branch.
  assert.notEqual(
    canonicalType(`<A, B, C>() => C extends [infer A, A] ? A : never`),
    canonicalType(`<A, B, C>() => C extends [infer B, B] ? B : never`),
  );
  assert.notEqual(
    canonicalType(`T extends [infer A] ? 1 : A`),
    canonicalType(`T extends [infer B] ? 1 : B`),
  );
  assert.equal(
    canonicalType(`T extends [infer A] ? 1 : A`),
    canonicalType(`T extends [infer B] ? 1 : A`),
  );
  // An infer name is visible in its own constraint.
  assert.notEqual(
    canonicalType(`<A, T>() => T extends (infer A extends { x: A }) ? A : never`),
    canonicalType(`<A, T>() => T extends (infer B extends { x: A }) ? B : never`),
  );
  assert.equal(
    canonicalType(`<A, T>() => T extends (infer A extends { x: A }) ? A : never`),
    canonicalType(`<A, T>() => T extends (infer B extends { x: B }) ? B : never`),
  );
  // Declarations are numbered after normalisation: reordered properties are one type.
  assert.equal(
    canonicalType(`<T>() => T extends { a: infer A; b: infer B } ? [A, B] : never`),
    canonicalType(`<T>() => T extends { b: infer B; a: infer A } ? [A, B] : never`),
  );
  assert.notEqual(
    canonicalType(`<T>() => T extends { a: infer A; b: infer B } ? [A, B] : never`),
    canonicalType(`<T>() => T extends { b: infer A; a: infer B } ? [A, B] : never`),
  );
  // Members that differ only in their declarations cannot be ordered: fail closed.
  assert.throws(() => canonicalType(`T extends [infer A] | [infer B] ? [A, B] : 0`), /ambiguous/);
  // An index signature's parameter binds a value in its value type.
  assert.notEqual(
    canonicalType(`<T>(x: T) => { [x: string]: typeof x }`),
    canonicalType(`<T>(x: T) => { [y: string]: typeof x }`),
  );
  assert.equal(
    canonicalType(`<T>(x: T) => { [x: string]: typeof x }`),
    canonicalType(`<T>(x: T) => { [y: string]: typeof y }`),
  );
  // A computed key's name is a value reference like any other.
  assert.notEqual(
    canonicalType(`(x: symbol, y: symbol) => { [x]: 1 }`),
    canonicalType(`(x: symbol, y: symbol) => { [y]: 1 }`),
  );
  assert.equal(
    canonicalType(`(x: symbol) => { [x]: 1 }`),
    canonicalType(`(y: symbol) => { [y]: 1 }`),
  );
  // Shadowing: the inner binder wins.
  assert.equal(
    canonicalType(`<T>(x: T) => <T>(y: T) => T`),
    canonicalType(`<A>(x: A) => <B>(y: B) => B`),
  );
  assert.notEqual(
    canonicalType(`<T>(x: T) => <T>(y: T) => T`),
    canonicalType(`<A>(x: A) => <B>(y: B) => A`),
  );
});

test("the canonical form fails closed on syntax it does not know", () => {
  for (const bad of ["{ x: }", "foo bar", "=> 1", "", "a |"])
    assert.throws(() => canonicalType(bad), `${JSON.stringify(bad)} must throw`);
});

// ---------------------------------------------------------------- catalog and schedule

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

test("the schedule balances every pair of arms in every cell, for odd and even cell counts", () => {
  const arms = ["verter", "tsc-api", "verter-obs", "tsc-cli"];
  for (const cells of [["a"], ["a", "b"], ["a", "b", "c"], ["a", "b", "c", "d"]]) {
    for (const warmup of [0, 1, 2]) {
      const plan = schedule(cells, arms, 4, warmup);
      assert.equal(plan.length, (4 + warmup) * cells.length * arms.length);
      assert.ok(plan.slice(0, warmup * cells.length * arms.length).every((p) => p.warmup));
      assert.deepEqual(
        scheduleBalanceProblems(plan, arms),
        [],
        `${cells.length} cells, ${warmup} warmups`,
      );
    }
  }
  // The earlier defect: with two cells, per-cell order must still alternate.
  const plan = schedule(["a", "b"], ["verter", "tsc-api"], 4, 1).filter(
    (p) => !p.warmup && p.key === "a",
  );
  const firsts = plan.filter((_, i) => i % 2 === 0).map((p) => p.arm);
  assert.deepEqual(firsts.sort(), ["tsc-api", "tsc-api", "verter", "verter"]);
  // An odd number of measured rounds balances to within one per cell, and
  // the cells alternate the extra lead, so the run is within one per pair.
  for (const cells of [["a"], ["a", "b"], ["a", "b", "c"]])
    assert.deepEqual(
      scheduleBalanceProblems(schedule(cells, ["verter", "tsc-api"], 3, 1), ["verter", "tsc-api"]),
      [],
    );
  // A plan with the same arm first in every round is unbalanced.
  const lopsided = [0, 1, 2].flatMap((rep) => [
    { key: "a", arm: "verter", rep, warmup: false },
    { key: "a", arm: "tsc-api", rep, warmup: false },
  ]);
  assert.notDeepEqual(scheduleBalanceProblems(lopsided, ["verter", "tsc-api"]), []);
});

test("tuning variables and architecture names are recognised", () => {
  assert.deepEqual(
    Object.keys(
      tuningEnvironment({
        GOGC: "off",
        PATH: "x",
        CARGO_PROFILE_RELEASE_LTO: "false",
        VERTER_X: "1",
        HOME: "h",
      }),
    ),
    ["CARGO_PROFILE_RELEASE_LTO", "GOGC", "VERTER_X"],
  );
  // Windows names are case-insensitive; build parallelism is not tuning.
  assert.deepEqual(
    Object.keys(
      tuningEnvironment({
        gogc: "off",
        Rustc_Wrapper: "sccache",
        CARGO_BUILD_JOBS: "6",
        cargo_build_rustflags: "-C x",
      }),
    ),
    ["cargo_build_rustflags", "gogc", "Rustc_Wrapper"],
  );
  assert.ok(sameArchitecture("aarch64", "arm64"));
  assert.ok(sameArchitecture("x86_64", "x64"));
  assert.ok(!sameArchitecture("x86_64", "arm64"));
});

// ---------------------------------------------------------------- reference

test("the reference measurement records tsc's print and the interpretation reads it, and neither guesses", () => {
  const source = moduleText("", "1") + MEASURING_SUFFIX;
  const lines = source.split("\n");
  const line = lines.findIndex((l) => l.includes("const __bench_s")) + 1;
  const neverLine = lines.findIndex((l) => l.includes("const __bench_n")) + 1;
  const verdictLine = (answer) =>
    `scenario.ts(${neverLine},7): error TS2322: Type '"${answer}"' is not assignable to type '"never-check"'.\n`;
  const read = (stdout) => interpretMeasurement(parseMeasurement(stdout, source));
  const union = read(
    `scenario.ts(${line},7): error TS2322: Type '[["ok"] | [undefined]]' is not assignable to type '[never]'.\n` +
      `  Type '["ok"] | [undefined]' is not assignable to type 'never'.\n` +
      verdictLine("no"),
  );
  assert.equal(union.digest.sha256, canonicalDigest(`"ok" | undefined`).sha256);
  assert.equal(union.errorAny, false);
  assert.deepEqual(union.codes, []);
  assert.equal(read(verdictLine("yes")).digest.sha256, canonicalDigest("never").sha256);
  const errorAny = read(
    `scenario.ts(3,20): error TS2589: Type instantiation is excessively deep and possibly infinite.\n` +
      `scenario.ts(${line},7): error TS2322: Type '[any]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
  );
  assert.equal(errorAny.errorAny, true);
  assert.deepEqual(errorAny.codes, [2589]);
  // A function member keeps its parameter types and its extent.
  const fn = read(
    `scenario.ts(${line},7): error TS2322: Type '[[(x: number) => string]]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
  );
  assert.equal(fn.digest.sha256, canonicalDigest(`(y: number) => string`).sha256);
  const fnOrTwo = read(
    `scenario.ts(${line},7): error TS2322: Type '[[() => 1] | [2]]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
  );
  assert.equal(fnOrTwo.digest.sha256, canonicalDigest(`(() => 1) | 2`).sha256);
  assert.notEqual(fnOrTwo.digest.sha256, canonicalDigest(`() => 1 | 2`).sha256);
  // A member of the probe that is itself an object is kept (the wrapper filters nothing).
  const objects = read(
    `scenario.ts(${line},7): error TS2322: Type '[[{ readonly __benchNothing: 1; }] | [1]]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
  );
  assert.equal(objects.digest.sha256, canonicalDigest(`{ readonly __benchNothing: 1 } | 1`).sha256);
  // tsc's printer elides a very large type: bare `any` among the tuples is not an answer.
  const elided = read(
    `scenario.ts(${line},7): error TS2322: Type '[["a0-b0"] | ["a0-b1"] | [any] | any | [any]]' is not assignable to type '[never]'.\n` +
      verdictLine("no"),
  );
  assert.equal(elided.truncated, true);
  // Output with neither assignment's verdict (a killed or truncated run) is never read as `never`.
  assert.throws(() => parseMeasurement("", source), /never check/);
  assert.throws(() => parseMeasurement(verdictLine("no"), source), /not never/);
  // A measuring run's kill counts as tsc exhausting the resource only with the supervisor's evidence.
  assert.equal(interpretMeasurement({ killed: "memory", codes: [] }).killed, undefined);
  const receipt = { method: JSON.stringify(METHOD), termination: MEASURED_KILL };
  assert.equal(interpretMeasurement({ killed: "memory", codes: [], receipt }).killed, "memory");
  const sampled = {
    ...receipt,
    termination: { ...MEASURED_KILL, killTriggerBytes: 7680 * 1024 * 1024 },
  };
  assert.equal(
    interpretMeasurement({ killed: "memory", codes: [], receipt: sampled }).killed,
    undefined,
  );
  const noTrigger = {
    ...receipt,
    termination: { ...MEASURED_KILL, killTriggerBytes: null, backend: null, containment: null },
  };
  assert.equal(
    interpretMeasurement({ killed: "memory", codes: [], receipt: noTrigger }).killed,
    undefined,
  );
  const deadline = (wallMs, terminationLatencyMs) => ({
    ...receipt,
    termination: { ...MEASURED_KILL, killedBy: "timeout", wallMs, terminationLatencyMs },
  });
  assert.equal(
    interpretMeasurement({ killed: "timeout", codes: [], receipt: deadline(300100, 50) }).killed,
    "timeout",
  );
  assert.equal(
    interpretMeasurement({ killed: "timeout", codes: [], receipt: deadline(300100, 200) }).killed,
    undefined,
  );
  assert.equal(
    interpretMeasurement({ killed: "timeout", codes: [], receipt: deadline(300100, null) }).killed,
    undefined,
  );
  const cgroup = { ...receipt, termination: { ...MEASURED_KILL, backend: "linux-cgroup-v2" } };
  assert.equal(
    interpretMeasurement({ killed: "memory", codes: [], receipt: cgroup }).killed,
    undefined,
  );
});

// ---------------------------------------------------------------- classification

const ANSWER = (text, extra = {}) => ({
  end: { kind: "exited", exitCode: 0 },
  outcome: { kind: "value" },
  warmKinds: ["value"],
  warmSame: [true],
  observed: true,
  evidence: true,
  digest: canonicalDigest(text),
  observeError: null,
  unknownLeaves: 0,
  unknownSamples: [],
  shape: "literal",
  enginePeakBytes: 1000,
  ...extra,
});
const REF = (text, extra = {}) => ({
  digest: canonicalDigest(text),
  errorAny: false,
  codes: [],
  ...extra,
});

test("a Verter answer is classified, and only an equal answer is matched", () => {
  const classify = (answer, ctx) => classifyVerterAnswer(answer, ctx).class;
  assert.equal(classify(ANSWER("1"), { reference: REF("1") }), "matched");
  assert.equal(classify(ANSWER("2"), { reference: REF("1") }), "mismatch");
  assert.equal(
    classify(ANSWER("any"), { reference: REF("any", { errorAny: true, codes: [2589] }) }),
    "mismatch",
  );
  assert.equal(
    classify(ANSWER(`"ok"`), {
      reference: REF("any", { errorAny: true, codes: [2589] }),
      beyond: canonicalDigest(`"ok"`),
    }),
    "beyond-tsc",
  );
  assert.equal(
    classify(ANSWER("1", { unknownLeaves: 1, unknownSamples: ["semanticMiss"] }), {
      reference: REF("1"),
    }),
    "partial",
  );
  assert.equal(
    classify(ANSWER("A<1>"), { reference: REF("1"), probe: canonicalDigest("A<1>") }),
    "partial",
  );
  assert.equal(
    classify(
      { ...ANSWER("1"), outcome: { kind: "fault", detail: "BudgetExceeded(..)" } },
      { reference: REF("1") },
    ),
    "refusal",
  );
  assert.equal(
    classify(
      { ...ANSWER("1"), end: { kind: "killed", detail: "memory" } },
      { reference: REF("1") },
    ),
    "killed",
  );
  assert.equal(classify(ANSWER("1", { warmSame: [false] }), { reference: REF("1") }), "error");
  assert.equal(classify(ANSWER("1", { warmKinds: ["fault"] }), { reference: REF("1") }), "error");
  assert.equal(
    classify(ANSWER("1", { enginePeakBytes: 2048 }), { reference: REF("1"), budgetBytes: 1024 }),
    "killed",
  );
  assert.equal(
    classify(
      { ...ANSWER("1"), end: { kind: "observe-killed", detail: "memory during observe" } },
      { reference: REF("1") },
    ),
    "unverified",
  );
});

test("beyond tsc is reserved for an established resource limit", () => {
  const beyond = canonicalDigest(`"ok"`);
  const classify = (reference, tscKilled) =>
    classifyVerterAnswer(ANSWER(`"ok"`), { reference, beyond, tscKilled }).class;
  // A truncated or unmeasurable reference is no evidence of exhaustion.
  assert.equal(
    classify({ gap: "tsc prints the answer elided", truncated: true, codes: [] }, false),
    "no-reference",
  );
  assert.equal(
    classify({ gap: "unmeasurable: x", unmeasurable: "x", codes: [] }, true),
    "no-reference",
  );
  // A measuring program killed at the cap counts only if the demand itself was killed too.
  assert.equal(
    classify({ gap: "tsc -p exhausts resources (memory)", killed: "memory", codes: [] }, false),
    "no-reference",
  );
  assert.equal(
    classify({ gap: "tsc -p exhausts resources (memory)", killed: "memory", codes: [] }, true),
    "beyond-tsc",
  );
});

test("an elided long answer classifies exactly as its raw text", () => {
  const long = Array.from({ length: 2000 }, (_, i) => `"m${i}"`).join(" | ");
  const record = {
    tool: "verter",
    probes: [{ alias: "__Probe", observation: { text: long, error: null } }],
  };
  const compact = compactProbeRecord(record);
  assert.equal(compact.probes[0].observation.text, null);
  assert.equal(compact.probes[0].observation.textElided, true);
  assert.equal(compact.probes[0].observation.canonical.sha256, canonicalDigest(long).sha256);
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

const SCENARIO = {
  id: "synthetic",
  family: "test",
  note: "",
  probe: "1",
  source: moduleText("", "1"),
};
const METHOD = {
  measuringSuffixSha256: sha256Text(MEASURING_SUFFIX),
  libSha256: "lib-digest",
  tscExeSha256: "t",
  tscVersion: "Version 7.0.2",
  memMb: 8192,
  timeoutMs: 300000,
};
/** The supervisor's evidence for a measuring run killed at the reference cap. */
const MEASURED_KILL = {
  killedBy: "memory",
  backend: "windows-job-object",
  containment: "hard",
  memLimitBytes: 8192 * 1024 * 1024,
  killTriggerBytes: 8192 * 1024 * 1024,
  timeoutMs: 300000,
  wallMs: 5000,
  peakBytes: 8192 * 1024 * 1024,
};
const RAW_ONE = {
  never: false,
  printed: "[1]",
  codes: [],
  receipt: {
    exit: 1,
    stdoutSha256: "r",
    stdoutBytes: 1,
    tsconfigSha256: sha256Text(tsconfigText(SETTINGS[0])),
    sourceSha256: sha256Text(SCENARIO.source + MEASURING_SUFFIX),
    method: JSON.stringify(METHOD),
  },
};
const EXPECTED = {
  schema: 3,
  method: METHOD,
  scenarios: {
    synthetic: { sourceSha256: sha256Text(SCENARIO.source), settings: { strict: RAW_ONE } },
  },
};
const INPUTS = {
  "lib.bench.d.ts": "lib-digest",
  "scenario.ts": sha256Text(SCENARIO.source),
  "tsconfig.json": sha256Text(tsconfigText(SETTINGS[0])),
  "cli/scenario.ts": sha256Text(cliSource(SCENARIO)),
};
const PACKAGES = [
  "verter_bench",
  "verter_session",
  "verter_semantic",
  "verter_workspace",
  "verter_audit",
  "verter_type_expr",
  "verter_scheduler",
  "verter_compiler",
];
const MEM_MB = 64;
const PIN = toolchainPin(ROOT);
const INFRA_MB = 1024;
const HERE = dirname(fileURLToPath(import.meta.url));
const LIB_TEXT = readFileSync(LIB_FILE, "utf8");
const NODE = "node";
const TSC_SESSION_DRIVER = join(HERE, "tsc-session-probe.mjs");

function supervisorRecord(overrides = {}) {
  return {
    schema: 1,
    launched: true,
    wallMs: 10,
    exitCode: 0,
    signal: null,
    killedBy: null,
    memLimitBytes: (MEM_MB + INFRA_MB) * 1024 * 1024,
    timeoutMs: 1500,
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
    schema: 2,
    tool: "verter",
    instrumented: arm === "verter-counted",
    observability: arm === "verter-obs",
    stage: "complete",
    pid: 1,
    phases: { engineStart: 500, setup: 1000, init: 100, teardown: 10 },
    init: { micros: 100, outcome: { kind: "value" } },
    probes: [
      {
        alias: "__Probe",
        cold: { micros: 50, outcome: { kind: "value" } },
        warm: [{ micros: 1, outcome: { kind: "value" }, sameAnswerAsCold: true }],
        observeMicros: 5,
        observation: {
          text,
          error: null,
          shape: "literal",
          unionMembers: null,
          unknownLeaves: 0,
          unknownSamples: [],
          conditionalNodes: 0,
        },
      },
    ],
    afterRequests: {
      pid: 1,
      metric: "private-commit",
      peakBytes: 2000,
      currentBytes: 1500,
      cpuMicros: 1000,
    },
    afterObserve: {
      pid: 1,
      metric: "private-commit",
      peakBytes: 2100,
      currentBytes: 1600,
      cpuMicros: 1100,
    },
    afterTeardown: null,
    statsErrors: [],
    retention: {},
  };
}

function tscProbe(text) {
  return {
    schema: 2,
    tool: "tsc",
    stage: "complete",
    tscExe: "/tsc/tsc",
    serverPid: 2,
    rootFiles: ["/x/synthetic/strict/lib.bench.d.ts", "/x/synthetic/strict/scenario.ts"],
    phases: {
      spawnMs: 40,
      engineStartMs: 0.5,
      engineStartRoundTripMs: 0.8,
      setupMs: 5,
      setupRoundTripMs: 6,
      initMs: 0.5,
      teardownMs: 1,
    },
    init: { roundTripMs: 1, serverMs: 0.5, outcome: { kind: "value" } },
    probes: [
      {
        alias: "__Probe",
        cold: { roundTripMs: 2, serverMs: 1, outcome: { kind: "value" } },
        warm: [
          { roundTripMs: 0.2, serverMs: 0, outcome: { kind: "value" }, sameAnswerAsCold: true },
        ],
        observeMs: 1,
        observation: { text, error: null, errorType: false, typeFlags: 2048, unionMembers: null },
      },
    ],
    serverAfterRequests: {
      pid: 2,
      metric: "private-commit",
      peakBytes: 3000,
      currentBytes: 2500,
      cpuMicros: 1000,
    },
    serverAfterObserve: {
      pid: 2,
      metric: "private-commit",
      peakBytes: 3100,
      currentBytes: 2600,
      cpuMicros: 1100,
    },
    calibration: {
      request: "getTypeAtPosition(__BenchInit), warm",
      serverMs: Array.from({ length: 20 }, (_, i) => (i % 2 ? 0 : 0.5)),
    },
    statsErrors: [],
  };
}

/** Warm repeats are made only by each arm's first measured invocation (one live process). */
function withWarm(probe, warm) {
  if (!warm) probe.probes[0].warm = [];
  return probe;
}

function syntheticRun({
  verterText = "1",
  tscText = "1",
  arms = ["verter", "tsc-api"],
  noTsc = undefined,
} = {}) {
  const options = {
    arms,
    ...(noTsc ? { noTsc } : {}),
    repeat: 2,
    warmup: 1,
    warmRepeats: 1,
    memMb: MEM_MB,
    infraMb: INFRA_MB,
    timeoutMs: 1000,
    startupAllowanceMs: 500,
    allowSampled: false,
    settings: "strict",
    libMode: "root-file",
    tier: "quick",
    only: ["synthetic"],
  };
  const plan = schedule(["synthetic/strict"], options.arms, options.repeat, options.warmup);
  const invocations = plan.map((p, index) => ({
    index,
    scenario: "synthetic",
    setting: "strict",
    arm: p.arm,
    rep: p.rep,
    warmup: p.warmup,
    command: [
      p.arm === "verter" ? "/bin/probe" : p.arm === "verter-observe" ? "/bin/observe" : "node",
    ],
    supervisorOut: `/nonexistent/${index}.sup.json`,
    supervisorExit: 0,
    supervisor: supervisorRecord(),
    probeOut: `/nonexistent/${index}.probe.json`,
    spawnedAtMs: 1000,
    phase: "done",
    phaseHistory: [
      { phase: p.arm === "tsc-api" ? "spawn" : "engine-start", atMs: 1010 },
      { phase: "done", atMs: 1020 },
    ],
    probe: withWarm(
      p.arm === "tsc-api" ? tscProbe(tscText) : verterProbe(verterText, p.arm),
      !p.warmup && p.rep === 0,
    ),
  }));
  const build = {
    rustcPath: "/rust/rustc",
    rustcSha256: "rs",
    env: { CARGO_INCREMENTAL: "0", RUSTC: "/rust/rustc", RUSTUP_TOOLCHAIN: PIN },
    toolchainPin: PIN,
    rustc: `rustc ${PIN}\nrelease: ${PIN}\nhost: x86_64-pc-windows-msvc`,
    environment: {
      names: ["CARGO_INCREMENTAL", "PATH", "RUSTC"],
      valuesSha256: sha256Text("build environment"),
    },
    packages: Object.fromEntries(
      PACKAGES.map((p) => [
        p,
        { features: [], profile: { opt_level: "3", debug_assertions: false, test: false } },
      ]),
    ),
  };
  const run = {
    schema: 1,
    meta: {
      options,
      plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
      host: { arch: "x64", nodeExe: NODE },
      tuning: {},
      environment: {
        runtime: { names: ["PATH", "SystemRoot"], valuesSha256: sha256Text("runtime environment") },
        inherited: false,
      },
      tree: { head: "h", diffSha256: "d", untrackedSha256: "u" },
      buildInputs: { head: "h", diffSha256: "d", untrackedSha256: "u" },
      buildInputsAfterBuild: { head: "h", diffSha256: "d", untrackedSha256: "u" },
      harness: { "scripts/benchmark/semantic-perf.mjs": "x" },
      harnessAfter: { "scripts/benchmark/semantic-perf.mjs": "x" },
      typescript: {
        version: "7.0.2",
        platformVersion: "7.0.2",
        versionText: "Version 7.0.2",
        exe: "/tsc/tsc",
        exeSha256: "t",
        platformPackage: "@typescript/typescript-test-x64",
        apiSha256: { "dist/api/sync/api.js": "a" },
      },
      build,
      binaries: {
        probe: {
          sha256: "p",
          pinned: "/bin/probe",
          identity: {
            debugAssertions: false,
            instrumented: false,
            captureAvailable: false,
            targetArch: "x86_64",
            nativeArch: "x86_64",
          },
        },
        counted: {
          sha256: "c",
          pinned: "/bin/counted",
          identity: {
            debugAssertions: false,
            instrumented: true,
            targetArch: "x86_64",
            nativeArch: "x86_64",
          },
        },
        supervisor: { sha256: "s", pinned: "/bin/sup" },
      },
      binariesAfter: {
        probe: "p",
        counted: "c",
        supervisor: "s",
        tsc: "t",
        tsApi: { "dist/api/sync/api.js": "a" },
      },
      scenarios: {
        "synthetic/strict": {
          id: "synthetic",
          family: "test",
          note: "",
          setting: "strict",
          dir: "/x/synthetic/strict",
          inputs: { ...INPUTS },
        },
      },
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
const firstOf = (run, arm, measured = true) =>
  run.invocations.find((i) => i.arm === arm && i.warmup !== measured);

test("a well-formed synthetic run passes and compares the matched row", () => {
  const run = syntheticRun();
  assert.deepEqual(validate(run).failures, []);
  const cell = run.summary.cells[0];
  assert.equal(cell.arms.verter.class, "matched");
  assert.ok(cell.headline, "a matched row is compared");
  // tsc's request time is its server time bounded by the round trip.
  assert.equal(cell.arms["tsc-api"].metrics.coldMs.median, 1);
});

test("under --no-tsc a Verter-only run validates, classifies against the reference and compares nothing", () => {
  const run = syntheticRun({ arms: ["verter"], noTsc: true });
  assert.deepEqual(validate(run).failures, []);
  const cell = run.summary.cells[0];
  assert.equal(cell.arms.verter.class, "matched");
  assert.equal(cell.arms["tsc-api"], undefined);
  assert.equal(cell.headline, null);
  // The answer is still held to the measured reference.
  assert.equal(
    syntheticRun({ arms: ["verter"], noTsc: true, verterText: "2" }).summary.cells[0].arms.verter
      .class,
    "mismatch",
  );
  failsWith(syntheticRun({ noTsc: true }), /--no-tsc, yet the arms include tsc-api/);
});

test("--no-demand records an empty demand section, and only then may it be empty", () => {
  const run = syntheticRun({ arms: [] });
  Object.assign(run.meta.options, { noDemand: true, oss: [] });
  resummarize(run);
  assert.deepEqual(validate(run).failures, []);
  delete run.meta.options.noDemand;
  failsWith(run, /zero records/);
  const armed = syntheticRun();
  Object.assign(armed.meta.options, { noDemand: true, oss: [] });
  failsWith(armed, /yet the demand section has arms verter, tsc-api/);
  const none = syntheticRun({ arms: [] });
  Object.assign(none.meta.options, { noDemand: true });
  failsWith(resummarize(none), /no other section/);
  // The selected sessions schedule no invocation either; that is not a missing record.
  const sessions = syntheticRun({ arms: [] });
  Object.assign(sessions.meta.options, {
    noDemand: true,
    oss: [],
    only: ["synthetic", "incremental-edits"],
  });
  addSessions(sessions, sessionsFor("quick", TIERS, sessions.meta.options.only));
  assert.equal(sessions.sessionInvocations.length, 0);
  assert.deepEqual(validate(resummarize(sessions)).failures, []);
});

test("a wrong tsc answer against the measured reference fails validation", () => {
  failsWith(
    resummarize(syntheticRun({ tscText: "2" })),
    /wrong answer against the measured reference/,
  );
});

test("a tsc error-type flag that disagrees with the reference fails validation", () => {
  const run = syntheticRun();
  for (const inv of run.invocations)
    if (inv.arm === "tsc-api") inv.probe.probes[0].observation.errorType = true;
  failsWith(resummarize(run), /error-type flag/);
});

test("when the measurement holds no answer, an API answer equal to the constructed one becomes the reference", () => {
  const scenario = { ...SCENARIO, beyond: "1" };
  const killedRef = structuredClone(EXPECTED);
  killedRef.scenarios.synthetic.settings.strict = {
    killed: "memory",
    codes: [],
    receipt: { ...RAW_ONE.receipt, termination: MEASURED_KILL },
  };
  const run = syntheticRun();
  run.summary = summarize(run, killedRef, [scenario]);
  assert.equal(run.summary.cells[0].arms["tsc-api"].class, "reference-by-construction");
  assert.equal(run.summary.cells[0].arms.verter.class, "matched");
  assert.ok(run.summary.cells[0].headline);
  assert.deepEqual(validateRun(run, killedRef, [scenario]).failures, []);
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
  for (const inv of run.invocations)
    if (inv.arm === "verter") inv.probe.probes[0].observation.text = "2";
  failsWith(run, /disagrees with its raw records/);
});

test("zero, duplicate and missing records fail validation", () => {
  const zero = syntheticRun();
  zero.invocations = zero.invocations
    .filter((i) => i.arm !== "tsc-api")
    .map((inv, index) => ({ ...inv, index }));
  failsWith(resummarize(zero), /zero records/);
  const none = syntheticRun();
  none.invocations = [];
  failsWith(resummarize(none), /zero records/);
  const duplicate = syntheticRun();
  duplicate.invocations.push({ ...duplicate.invocations[1], index: duplicate.invocations.length });
  failsWith(resummarize(duplicate), /duplicate record/);
  const missing = syntheticRun();
  missing.invocations = missing.invocations.slice(0, -1);
  failsWith(resummarize(missing), /missing record/);
});

test("a failed child or a supervisor that lost containment fails validation", () => {
  const run = syntheticRun();
  run.invocations[2].supervisor = supervisorRecord({ exitCode: 101 });
  run.invocations[2].supervisorExit = 101;
  failsWith(resummarize(run), /failed child/);
  const run2 = syntheticRun();
  run2.invocations[2].supervisor = supervisorRecord({
    killedBy: "supervisor-error",
    exitCode: null,
  });
  run2.invocations[2].supervisorExit = 125;
  failsWith(resummarize(run2), /failed child/);
});

test("records with a failed or different warm answer, a missing or invalid time, or a statistics error fail validation", () => {
  const mutate = (arm, fn, pattern) => {
    const run = syntheticRun();
    fn(firstOf(run, arm).probe);
    failsWith(resummarize(run), pattern);
  };
  mutate("tsc-api", (r) => (r.probes[0].warm[0].outcome = { kind: "fault", detail: "x" }), /warm/);
  // Verter's warm answer differing from its cold one is a finding, not a harness failure.
  const differs = syntheticRun();
  firstOf(differs, "verter").probe.probes[0].warm[0].sameAnswerAsCold = false;
  resummarize(differs);
  assert.equal(differs.summary.cells[0].arms.verter.class, "error");
  assert.equal(differs.summary.cells[0].headline, null);
  assert.deepEqual(validate(differs).failures, []);
  mutate("tsc-api", (r) => (r.probes[0].warm[0].sameAnswerAsCold = false), /answered differently/);
  mutate("verter", (r) => delete r.probes[0].cold.micros, /cold time/);
  mutate("tsc-api", (r) => (r.probes[0].cold.serverMs = -1), /cold time/);
  mutate("tsc-api", (r) => (r.init.outcome = { kind: "miss" }), /init request/);
  mutate("verter", (r) => (r.phases.engineStart = Number.NaN), /phase time/);
  mutate("verter", (r) => (r.afterRequests = null), /engine statistics/);
  mutate("tsc-api", (r) => r.statsErrors.push("stats failed"), /statistics errors/);
  mutate("tsc-api", (r) => (r.tscExe = "/elsewhere/tsc"), /not the verified tsc/);
});

test("the arms' memory must come from one metric", () => {
  const run = syntheticRun();
  for (const inv of run.invocations)
    if (inv.arm === "tsc-api") inv.probe.serverAfterRequests.metric = "resident";
  failsWith(resummarize(run), /memory metrics differ/);
});

test("an engine over the budget is exhausted, not compared", () => {
  const run = syntheticRun();
  for (const inv of run.invocations) {
    if (inv.arm !== "verter") continue;
    inv.probe.afterRequests.peakBytes = (MEM_MB + 1) * 1024 * 1024;
    inv.probe.afterObserve.peakBytes = (MEM_MB + 1) * 1024 * 1024;
  }
  resummarize(run);
  assert.equal(run.summary.cells[0].arms.verter.class, "killed");
  assert.equal(run.summary.cells[0].headline, null);
  assert.deepEqual(validate(run).failures, []);
});

test("a kill while observing leaves the measured demand, classed unverified", () => {
  const run = syntheticRun();
  for (const inv of run.invocations.filter((i) => i.arm === "tsc-api")) {
    inv.supervisor = supervisorRecord({ killedBy: "memory", exitCode: null });
    inv.supervisorExit = 137;
    inv.phase = "observe";
    inv.probe = withWarm(
      { ...tscProbe("1"), stage: "measured", serverAfterObserve: null },
      !inv.warmup && inv.rep === 0,
    );
    inv.probe.probes[0].observation = null;
    inv.probe.probes[0].observeMs = null;
    for (const w of inv.probe.probes[0].warm) delete w.sameAnswerAsCold;
  }
  resummarize(run);
  assert.equal(run.summary.cells[0].arms["tsc-api"].class, "unverified");
  assert.equal(run.summary.cells[0].headline, null);
  assert.deepEqual(validate(run).failures, []);
});

test("a kill is the engine's only with evidence: the engine alone, at the budget, while it computes", () => {
  const kill = (arm, overrides, phase = "cold", probe = null) => {
    const run = syntheticRun();
    for (const inv of run.invocations.filter((i) => i.arm === arm)) {
      inv.supervisor = supervisorRecord({
        exitCode: null,
        backend: "windows-job-object",
        ...(overrides.killedBy === "memory"
          ? { killTriggerBytes: (MEM_MB + INFRA_MB) * 1024 * 1024 }
          : {}),
        ...(overrides.killedBy === "timeout" ? { wallMs: 1600, terminationLatencyMs: 50 } : {}),
        ...overrides,
      });
      inv.supervisorExit = overrides.killedBy === "timeout" ? 124 : 137;
      inv.phase = phase;
      if (probe === null) inv.probe = null;
      else inv.probe.stage = probe;
    }
    return resummarize(run);
  };
  const classOf = (run, arm) => run.summary.cells[0].arms[arm].class;
  const MiB = 1024 * 1024;
  // A Verter probe is the engine alone: a memory kill whose trigger is at
  // least the engine budget is its own.
  const own = kill("verter", { killedBy: "memory" });
  assert.equal(classOf(own, "verter"), "killed");
  assert.deepEqual(validate(own).failures, []);
  assert.equal(
    classOf(kill("verter", { killedBy: "memory", killTriggerBytes: MEM_MB * MiB }), "verter"),
    "killed",
  );
  // A missing actual threshold or an unknown accounting proves nothing (the configured cap is not the threshold).
  assert.equal(
    classOf(kill("verter", { killedBy: "memory", killTriggerBytes: null }), "verter"),
    "unverified",
  );
  assert.equal(
    classOf(kill("verter", { killedBy: "memory", backend: null }), "verter"),
    "unverified",
  );
  // A sampled trigger below the budget, or a Linux cgroup's accounting, proves nothing.
  assert.equal(
    classOf(kill("verter", { killedBy: "memory", killTriggerBytes: MEM_MB * MiB - 1 }), "verter"),
    "unverified",
  );
  assert.equal(
    classOf(kill("verter", { killedBy: "memory", backend: "linux-cgroup-v2" }), "verter"),
    "unverified",
  );
  // Killed after the demand completed (a measured or complete record): not the demand.
  const late = kill("verter", { killedBy: "memory" }, "teardown", "complete");
  assert.equal(classOf(late, "verter"), "unverified");
  assert.equal(
    classOf(kill("verter", { killedBy: "memory" }, "observe", "measured"), "verter"),
    "unverified",
  );
  // The tsc API tree holds the node driver too: a memory kill there is never attributed.
  for (const phase of ["cold", "setup", "spawn", "stats", "observe"]) {
    const run = kill("tsc-api", { killedBy: "memory" }, phase);
    assert.equal(classOf(run, "tsc-api"), "unverified", phase);
    assert.deepEqual(validate(run).failures, []);
  }
  // A probe's deadline is never read as exhaustion: no engine-owned clock
  // ends at the kill. It is reported with the time since the first engine phase.
  for (const [arm, phase] of [
    ["tsc-api", "cold"],
    ["tsc-api", "observe"],
    ["verter", "warm"],
    ["verter", "stats"],
  ]) {
    const run = kill(arm, { killedBy: "timeout" }, phase);
    assert.equal(classOf(run, arm), "unverified", `${arm} ${phase}`);
    assert.deepEqual(validate(run).failures, []);
  }
  // A supervisor error invalidates the invocation whatever the kill.
  const dirty = kill("verter", {
    killedBy: "memory",
    errors: ["job did not empty within the teardown limit"],
  });
  assert.ok(validate(dirty).failures.length > 0);
  // An unattributed tsc kill never makes a Verter answer beyond tsc.
  const scenario = { ...SCENARIO, beyond: "1" };
  const killedRef = structuredClone(EXPECTED);
  killedRef.scenarios.synthetic.settings.strict = {
    killed: "memory",
    codes: [],
    receipt: { ...RAW_ONE.receipt, termination: MEASURED_KILL },
  };
  const run = kill("tsc-api", { killedBy: "memory" }, "cold");
  run.summary = summarize(run, killedRef, [scenario]);
  assert.equal(run.summary.cells[0].arms.verter.class, "no-reference");
  const run2 = kill("tsc-api", { killedBy: "timeout" }, "cold");
  run2.summary = summarize(run2, killedRef, [scenario]);
  assert.equal(run2.summary.cells[0].arms.verter.class, "no-reference");
  // The demand itself exhausting the budget — the tsc server's own peak above
  // it — together with the measured exhaustion does.
  const run3 = syntheticRun();
  for (const inv of run3.invocations.filter((i) => i.arm === "tsc-api")) {
    inv.probe.serverAfterRequests.peakBytes = MEM_MB * MiB + 1;
    inv.probe.serverAfterObserve.peakBytes = MEM_MB * MiB + 2;
  }
  run3.summary = summarize(run3, killedRef, [scenario]);
  assert.equal(run3.summary.cells[0].arms.verter.class, "beyond-tsc");
});

test("an observation without its evidence is never a match, and statistics must be finite numbers", () => {
  const noFlag = syntheticRun();
  for (const inv of noFlag.invocations.filter((i) => i.arm === "tsc-api"))
    delete inv.probe.probes[0].observation.errorType;
  failsWith(resummarize(noFlag), /lacks the evidence/);
  // A failed observation supplies no answer, whatever text it still carries.
  const failed = syntheticRun();
  for (const inv of failed.invocations.filter((i) => i.arm === "tsc-api")) {
    delete inv.probe.probes[0].observation.errorType;
    inv.probe.probes[0].observation.error = "printer failed";
  }
  resummarize(failed);
  assert.notEqual(failed.summary.cells[0].arms["tsc-api"].class, "reference");
  assert.equal(failed.summary.cells[0].headline ?? null, null);
  assert.equal(validate(failed).ok, false);
  const noLeaves = syntheticRun();
  for (const inv of noLeaves.invocations.filter((i) => i.arm === "verter"))
    delete inv.probe.probes[0].observation.unknownLeaves;
  resummarize(noLeaves);
  assert.equal(noLeaves.summary.cells[0].arms.verter.class, "unverified");
  failsWith(noLeaves, /lacks the evidence/);
  for (const bad of [Infinity, "2000", -1, NaN]) {
    const run = syntheticRun();
    firstOf(run, "verter").probe.afterRequests.peakBytes = bad;
    failsWith(resummarize(run), /not a valid reading|no engine statistics/);
  }
  const noCalibration = syntheticRun();
  firstOf(noCalibration, "tsc-api").probe.calibration.serverMs.pop();
  failsWith(resummarize(noCalibration), /calibration/);
});

test("inconsistent statistics or duplicated times fail validation", () => {
  const mutate = (fn, pattern) => {
    const run = syntheticRun();
    fn(run);
    failsWith(resummarize(run), pattern);
  };
  mutate(
    (run) => (firstOf(run, "tsc-api").probe.serverAfterRequests.pid = 99),
    /not of the tsc server process/,
  );
  mutate(
    (run) => (firstOf(run, "verter").probe.afterRequests.pid = 99),
    /not of the probe's own process/,
  );
  mutate((run) => {
    const later = run.invocations.filter((i) => i.arm === "tsc-api" && !i.warmup).at(-1);
    later.probe.serverAfterRequests.metric = "resident";
    later.probe.serverAfterObserve.metric = "resident";
  }, /memory metrics differ/);
  mutate(
    (run) => (firstOf(run, "tsc-api").probe.serverAfterRequests.metric = ""),
    /not a valid reading/,
  );
  mutate(
    (run) => (firstOf(run, "verter").probe.afterRequests.currentBytes = 999999),
    /not a valid reading/,
  );
  mutate((run) => (firstOf(run, "verter").probe.phases.init = 0), /init time disagrees/);
  mutate(
    (run) => delete firstOf(run, "tsc-api").probe.phases.engineStartRoundTripMs,
    /engine-start round trip/,
  );
});

test("tsc repetitions that disagree fail validation; Verter's are a reported finding", () => {
  const run = syntheticRun();
  const lastTsc = run.invocations.filter((i) => i.arm === "tsc-api").at(-1);
  lastTsc.probe = withWarm(tscProbe("2"), !lastTsc.warmup && lastTsc.rep === 0);
  failsWith(resummarize(run), /inconsistent repetitions|wrong answer/);
  const v = syntheticRun();
  const lastVerter = v.invocations.filter((i) => i.arm === "verter").at(-1);
  lastVerter.probe = withWarm(
    verterProbe("2", "verter"),
    !lastVerter.warmup && lastVerter.rep === 0,
  );
  resummarize(v);
  const s = v.summary.cells[0].arms.verter;
  assert.equal(s.class, "mismatch");
  assert.equal(s.repetitionsDiffer, true);
  assert.match(s.detail, /repetitions differ/);
  assert.equal(v.summary.cells[0].headline, null);
  assert.deepEqual(validate(v).failures, []);
});

test("a changed binary, build input or harness fails validation", () => {
  const a = syntheticRun();
  a.meta.binariesAfter.probe = "other";
  failsWith(a, /the Verter probe changed/);
  const b = syntheticRun();
  b.meta.buildInputsAfterBuild.diffSha256 = "other";
  failsWith(b, /build inputs changed/);
  const c = syntheticRun();
  c.meta.harnessAfter = { "scripts/benchmark/semantic-perf.mjs": "y" };
  failsWith(c, /harness changed/);
  const d = syntheticRun();
  d.meta.binariesAfter.tsApi = { "dist/api/sync/api.js": "b" };
  failsWith(d, /tsc API client/);
});

test("TypeScript other than 7.0.2, or a non-production probe, fails validation", () => {
  const run = syntheticRun();
  run.meta.typescript.version = "7.0.1";
  failsWith(run, /wrong binary: TypeScript/);
  const run2 = syntheticRun();
  run2.meta.build.packages.verter_session.features = ["test-support"];
  failsWith(run2, /non-production feature test-support/);
  const run3 = syntheticRun();
  run3.meta.build.packages.verter_session.profile.opt_level = "0";
  failsWith(run3, /opt-level 0/);
});

test("mixed architectures, undeclared tuning and an unbalanced plan fail validation", () => {
  const arch = syntheticRun();
  arch.meta.binaries.probe.identity.targetArch = "aarch64";
  failsWith(arch, /one native architecture/);
  const tuned = syntheticRun();
  tuned.meta.tuning = { GOMAXPROCS: "1" };
  failsWith(tuned, /tuning variables/);
  const odd = syntheticRun();
  odd.meta.options.repeat = 3;
  failsWith(odd, /odd|plan/);
});

test("a probe run that is not the pinned binary fails validation", () => {
  const run = syntheticRun();
  run.invocations.find((i) => i.arm === "verter").command = ["/somewhere/else"];
  failsWith(run, /not the pinned probe/);
});

test("sampled containment without consent, or a wrong containment cap, fails validation", () => {
  const run = syntheticRun();
  run.invocations[0].supervisor.containment = "sampled";
  failsWith(run, /without consent/);
  const run2 = syntheticRun();
  run2.invocations[0].supervisor.memLimitBytes = MEM_MB * 1024 * 1024;
  failsWith(run2, /containment cap/);
});

test("a reference measured on another source or by another method, or inputs that are not the catalog's, fail validation", () => {
  const stale = structuredClone(EXPECTED);
  stale.scenarios.synthetic.sourceSha256 = "other";
  assert.ok(
    validateRun(syntheticRun(), stale, [SCENARIO]).failures.some((f) =>
      /reference is stale/.test(f),
    ),
  );
  const method = structuredClone(EXPECTED);
  method.method.measuringSuffixSha256 = "other";
  assert.ok(
    validateRun(syntheticRun(), method, [SCENARIO]).failures.some((f) =>
      /different measuring method/.test(f),
    ),
  );
  // A cell measured by another method cannot be relabelled by the file's header.
  const relabelled = structuredClone(EXPECTED);
  relabelled.scenarios.synthetic.settings.strict.receipt.method = JSON.stringify({
    ...METHOD,
    launcher: "capped tsc wrapper",
    tscExeSha256: null,
  });
  assert.ok(
    validateRun(syntheticRun(), relabelled, [SCENARIO]).failures.some((f) =>
      /measured by another method/.test(f),
    ),
  );
  const unverified = structuredClone(EXPECTED);
  unverified.method.tscExeSha256 = null;
  unverified.scenarios.synthetic.settings.strict.receipt.method = JSON.stringify(unverified.method);
  assert.ok(
    validateRun(syntheticRun(), unverified, [SCENARIO]).failures.some((f) =>
      /verified tsc/.test(f),
    ),
  );
  const run = syntheticRun();
  run.meta.scenarios["synthetic/strict"].inputs["tsconfig.json"] = "other";
  failsWith(run, /tsconfig.json is not the catalog/);
});

test("after a warmup killed at the memory cap the rest of that arm may be skipped, and only then", () => {
  const run = syntheticRun();
  run.meta.options.skipAfterKill = true;
  const warmup = run.invocations.find((i) => i.arm === "verter" && i.warmup);
  warmup.supervisor = supervisorRecord({
    killedBy: "memory",
    exitCode: null,
    backend: "windows-job-object",
    killTriggerBytes: (MEM_MB + INFRA_MB) * 1024 * 1024,
  });
  warmup.supervisorExit = 137;
  warmup.phase = "cold";
  warmup.probe = null;
  for (const inv of run.invocations) {
    if (inv.arm === "verter" && !inv.warmup) {
      for (const key of [
        "command",
        "supervisorOut",
        "supervisorExit",
        "supervisor",
        "probeOut",
        "probe",
        "phase",
      ])
        delete inv[key];
      inv.skipped = {
        after: warmup.index,
        reason: "a warmup of this scenario and arm was killed at the memory cap",
      };
    }
  }
  resummarize(run);
  assert.deepEqual(validate(run).failures, []);
  assert.equal(run.summary.cells[0].arms.verter.class, "killed");
  assert.equal(run.summary.cells[0].headline, null);
  const run2 = syntheticRun();
  run2.meta.options.skipAfterKill = true;
  run2.invocations.find((i) => i.arm === "tsc-api" && !i.warmup).skipped = {
    after: 0,
    reason: "made up",
  };
  failsWith(resummarize(run2), /skipped without a warmup/);
  // A warmup killed without evidence that its engine exhausted the cap does not justify a skip.
  const run3 = structuredClone(run);
  run3.invocations.find((i) => i.arm === "verter" && i.warmup).supervisor.killTriggerBytes =
    MEM_MB * 1024 * 1024 - 1;
  failsWith(resummarize(run3), /skipped without a warmup/);
  // A tsc API memory kill is never the engine's, so it never justifies a skip.
  const run4 = syntheticRun();
  run4.meta.options.skipAfterKill = true;
  const tscWarmup = run4.invocations.find((i) => i.arm === "tsc-api" && i.warmup);
  tscWarmup.supervisor = supervisorRecord({ killedBy: "memory", exitCode: null });
  tscWarmup.supervisorExit = 137;
  tscWarmup.phase = "cold";
  tscWarmup.probe = null;
  run4.invocations.find((i) => i.arm === "tsc-api" && !i.warmup).skipped = {
    after: tscWarmup.index,
    reason: "memory",
  };
  failsWith(resummarize(run4), /skipped without a warmup/);
});

test("a raw record on disk that differs from results.json is reported", () => {
  const dir = mkdtempSync(join(tmpdir(), "semantic-perf-test-"));
  const run = syntheticRun();
  const inv = run.invocations[0];
  inv.supervisorOut = join(dir, "rep-0.sup.json");
  inv.probeOut = join(dir, "rep-0.probe.json");
  writeFileSync(inv.supervisorOut, JSON.stringify(inv.supervisor));
  writeFileSync(inv.probeOut, JSON.stringify(inv.probe));
  writeFileSync(
    `${inv.probeOut}.phase`,
    JSON.stringify({ phase: inv.phase, history: inv.phaseHistory }),
  );
  run.invocations = [inv];
  assert.deepEqual(rawFileProblems(run), []);
  writeFileSync(
    `${inv.probeOut}.phase`,
    JSON.stringify({ phase: "cold", history: inv.phaseHistory }),
  );
  assert.ok(rawFileProblems(run).some((p) => /phase marker/.test(p)));
  writeFileSync(
    `${inv.probeOut}.phase`,
    JSON.stringify({ phase: inv.phase, history: inv.phaseHistory }),
  );
  // A whole-program run's stdout is re-read from the file the supervisor recorded.
  const cliOut = join(dir, "cli.stdout.log");
  writeFileSync(cliOut, "Check time: 1.00s\n");
  const cliSup = { ...inv.supervisor, stdoutPath: cliOut };
  const cliInv = {
    ...structuredClone(inv),
    arm: "tsc-cli",
    supervisorOut: join(dir, "cli.sup.json"),
    supervisor: cliSup,
    probeOut: null,
    probe: null,
    phase: null,
    phaseHistory: null,
    cliStdout: "Check time: 1.00s\n",
  };
  writeFileSync(cliInv.supervisorOut, JSON.stringify(cliSup));
  const cliRun = { ...run, invocations: [cliInv] };
  assert.deepEqual(rawFileProblems(cliRun), []);
  writeFileSync(cliOut, "Check time: 9.00s\n");
  assert.ok(rawFileProblems(cliRun).some((p) => /whole-program stdout/.test(p)));
  // A record on disk that results.json omits is a disagreement too.
  const omitted = structuredClone(run);
  omitted.invocations[0].probe = null;
  assert.ok(rawFileProblems(omitted).some((p) => /probe record on disk/.test(p)));
  writeFileSync(inv.probeOut, JSON.stringify({ ...inv.probe, tool: "tsc" }));
  assert.ok(rawFileProblems(run).some((p) => /differs from results.json/.test(p)));
});

test("without a supervisor the harness refuses to run", () => {
  const empty = mkdtempSync(join(tmpdir(), "semantic-perf-root-"));
  assert.throws(() => resolveSupervisor(empty, null), /no process supervisor/);
  assert.throws(() => resolveSupervisor(empty, join(empty, "missing")), /does not exist/);
});

test("the timer resolution is calibrated: a coarse clock by its quantum, a fine one by the trivial request", () => {
  const coarse = syntheticRun();
  const res = timerResolution(coarse);
  // The fixture's calibration reads 0 half the time: coarse; its smallest positive server time is 0.5 ms.
  assert.equal(res.clock, "coarse");
  assert.equal(res.tscQuantumMs, 0.5);
  assert.equal(res.single, 1);
  const fine = syntheticRun();
  for (const inv of fine.invocations.filter((i) => i.arm === "tsc-api"))
    inv.probe.calibration.serverMs = Array(20).fill(0.004);
  const fr = timerResolution(fine);
  assert.equal(fr.clock, "fine");
  assert.equal(fr.tscQuantumMs, 0.004);
  const none = syntheticRun();
  for (const inv of none.invocations.filter((i) => i.arm === "tsc-api"))
    delete inv.probe.calibration;
  assert.equal(timerResolution(none).clock, "uncalibrated");
});

test("finalized infer declarations credit equivalent conditional union answers", () => {
  const single = "T extends [infer X] ? X : never";
  const union = `(${single}) | (T extends [infer Y] ? Y : never)`;
  assert.equal(canonicalType(union), canonicalType(single));
  const measured = interpretMeasurement({ printed: `[(${union})]`, codes: [] });
  assert.equal(classifyVerterAnswer(ANSWER(single), { reference: measured }).class, "matched");
  assert.equal(classifyVerterAnswer(ANSWER(union), { reference: REF(single) }).class, "matched");
  assert.notEqual(canonicalType(union), canonicalType("T extends [infer X] ? [X] : never"));
  assert.throws(
    () => canonicalType("T extends [infer X] | [infer Y] ? [X, Y] : never"),
    /ambiguous/,
  );
});

function cliFixture(arm = "tsc-cli") {
  const run = syntheticRun();
  run.meta.options.arms = [arm];
  const plan = schedule(["synthetic/strict"], [arm], 2, 1);
  run.meta.plan = plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`);
  run.invocations = plan.map((p, index) => ({
    index,
    scenario: "synthetic",
    setting: "strict",
    arm,
    rep: p.rep,
    warmup: p.warmup,
    command: [
      run.meta.typescript.exe,
      "-p",
      "/x/synthetic/strict/cli/tsconfig.json",
      "--extendedDiagnostics",
      ...(arm === "tsc-cli-1" ? ["--singleThreaded"] : []),
    ],
    supervisor: supervisorRecord({
      backend: "windows-job-object",
      peakMetric: "job-peak-commit-charge",
    }),
    supervisorExit: 0,
    cliStdout: "Check time: 1.00s\nTotal time: 1.00s\n",
  }));
  return resummarize(run);
}

test("CLI provenance binds the executable, project and thread mode even for killed children", () => {
  for (const arm of ["tsc-cli", "tsc-cli-1"]) {
    assert.deepEqual(validate(cliFixture(arm)).failures, []);
    // The project path spelled with either separator is the same project:
    // records reach validation from workers of either platform.
    const backslashed = cliFixture(arm);
    for (const inv of backslashed.invocations)
      inv.command = inv.command.map((a) => a.replace(/\//g, "\\"));
    assert.deepEqual(validate(backslashed).failures, []);
    for (const command of [
      ["/other/tsc", "-p", "/x/synthetic/strict/cli/tsconfig.json", "--extendedDiagnostics"],
      ["/tsc/tsc", "-p", "/other/tsconfig.json", "--extendedDiagnostics"],
      [
        "/tsc/tsc",
        "-p",
        "/x/synthetic/strict/cli/tsconfig.json",
        "--extendedDiagnostics",
        ...(arm === "tsc-cli" ? ["--singleThreaded"] : []),
      ],
    ]) {
      const run = cliFixture(arm);
      run.invocations[0].command = command;
      failsWith(run, /CLI command/);
      run.invocations[0].supervisor = supervisorRecord({
        killedBy: "memory",
        exitCode: null,
        backend: "linux-cgroup-v2",
      });
      run.invocations[0].supervisorExit = 137;
      failsWith(resummarize(run), /CLI command/);
    }
  }
});

test("CLI engine memory refuses cgroup and unknown accounting without losing answers", () => {
  const own = cliFixture();
  assert.equal(own.summary.cells[0].arms["tsc-cli"].peakBytes.n, 2);
  for (const overrides of [
    { backend: "linux-cgroup-v2", peakMetric: "cgroup-memory.peak" },
    { backend: "windows-job-object", peakMetric: "cgroup-memory.peak" },
    { backend: "unknown" },
  ]) {
    const run = cliFixture();
    for (const inv of run.invocations)
      Object.assign(inv.supervisor, overrides, { peakBytes: 1e12 });
    resummarize(run);
    assert.deepEqual(validate(run).failures, []);
    const cell = run.summary.cells[0].arms["tsc-cli"];
    assert.equal(cell.peakBytes, null);
    assert.equal(cell.peakMetric, null);
    assert.equal(cell.status, "completed");
    assert.equal(cell.memoryUnavailable, "supervisor accounting is not attributable to the engine");
  }
  // A partially attributable cell is refused as a whole: the statistic would
  // span an unknown subset of the cell's completed invocations, and a budget
  // verdict over that subset is not the cell's either.
  for (const last of [true, false]) {
    const run = cliFixture();
    const measured = run.invocations.filter((i) => !i.warmup);
    const attributable = measured[last ? 0 : 1];
    attributable.supervisor.peakBytes = (MEM_MB + 1) * 1024 * 1024;
    Object.assign(measured[last ? 1 : 0].supervisor, {
      backend: "linux-cgroup-v2",
      peakMetric: "cgroup-memory.peak",
      peakBytes: 1e12,
    });
    resummarize(run);
    assert.deepEqual(validate(run).failures, []);
    const cell = run.summary.cells[0].arms["tsc-cli"];
    assert.equal(cell.peakBytes, null);
    assert.equal(cell.peakMetric, null);
    assert.equal(cell.status, "completed");
    assert.equal(cell.memoryUnavailable, "supervisor accounting is not attributable to the engine");
  }
  // The published side of the same gate: a fully attributable cell whose
  // engine passed its memory budget carries the verdict, not a refusal.
  const over = cliFixture();
  for (const inv of over.invocations) inv.supervisor.peakBytes = (MEM_MB + 1) * 1024 * 1024;
  resummarize(over);
  assert.deepEqual(validate(over).failures, []);
  const verdict = over.summary.cells[0].arms["tsc-cli"];
  assert.equal(verdict.status, "over the engine budget");
  assert.equal(verdict.memoryUnavailable, null);
  assert.equal(verdict.peakBytes.n, 2);
  assert.equal(verdict.peakMetric, "job-peak-commit-charge");
});

test("CLI diagnostics beyond one MiB survive storage and raw verification", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "semantic-cli-full-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const text =
    "padding\n".repeat(80000) +
    "scenario.ts(1,1): error TS2589: deep\n" +
    "padding\n".repeat(80000) +
    "Check time: 1.00s\nTotal time: 1.00s\n";
  const run = cliFixture();
  const inv = run.invocations[0];
  inv.supervisorOut = join(dir, "cli.sup.json");
  inv.supervisor.stdoutPath = join(dir, "stdout.log");
  inv.cliStdout = text;
  writeFileSync(inv.supervisorOut, JSON.stringify(inv.supervisor));
  writeFileSync(inv.supervisor.stdoutPath, text);
  run.invocations = [inv];
  assert.deepEqual(rawFileProblems(run), []);
  assert.deepEqual(parseCli(inv.cliStdout).codes, [2589]);
  writeFileSync(inv.supervisor.stdoutPath, text.replace("TS2589", "TS2590"));
  assert.ok(rawFileProblems(run).some((p) => /whole-program stdout/.test(p)));
});

test("environment receipts require SHA-256 digests in tuned and constructed runs", () => {
  for (const allowTuning of [false, true]) {
    for (const path of ["runtime", "build"]) {
      for (const bad of [
        "",
        "x".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        ["a".repeat(64)],
        null,
      ]) {
        const run = syntheticRun();
        run.meta.options.allowTuning = allowTuning;
        run.meta.environment.inherited = allowTuning;
        const receipt =
          path === "runtime" ? run.meta.environment.runtime : run.meta.build.environment;
        receipt.valuesSha256 = bad;
        failsWith(run, /environment receipts/);
      }
    }
  }
});

test("the real quick catalog and invocation manifest validate and reject a missing cell", () => {
  const scenarios = scenariosForTier("quick");
  assert.ok(scenarios.length > 0);
  const run = withObserveBuild(syntheticRun());
  Object.assign(run.meta.options, TIER_DEFAULTS.quick);
  run.meta.options.only = [];
  const keys = scenarios.map((s) => `${s.id}/strict`);
  const plan = schedule(
    keys,
    run.meta.options.arms,
    run.meta.options.repeat,
    run.meta.options.warmup,
  );
  run.meta.plan = plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`);
  const expected = structuredClone(EXPECTED);
  expected.scenarios = {};
  run.meta.scenarios = {};
  for (const s of scenarios) {
    const dir = `/quick/${s.id}/strict`;
    run.meta.scenarios[`${s.id}/strict`] = {
      id: s.id,
      setting: "strict",
      dir,
      inputs: {
        ...INPUTS,
        "scenario.ts": sha256Text(s.source),
        "cli/scenario.ts": sha256Text(cliSource(s)),
      },
    };
    expected.scenarios[s.id] = {
      sourceSha256: sha256Text(s.source),
      settings: {
        strict: {
          ...RAW_ONE,
          receipt: { ...RAW_ONE.receipt, sourceSha256: sha256Text(s.source + MEASURING_SUFFIX) },
        },
      },
    };
  }
  run.invocations = plan.map((p, index) => {
    const [scenario, setting] = p.key.split("/");
    const warm = !p.warmup && p.rep === 0;
    const probe = p.arm === "tsc-api" ? tscProbe("1") : verterProbe("1", p.arm);
    probe.probes[0].warm = warm
      ? Array.from({ length: run.meta.options.warmRepeats }, () =>
          structuredClone(probe.probes[0].warm[0]),
        )
      : [];
    if (p.arm === "tsc-api")
      probe.rootFiles = [
        join(run.meta.scenarios[p.key].dir, "lib.bench.d.ts"),
        join(run.meta.scenarios[p.key].dir, "scenario.ts"),
      ];
    const fixture = syntheticRun().invocations.find(
      (i) => i.arm === (p.arm === "tsc-api" ? "tsc-api" : "verter"),
    );
    return {
      ...fixture,
      supervisor: supervisorRecord({
        timeoutMs: run.meta.options.timeoutMs + run.meta.options.startupAllowanceMs,
      }),
      index,
      scenario,
      setting,
      arm: p.arm,
      rep: p.rep,
      warmup: p.warmup,
      probe,
      command: [
        {
          "verter-counted": "/bin/counted",
          "verter-observe": "/bin/observe",
          "tsc-api": "node",
        }[p.arm] ?? "/bin/probe",
      ],
    };
  });
  addSessions(run, sessionsFor("quick", TIERS, []));
  run.summary = summarize(run, expected, scenarios);
  assert.deepEqual(validateRun(run, expected, scenarios).failures, []);
  // AC: the quick tier holds the INCREMENTAL, EDITOR SESSION and CONCURRENT
  // workloads with answers validated, a Capacity row per cell, and the
  // observe build's comparison.
  assert.deepEqual(run.summary.sessions.cells.map((c) => c.family).sort(), [
    "concurrent",
    "editor",
    "incremental",
  ]);
  for (const cell of run.summary.sessions.cells) {
    assert.equal(cell.arms.verter.class, "matched", cell.key);
    assert.equal(cell.arms["verter-observe"].class, "matched", cell.key);
    assert.equal(cell.arms["tsc-api"].class, "reference", cell.key);
    assert.equal(cell.requiredState.state, "identical", cell.key);
  }
  assert.equal(run.summary.cells.length, scenarios.length);
  for (const cell of run.summary.cells) {
    assert.ok(cell.capacity?.verter && cell.capacity?.tscApi, `${cell.key} has a capacity row`);
    assert.equal(cell.observeBuild?.requiredState?.state, "identical", cell.key);
  }
  const report = renderMarkdown({ ...run, validation: { ok: true, failures: [], warnings: [] } });
  for (const section of ["## Session workloads", "## Capacity", "## Observe build"])
    assert.ok(report.includes(section), `the report has ${section}`);
  // Program roots spelled with the platform's separators are the same
  // program: the probe normalises to forward slashes, and validation must
  // not depend on which spelling a record carries.
  for (const inv of run.invocations)
    if (Array.isArray(inv.probe?.rootFiles))
      inv.probe.rootFiles = inv.probe.rootFiles.map((f) => f.replace(/\//g, "\\"));
  assert.deepEqual(validateRun(run, expected, scenarios).failures, []);
  delete run.meta.scenarios[keys[0]];
  assert.ok(
    validateRun(run, expected, scenarios).failures.some((f) => /recorded scenarios/.test(f)),
  );
});

// ---------------------------------------------------------------- session workloads

const verterObservation = (text) => ({
  text,
  error: null,
  shape: "union",
  unionMembers: null,
  unknownLeaves: 0,
  unknownSamples: [],
  conditionalNodes: 0,
});
const RETENTION = {
  semanticNodes: 5,
  semanticMemoEntries: 4,
  unionViews: 1,
  shapeCacheEntries: 0,
  activeBytes: 0,
  retainedBytes: 100,
  pinnedBytes: 0,
  peakTotalBytes: 120,
};
const isConcurrent = (step) => Boolean(step.concurrent && step.requests.length > 1);

/** A Verter session record answering `session`'s script with its constructed answers (or `answers[step/request]`). */
function verterSessionRecord(session, { arm = "verter", answers = {} } = {}) {
  return {
    schema: 1,
    tool: "verter",
    kind: "session",
    observability: false,
    captureAvailable: arm === "verter-observe",
    stage: "complete",
    pid: 1,
    phases: { engineStart: 10, setup: 100, init: 50 },
    initOutcome: { kind: "value" },
    steps: session.steps.map((step, s) => {
      if (step.kind === "edit")
        return {
          kind: "edit",
          file: step.file,
          textSha256: sha256Text(step.text),
          micros: 20,
        };
      if (step.kind === "meta")
        return {
          kind: "meta",
          file: step.file,
          micros: 30,
          outcome: { kind: "value" },
          surface: structuredClone(step.expect),
        };
      return {
        kind: "demand",
        concurrent: isConcurrent(step),
        threads: isConcurrent(step) ? step.requests.length : 1,
        wallMicros: 100,
        requests: step.requests.map((r, k) => ({
          file: r.file,
          alias: r.alias,
          micros: 50,
          outcome: { kind: "value" },
          observation: verterObservation(answers[`${s}/${k}`] ?? r.expect),
        })),
      };
    }),
    afterSteps: {
      pid: 1,
      metric: "private-commit",
      peakBytes: 2000,
      currentBytes: 1500,
      cpuMicros: 1,
    },
    statsErrors: [],
    retention: { ...RETENTION },
  };
}

/** A tsc session record: answers, and the API snapshot evidence of program reuse. */
function tscSessionRecord(session, projectDir, { answers = {} } = {}) {
  let snapshot = 1;
  return {
    schema: 1,
    tool: "tsc",
    kind: "session",
    stage: "complete",
    incremental: INCREMENTAL_FACILITY,
    tscExe: "/tsc/tsc",
    statsExe: "/bin/probe",
    serverPid: 2,
    rootFiles: ["lib.bench.d.ts", ...tscFiles(session)].map((f) => `${projectDir}/${f}`),
    phases: { spawnMs: 1, setupMs: 2, setupRoundTripMs: 3, initMs: 1, initRoundTripMs: 2 },
    initOutcome: { kind: "value" },
    steps: session.steps.map((step, s) => {
      if (step.kind === "meta") return { kind: "meta", file: step.file, applicable: false };
      if (step.kind === "edit")
        return {
          kind: "edit",
          file: step.file,
          textSha256: sha256Text(step.text),
          serverMs: 0.3,
          roundTripMs: 1,
          snapshot: ++snapshot,
          fileChanges: { changed: [`${projectDir}/${step.file}`] },
        };
      return {
        kind: "demand",
        concurrent: isConcurrent(step),
        threads: isConcurrent(step) ? step.requests.length : 1,
        basis: isConcurrent(step) ? "round-trip" : "server",
        wallMs: 2,
        requests: step.requests.map((r, k) => ({
          file: r.file,
          alias: r.alias,
          serverMs: isConcurrent(step) ? null : 1,
          outcome: { kind: "value" },
          observation: {
            text: answers[`${s}/${k}`] ?? r.expect,
            error: null,
            errorType: false,
            unionMembers: null,
          },
        })),
      };
    }),
    serverAfterSteps: {
      pid: 2,
      metric: "private-commit",
      peakBytes: 3000,
      currentBytes: 2500,
      cpuMicros: 1,
    },
    statsErrors: [],
  };
}

/** Add `sessions`' records to `run` in its counterbalanced session plan. */
function addSessions(run, sessions) {
  const o = run.meta.options;
  run.meta.sessions = Object.fromEntries(
    sessions.map((session) => [
      session.id,
      {
        id: session.id,
        family: session.family,
        note: session.note,
        dir: `/s/${session.id}`,
        inputs: sessionInputs(session, LIB_TEXT),
      },
    ]),
  );
  const plan = schedule(
    sessions.map((x) => x.id),
    sessionArmsOf(o.arms),
    o.repeat,
    o.warmup,
  );
  run.meta.sessionPlan = plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`);
  const byId = new Map(sessions.map((x) => [x.id, x]));
  run.sessionInvocations = plan.map((p, index) => {
    const session = byId.get(p.key);
    const projectDir = `/s/${p.key}/${p.arm}/${p.warmup ? "w" : "r"}${p.rep}/project`;
    const sessionOut = `/nonexistent/s${index}.session.json`;
    return {
      index,
      sessionId: p.key,
      arm: p.arm,
      rep: p.rep,
      warmup: p.warmup,
      command:
        p.arm === "tsc-api"
          ? [
              NODE,
              TSC_SESSION_DRIVER,
              "--job",
              `/nonexistent/s${index}.job.json`,
              "--out",
              sessionOut,
            ]
          : [p.arm === "verter-observe" ? "/bin/observe" : "/bin/probe", "session"],
      projectDir,
      supervisorOut: `/nonexistent/s${index}.sup.json`,
      supervisorExit: 0,
      supervisor: supervisorRecord({ timeoutMs: o.timeoutMs + o.startupAllowanceMs }),
      sessionOut,
      spawnedAtMs: 1000,
      phase: "done",
      phaseHistory: [{ phase: "done", atMs: 1010 }],
      session:
        p.arm === "tsc-api"
          ? tscSessionRecord(session, projectDir)
          : verterSessionRecord(session, { arm: p.arm }),
    };
  });
  return run;
}

/** The run's observe build: the probe with semantic-observe compiled in. */
function withObserveBuild(run) {
  run.meta.binaries.observe = {
    sha256: "o",
    pinned: "/bin/observe",
    identity: {
      debugAssertions: false,
      instrumented: false,
      captureAvailable: true,
      targetArch: "x86_64",
      nativeArch: "x86_64",
    },
  };
  run.meta.binariesAfter.observe = "o";
  const packages = structuredClone(run.meta.build.packages);
  packages.verter_bench.features = ["attribution", "currency_probe", "hotpath", "semantic-observe"];
  packages.verter_audit.features = ["attribution", "semantic-observe"];
  run.meta.observeBuild = { ...run.meta.build, observe: true, packages };
  return run;
}

/** A synthetic run with the incremental session (verter and tsc-api arms). */
function sessionRun() {
  const run = syntheticRun();
  run.meta.options.only = ["synthetic", "incremental-edits"];
  addSessions(run, sessionsFor("quick", TIERS, run.meta.options.only));
  return resummarize(run);
}

/** A synthetic run whose session runs the production and the observe build. */
function observeSessionRun() {
  const run = withObserveBuild(syntheticRun({ arms: ["verter", "verter-observe"] }));
  run.meta.options.only = ["synthetic", "incremental-edits"];
  addSessions(run, sessionsFor("quick", TIERS, run.meta.options.only));
  return resummarize(run);
}
const INCREMENTAL = allSessions().find((x) => x.id === "incremental-edits");

test("the session catalog is well-formed and its workloads sit in the quick tier", () => {
  const sessions = allSessions();
  assert.deepEqual(sessions.map((x) => x.family).sort(), ["concurrent", "editor", "incremental"]);
  for (const session of sessions) {
    assert.deepEqual(sessionProblems(session), [], session.id);
    assert.equal(SESSION_TIERS[session.id], "quick");
  }
  // The incremental script edits a leaf, an intermediate type and an
  // unrelated file, re-requesting after each; the editor session demands
  // component metadata; the concurrent one issues its demands at once.
  assert.deepEqual(
    INCREMENTAL.steps.filter((x) => x.kind === "edit").map((x) => x.file),
    ["leaf.ts", "mid.ts", "unrelated.ts"],
  );
  assert.ok(sessions.find((x) => x.family === "editor").steps.some((x) => x.kind === "meta"));
  assert.ok(sessions.find((x) => x.family === "concurrent").steps.every((x) => isConcurrent(x)));
  // A malformed construction is refused.
  const broken = structuredClone(INCREMENTAL);
  broken.steps[1].text = broken.files["leaf.ts"];
  broken.steps[0].requests[0].alias = "__Missing";
  const problems = sessionProblems(broken);
  assert.ok(problems.some((p) => /changes nothing/.test(p)));
  assert.ok(problems.some((p) => /declares __Missing 0 times/.test(p)));
});

test("a session run validates; tsc contradicting a constructed answer fails it, a wrong Verter answer is a finding", () => {
  const run = sessionRun();
  assert.deepEqual(validate(run).failures, []);
  const cell = run.summary.sessions.cells[0];
  assert.equal(cell.arms.verter.class, "matched");
  // Every edit and every re-request after it is compared.
  assert.deepEqual(
    cell.comparison.map((c) => `${c.step}:${c.kind}`),
    ["0:demand", "1:edit", "2:demand", "3:edit", "4:demand", "5:edit", "6:demand"],
  );

  const noopEdit = sessionRun();
  const verterInv = noopEdit.sessionInvocations.find((i) => i.arm === "verter");
  verterInv.session.steps.find((x) => x.file === "unrelated.ts").textSha256 = sha256Text("");
  assert.ok(validate(noopEdit).failures.some((x) => /installed text other than/.test(x)));

  const wrongTsc = sessionRun();
  wrongTsc.sessionInvocations.find((i) => i.arm === "tsc-api").session = tscSessionRecord(
    INCREMENTAL,
    wrongTsc.sessionInvocations.find((i) => i.arm === "tsc-api").projectDir,
    { answers: { "2/0": '1 | "m" | "mid0"' } },
  );
  failsWith(resummarize(wrongTsc), /wrong answer against the constructed answer/);

  const wrongVerter = sessionRun();
  for (const inv of wrongVerter.sessionInvocations.filter((i) => i.arm === "verter"))
    inv.session = verterSessionRecord(INCREMENTAL, { answers: { "2/0": '1 | "m" | "mid0"' } });
  resummarize(wrongVerter);
  assert.deepEqual(validate(wrongVerter).failures, []);
  const verter = wrongVerter.summary.sessions.cells[0].arms.verter;
  assert.equal(verter.class, "mismatch");
  assert.equal(verter.demands.find((d) => d.step === 2).class, "mismatch");
  // The stale answer after the leaf edit is never compared.
  assert.ok(!wrongVerter.summary.sessions.cells[0].comparison.some((c) => c.step === 2));
});

test("tsc's incremental arm must reuse the API program: each edit names its file and advances the snapshot", () => {
  const noReuse = sessionRun();
  const inv = noReuse.sessionInvocations.find((i) => i.arm === "tsc-api");
  inv.session.steps[1].fileChanges = { invalidateAll: true };
  failsWith(noReuse, /did not name the edited file/);

  const stale = sessionRun();
  const staleInv = stale.sessionInvocations.find((i) => i.arm === "tsc-api");
  staleInv.session.steps[3].snapshot = staleInv.session.steps[1].snapshot;
  failsWith(stale, /did not advance the API snapshot/);

  const other = sessionRun();
  other.sessionInvocations.find((i) => i.arm === "tsc-api").session.incremental = "fresh-program";
  failsWith(other, /incremental facility/);
});

test("session records must follow the script, and the stored session summary must match them", () => {
  const skipped = sessionRun();
  skipped.sessionInvocations.find((i) => i.arm === "verter").session.steps.pop();
  failsWith(resummarize(skipped), /steps recorded, the script has/);

  const missing = sessionRun();
  missing.sessionInvocations.pop();
  failsWith(missing, /missing session record/);

  const claimed = sessionRun();
  for (const inv of claimed.sessionInvocations.filter((i) => i.arm === "verter"))
    inv.session = verterSessionRecord(INCREMENTAL, { answers: { "0/0": "never" } });
  // The stored summary still claims the match.
  failsWith(claimed, /stored session summary disagrees/);

  const observe = sessionRun();
  observe.sessionInvocations.find((i) => i.arm === "verter").session.captureAvailable = true;
  failsWith(observe, /capture availability/);
});

test("a session's engine statistics must be of the process the run measured", () => {
  const verter = sessionRun();
  verter.sessionInvocations.find((i) => i.arm === "verter").session.afterSteps.pid = 777;
  failsWith(verter, /statistics are not of the session's own process/);

  const tsc = sessionRun();
  tsc.sessionInvocations.find((i) => i.arm === "tsc-api").session.serverAfterSteps.pid = 777;
  failsWith(tsc, /statistics are not of the session's own process/);

  const noPid = sessionRun();
  noPid.sessionInvocations.find((i) => i.arm === "verter").session.pid = 12.5;
  failsWith(noPid, /statistics are not of the session's own process/);
});

test("the tsc session arm's provenance is bound to the command that ran it", () => {
  const otherNode = sessionRun();
  otherNode.sessionInvocations.find((i) => i.arm === "tsc-api").command[0] = "/other/node";
  failsWith(otherNode, /not the harness's node on its own tsc session driver/);

  const otherDriver = sessionRun();
  otherDriver.sessionInvocations.find((i) => i.arm === "tsc-api").command[1] =
    "/elsewhere/driver.mjs";
  failsWith(otherDriver, /not the harness's node on its own tsc session driver/);

  const otherJob = sessionRun();
  otherJob.sessionInvocations.find((i) => i.arm === "tsc-api").command[3] = "/other/job.json";
  failsWith(otherJob, /not the harness's node on its own tsc session driver/);

  const otherTsc = sessionRun();
  otherTsc.sessionInvocations.find((i) => i.arm === "tsc-api").session.tscExe = "/WHATEVER/tsc";
  failsWith(otherTsc, /not the verified tsc/);

  const otherStats = sessionRun();
  otherStats.sessionInvocations.find((i) => i.arm === "tsc-api").session.statsExe =
    "/bin/other-stats";
  failsWith(otherStats, /not the pinned probe/);
});

test("a session's recorded inputs must be the catalog's, the benchmark library's included", () => {
  const run = sessionRun();
  const id = Object.keys(run.meta.sessions)[0];
  run.meta.sessions[id].inputs["lib.bench.d.ts"] = sha256Text("TOTALLY-DIFFERENT-LIB");
  failsWith(run, /the library is not the benchmark's/);
});

test("the observe arm must retain the production build's REQUIRED state", () => {
  const run = observeSessionRun();
  assert.deepEqual(validate(run).failures, []);
  assert.equal(run.summary.sessions.cells[0].requiredState.state, "identical");

  const differs = observeSessionRun();
  for (const inv of differs.sessionInvocations.filter(
    (i) => i.arm === "verter-observe" && !i.warmup,
  ))
    inv.session.retention.retainedBytes = 999999;
  failsWith(resummarize(differs), /REQUIRED state differs/);

  // An arm with no completed measured invocation leaves nothing to compare:
  // reported n/a and warned, never a silent pass.
  const none = observeSessionRun();
  for (const inv of none.sessionInvocations.filter(
    (i) => i.arm === "verter-observe" && !i.warmup,
  )) {
    inv.supervisor = supervisorRecord({ killedBy: "memory", exitCode: null });
    inv.session = null;
    inv.phase = "step-1";
  }
  resummarize(none);
  const checked = validate(none);
  assert.deepEqual(checked.failures, []);
  assert.equal(none.summary.sessions.cells[0].requiredState.state, "n/a");
  assert.ok(checked.warnings.some((w) => /REQUIRED-state comparison is n\/a/.test(w)));
});

test("a session killed after writing its complete record keeps its measurement; a kill inside a step is the engine's", () => {
  const killed = sessionRun();
  const inv = killed.sessionInvocations.find((i) => i.arm === "verter" && !i.warmup);
  inv.supervisor = supervisorRecord({ killedBy: "memory", exitCode: null });
  inv.supervisorExit = 137;
  inv.phase = "stats";
  resummarize(killed);
  assert.deepEqual(validate(killed).failures, []);
  const arm = killed.summary.sessions.cells[0].arms.verter;
  assert.equal(arm.class, "matched");
  assert.equal(arm.completedMeasured, 2);
  assert.equal(arm.metrics.peakBytes.median, 2000);
  assert.ok(arm.ends.includes("observe-killed"));

  const mid = sessionRun();
  const midInv = mid.sessionInvocations.find((i) => i.arm === "verter" && !i.warmup);
  midInv.supervisor = supervisorRecord({
    killedBy: "memory",
    exitCode: null,
    backend: "windows-job-object",
    killTriggerBytes: MEM_MB * 1024 * 1024,
  });
  midInv.supervisorExit = 137;
  midInv.phase = "step-1";
  midInv.session = null;
  resummarize(mid);
  assert.deepEqual(validate(mid).failures, []);
  assert.equal(mid.summary.sessions.cells[0].arms.verter.class, "killed");
});

test("a component's metadata is matched only on equal prop names, required flags and events", () => {
  const expect = {
    props: [
      { name: "items", required: true },
      { name: "size", required: false },
    ],
    events: ["focus"],
  };
  const step = (surface) => ({ kind: "meta", outcome: { kind: "value" }, surface });
  assert.equal(classifyMeta(step(structuredClone(expect)), expect).class, "matched");
  const reordered = { props: [...expect.props].reverse(), events: ["focus"] };
  assert.equal(classifyMeta(step(reordered), expect).class, "matched");
  const required = structuredClone(expect);
  required.props[1].required = true;
  assert.equal(classifyMeta(step(required), expect).class, "mismatch");
  assert.equal(classifyMeta(step({ ...expect, events: [] }), expect).class, "mismatch");
  assert.equal(
    classifyMeta({ kind: "meta", outcome: { kind: "fault", detail: "x" } }, expect).class,
    "error",
  );
});

test("the observe arm's build carries semantic-observe, the production probe never does, and both retain one REQUIRED state", () => {
  const run = withObserveBuild(syntheticRun());
  assert.deepEqual(buildProblems(run.meta.observeBuild), []);
  // The gate itself is required of the observe build ...
  const gateless = structuredClone(run.meta.observeBuild);
  gateless.packages.verter_bench.features = ["attribution"];
  assert.ok(buildProblems(gateless).some((p) => /lacks semantic-observe/.test(p)));
  // ... and refused in the production build.
  const production = structuredClone(run.meta.build);
  production.packages.verter_audit.features = ["semantic-observe"];
  assert.ok(buildProblems(production).some((p) => /semantic-observe/.test(p)));
  const captured = syntheticRun();
  captured.meta.binaries.probe.identity.captureAvailable = true;
  failsWith(captured, /capture compiled in/);

  const a = { ...RETENTION };
  assert.equal(requiredStateComparison([a, a], [{ ...a }]).state, "identical");
  // Histories and peaks are optional state: they may differ.
  assert.equal(requiredStateComparison([a], [{ ...a, peakTotalBytes: 999 }]).state, "identical");
  const differs = requiredStateComparison([a], [{ ...a, semanticNodes: 6 }]);
  assert.equal(differs.state, "differs");
  assert.deepEqual(differs.fields, ["semanticNodes"]);
  assert.equal(
    requiredStateComparison([a, { ...a, unionViews: 2 }], [a]).state,
    "nondeterministic",
  );
});

test("the Capacity row reports each arm's outcome, and a kill's tree peak and time to it", () => {
  const run = syntheticRun();
  const inv = firstOf(run, "tsc-api");
  inv.supervisor = supervisorRecord({
    killedBy: "memory",
    exitCode: null,
    peakBytes: 9e9,
    wallMs: 4321,
    timeoutMs: 1500,
  });
  inv.supervisorExit = 137;
  inv.phase = "cold";
  inv.probe = null;
  resummarize(run);
  const row = run.summary.cells[0].capacity;
  assert.equal(row.budgetBytes, MEM_MB * 1024 * 1024);
  assert.equal(row.verter.outcome, "matched");
  assert.equal(row.tscApi.kills, 1);
  // A tsc API tree's memory kill is never attributed to the engine.
  assert.equal(row.tscApi.attributedKills, 0);
  assert.equal(row.tscApi.peakAtKillBytes.median, 9e9);
  assert.equal(row.tscApi.timeToKillMs.median, 4321);
  assert.deepEqual(row.tscApi.killedBy, ["memory"]);
});

test("a session invocation whose supervisor file is unreadable is reported", () => {
  const run = {
    sessionInvocations: [
      {
        sessionId: "s",
        arm: "verter",
        warmup: false,
        rep: 0,
        supervisorOut: join(tmpdir(), "semantic-perf-absent-supervisor.json"),
        sessionOut: join(tmpdir(), "semantic-perf-absent-session.json"),
        supervisor: null,
        session: null,
      },
    ],
    invocations: [],
  };
  assert.ok(rawFileProblems(run).some((p) => /cannot read .*absent-supervisor/.test(p)));
});
