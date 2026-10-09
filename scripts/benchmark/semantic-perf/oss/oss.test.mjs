// Self-tests of the opt-in comparisons (`--oss`, `--biome`): the pinned
// manifest, provisioning's bookkeeping, reading and classifying each tool's
// answer, the thenable catalog and its measured reference, and the
// sections' validation on synthetic runs. No tool, cargo build or tsc
// process is needed.
//
//   node --test scripts/benchmark/semantic-perf/oss/oss.test.mjs

import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { canonicalDigest, normalize, parseType } from "../canonical.mjs";
import { MEASURING_SUFFIX } from "../measure-expected.mjs";
import { sha256Text } from "../provenance.mjs";
import { parseArgs, schedule } from "../run.mjs";
import { moduleText, SETTINGS, tsconfigText } from "../scenarios.mjs";
import { classifyOssAnswer, readOssAnswer, tscMeasureStatus } from "./answers.mjs";
import {
  BIOME_ARMS,
  biomeArms,
  biomeInvocationClass,
  expectedFor,
  readBiomeAnswer,
  summarizeBiome,
  validateBiome,
} from "./biome.mjs";
import {
  measureSource,
  ossArm,
  ossArms,
  OSS_SCHEMA,
  VERTER_ARM,
  readInvocation,
  REFERENCE_ARMS,
  renderOssMarkdown,
  selectCheckers,
  summarizeOss,
  validateOss,
} from "./checkers.mjs";
import { binaryPath, loadTools, provenanceProblems, provisioned, recipeFor } from "./provision.mjs";
import {
  BIOME_DEMAND,
  demandLine,
  isThenable,
  loadThenableExpected,
  thenableCases,
} from "./thenable.mjs";

const SOURCE = moduleText("", "1") + MEASURING_SUFFIX;
const LINES = SOURCE.split("\n");
const MARKER_LINE = LINES.findIndex((l) => l.includes("const __bench_s")) + 1;
const NEVER_LINE = LINES.findIndex((l) => l.includes("const __bench_n")) + 1;
/** A tsc-format measuring output printing `tuple` (e.g. `[[1]]`) and the never verdict. */
const tscOut = (tuple, { never = "no", extra = "" } = {}) =>
  extra +
  (tuple
    ? `scenario.ts(${MARKER_LINE},7): error TS2322: Type '${tuple}' is not assignable to type '[never]'.\n`
    : "") +
  `scenario.ts(${NEVER_LINE},7): error TS2322: Type '"${never}"' is not assignable to type '"never-check"'.\n`;
const exited = (exitCode = 1) => ({ kind: "exited", exitCode });
const reference = (text, codes = [], errorAny = false) => ({
  digest: canonicalDigest(text),
  errorAny,
  codes,
});

// ---------------------------------------------------------------- the pinned manifest

test("every pinned tool names an exact commit, a license and a verified source per platform", () => {
  const tools = loadTools();
  assert.deepEqual(Object.keys(tools).sort(), ["bamtiscript", "biome", "ezno", "tsrust", "tsz"]);
  for (const [id, tool] of Object.entries(tools)) {
    assert.match(tool.commit, /^[0-9a-f]{40}$/, `${id} commit`);
    assert.ok(tool.pin && tool.license && tool.project, `${id} pin, license, project`);
    assert.ok(tool.smoke?.argv?.length && tool.smoke.expect, `${id} smoke check`);
    for (const platform of ["win32-x64", "darwin-arm64"]) {
      const recipe = recipeFor(tool, platform);
      assert.ok(!recipe.unavailable, `${id} has a recipe for ${platform}`);
      if (recipe.source.type === "git") {
        assert.equal(recipe.source.commit, tool.commit, `${id} builds its pinned commit`);
        assert.ok(recipe.build?.length, `${id} names its build`);
      } else assert.match(recipe.source.sha256, /^[0-9a-f]{64}$/, `${id} ${platform} sha256`);
    }
    if (tool.kind === "checker")
      assert.ok(tool.argv.length && tool.format, `${id} argv and format`);
  }
});

test("a platform without a recipe, or a tool never provisioned, is unavailable with its reason", () => {
  const tools = loadTools();
  const r = recipeFor(tools.tsz, "linux-riscv64");
  assert.match(r.unavailable, /no pinned release or source build for linux-riscv64/);
  assert.equal(recipeFor(tools.bamtiscript, "linux-riscv64").source.type, "git");
  const root = mkdtempSync(join(tmpdir(), "oss-prov-"));
  const p = provisioned(root, "tsz", tools.tsz, "win32-x64");
  assert.match(p.unavailable, /^unavailable on win32-x64: not provisioned/);
  assert.match(
    provisioned(root, "tsz", tools.tsz, "linux-riscv64").unavailable,
    /^unavailable on linux-riscv64: /,
  );
  assert.equal(binaryPath("d", { binary: "t/bin{exe}" }, "win32-x64"), join("d", "t/bin.exe"));
  assert.equal(binaryPath("d", { binary: "t/bin{exe}" }, "darwin-arm64"), join("d", "t/bin"));
});

test("a provisioning record for another pin, platform or source is never used", () => {
  const tools = loadTools();
  const good = {
    schema: 1,
    tool: "tsz",
    pin: tools.tsz.pin,
    commit: tools.tsz.commit,
    platform: "win32-x64",
    source: recipeFor(tools.tsz, "win32-x64").source,
    binary: { sha256: "a".repeat(64) },
  };
  assert.deepEqual(provenanceProblems(good, "tsz", tools.tsz, "win32-x64"), []);
  assert.match(
    provenanceProblems({ ...good, pin: "v0.0.1" }, "tsz", tools.tsz, "win32-x64")[0],
    /provisioned v0\.0\.1/,
  );
  assert.match(
    provenanceProblems(good, "tsz", tools.tsz, "darwin-arm64").join(";"),
    /provisioned for win32-x64/,
  );
  assert.match(
    provenanceProblems(
      { ...good, source: { ...good.source, sha256: "b".repeat(64) } },
      "tsz",
      tools.tsz,
      "win32-x64",
    ).join(";"),
    /another source/,
  );
});

// ---------------------------------------------------------------- the CLI

test("--oss and --biome are opt-in: a default run's options are unchanged", () => {
  const plain = parseArgs([]);
  assert.equal("oss" in plain, false);
  assert.equal("biome" in plain, false);
  // Every tool: the checkers and Biome's own section.
  const all = parseArgs(["--oss"]);
  assert.deepEqual([all.oss, all.biome], [[], true]);
  const two = parseArgs(["--oss", "tsz,ezno"]);
  assert.deepEqual([two.oss, two.biome], [["tsz", "ezno"], undefined]);
  const mixed = parseArgs(["--oss", "tsz,biome"]);
  assert.deepEqual([mixed.oss, mixed.biome], [["tsz"], true]);
  const biomeOnly = parseArgs(["--oss", "biome"]);
  assert.deepEqual(["oss" in biomeOnly, biomeOnly.biome], [false, true]);
  const alias = parseArgs(["--biome"]);
  assert.deepEqual(["oss" in alias, alias.biome], [false, true]);
  const both = parseArgs(["--oss", "--biome"]);
  assert.deepEqual([both.oss, both.biome], [[], true]);
  assert.throws(() => parseArgs(["--oss", "no-such-tool"]), /not a pinned tool/);
  // --no-tsc drops every tsc arm from the tier's arms, and refuses one named explicitly.
  assert.ok(plain.arms.includes("tsc-api") && !("noTsc" in plain));
  const noTsc = parseArgs(["--tier", "stress", "--oss", "--allow-sampled", "--no-tsc"]);
  assert.deepEqual(
    [noTsc.arms, noTsc.oss, noTsc.biome, noTsc.noTsc],
    [["verter", "verter-obs", "verter-observe", "verter-counted"], [], true, true],
  );
  assert.throws(() => parseArgs(["--no-tsc", "--arms", "verter,tsc-cli"]), /excludes.*tsc-cli/);
  // --no-demand skips the demand section; --only-oss / --only-biome are --oss / --biome with it.
  const onlyOss = parseArgs(["--only-oss", "--allow-sampled"]);
  assert.deepEqual(
    [onlyOss.arms, onlyOss.oss, onlyOss.biome, onlyOss.noDemand],
    [[], [], true, true],
  );
  const onlyTsz = parseArgs(["--only-oss", "tsz"]);
  assert.deepEqual([onlyTsz.oss, onlyTsz.biome, onlyTsz.noDemand], [["tsz"], undefined, true]);
  const onlyBiome = parseArgs(["--only-biome"]);
  assert.deepEqual(
    [onlyBiome.arms, "oss" in onlyBiome, onlyBiome.biome, onlyBiome.noDemand],
    [[], false, true, true],
  );
  assert.ok(!("noDemand" in parseArgs(["--oss"])));
  assert.throws(() => parseArgs(["--only-biome", "--arms", "verter"]), /--arms does not apply/);
  const noDemand = parseArgs(["--oss", "--no-demand"]);
  assert.deepEqual([noDemand.arms, noDemand.oss, noDemand.noDemand], [[], [], true]);
  assert.throws(() => parseArgs(["--no-demand"]), /nothing to run/);
  const tools = loadTools();
  assert.deepEqual(selectCheckers(tools, []), ["tsz", "bamtiscript", "ezno", "tsrust"]);
  assert.deepEqual(selectCheckers(tools, ["ezno", "tsz"]), ["tsz", "ezno"]);
});

// ---------------------------------------------------------------- reading and classifying OSS answers

test("a tsc-format output is read by the reference's own reader; any other output is unreadable", () => {
  const one = readOssAnswer("tsc", tscOut("[[1]]"), SOURCE);
  assert.equal(one.answer.digest.sha256, canonicalDigest("1").sha256);
  assert.deepEqual(one.answer.codes, []);
  assert.match(
    readOssAnswer("ezno", "error: Type [B] is not assignable", SOURCE).unreadable,
    /own format/,
  );
  // No TS2322 on the marker while the never check says "no": not an answer.
  assert.match(readOssAnswer("tsc", tscOut(null), SOURCE).unreadable, /not never/);
  assert.match(
    readOssAnswer("tsc", "error TS5083: platform path prefix is unsupported\n", SOURCE).unreadable,
    /never check/,
  );
  // A print that is not type syntax fails closed.
  assert.ok(readOssAnswer("tsc", tscOut("[[... 3 more ...]]"), SOURCE).unreadable);
  const r = readInvocation(
    "ezno",
    "",
    "\x1b[1merror\x1b[0m: Expected identifier at property key",
    SOURCE,
  );
  assert.match(r.unreadable, /output: error: Expected identifier/);
});

test("only the same answer with the same diagnostics is matched; a fast wrong answer is a finding", () => {
  const ctx = { reference: reference("1") };
  const read = (tuple, extra) => readOssAnswer("tsc", tscOut(tuple, { extra }), SOURCE);
  assert.equal(classifyOssAnswer(exited(), read("[[1]]"), ctx).class, "matched");
  assert.equal(classifyOssAnswer(exited(), read("[[2]]"), ctx).class, "mismatch");
  const extra = `scenario.ts(1,1): error TS2304: Cannot find name 'X'.\n`;
  const other = classifyOssAnswer(exited(), read("[[1]]", extra), ctx);
  assert.equal(other.class, "mismatch");
  assert.match(other.detail, /other diagnostics: TS2304/);
  assert.equal(classifyOssAnswer(exited(3), read("[[1]]"), ctx).class, "error");
  assert.equal(
    classifyOssAnswer({ kind: "killed", detail: "memory" }, read("[[1]]"), ctx).class,
    "killed",
  );
  assert.equal(
    classifyOssAnswer({ kind: "unattributed-kill", detail: "t" }, read("[[1]]"), ctx).class,
    "unverified",
  );
  assert.equal(classifyOssAnswer(exited(), { unreadable: "x" }, ctx).class, "unreadable");
  // tsc's error-any at its limit: matched only with the same resource code.
  const limit = { reference: reference("any", [2589], true), beyond: canonicalDigest('"ok"') };
  const anyWith = (codes) => ({
    answer: { digest: canonicalDigest("any"), errorAny: codes.includes(2589), codes },
  });
  assert.equal(classifyOssAnswer(exited(), anyWith([2589]), limit).class, "matched");
  assert.equal(classifyOssAnswer(exited(), anyWith([]), limit).class, "mismatch");
  const beyond = { answer: { digest: canonicalDigest('"ok"'), errorAny: false, codes: [] } };
  assert.equal(classifyOssAnswer(exited(), beyond, limit).class, "beyond-tsc");
  assert.equal(
    classifyOssAnswer(exited(), beyond, { reference: { gap: "g" } }).class,
    "no-reference",
  );
});

test("the tsc reference arm must reproduce the reference, or the run fails", () => {
  const read = (tuple) => readOssAnswer("tsc", tscOut(tuple), SOURCE);
  assert.equal(tscMeasureStatus(exited(), read("[[1]]"), reference("1")).status, "reference");
  assert.equal(tscMeasureStatus(exited(), read("[[2]]"), reference("1")).status, "problem");
  assert.equal(tscMeasureStatus(exited(), read("[[1]]"), reference("1", [2589])).status, "problem");
  assert.equal(tscMeasureStatus(exited(7), read("[[1]]"), reference("1")).status, "problem");
  assert.equal(
    tscMeasureStatus({ kind: "killed", detail: "m" }, {}, reference("1")).status,
    "killed",
  );
});

// ---------------------------------------------------------------- the OSS section on a synthetic run

const MEM_MB = 64;
const INFRA_MB = 1024;
const OPTIONS = {
  repeat: 2,
  warmup: 1,
  memMb: MEM_MB,
  infraMb: INFRA_MB,
  timeoutMs: 1000,
  startupAllowanceMs: 500,
};
const supervisorRecord = (overrides = {}) => ({
  schema: 1,
  launched: true,
  wallMs: 10,
  exitCode: 1,
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
  cpuUserMs: 5,
  cpuKernelMs: 1,
  ...overrides,
});
const SCENARIO = {
  id: "synthetic",
  family: "test",
  note: "",
  probe: "1",
  source: moduleText("", "1"),
};
const EXPECTED = {
  schema: 3,
  method: { measuringSuffixSha256: sha256Text(MEASURING_SUFFIX), libSha256: "lib" },
  scenarios: {
    synthetic: {
      sourceSha256: sha256Text(SCENARIO.source),
      settings: {
        strict: {
          never: false,
          printed: "[1]",
          codes: [],
          receipt: {
            sourceSha256: sha256Text(measureSource(SCENARIO)),
            tsconfigSha256: sha256Text(tsconfigText(SETTINGS[0])),
          },
        },
      },
    },
  },
};

/** A Verter probe record answering `text`, as the probe writes it. */
const verterRecord = (text) => ({
  tool: "verter",
  stage: "complete",
  phases: { engineStart: 500, setup: 1000, teardown: 10 },
  init: { micros: 100, outcome: { kind: "value" } },
  probes: [
    {
      alias: "__Probe",
      cold: { micros: 50, outcome: { kind: "value" } },
      warm: [],
      observeMicros: 5,
      observation: {
        text,
        error: null,
        shape: "literal",
        unknownLeaves: 0,
        unknownSamples: [],
        conditionalNodes: 0,
      },
    },
  ],
  afterRequests: { metric: "private-commit", peakBytes: 50, currentBytes: 40, cpuMicros: 900 },
});

function ossRun({
  toolTuple = "[[1]]",
  tscTuple = "[[1]]",
  verterText = "1",
  wall = { tool: 5, tsc: 20, verter: 2 },
  options = OPTIONS,
} = {}) {
  const tools = {
    tsz: { status: "available", name: "tsz", binary: { path: "tsz.exe", sha256: "s" } },
    ezno: {
      status: "unavailable",
      name: "Ezno",
      reason: "unavailable on test-x64: it does not run here",
    },
  };
  const arms = ossArms(options, ["tsz"]);
  const plan = schedule(["synthetic/strict"], arms, OPTIONS.repeat, OPTIONS.warmup);
  const invocations = plan.map((step, index) => {
    if (step.arm === VERTER_ARM)
      return {
        index,
        scenario: "synthetic",
        setting: "strict",
        arm: step.arm,
        rep: step.rep,
        warmup: step.warmup,
        command: ["/bin/probe", "run", "--job", "j.json", "--out", "o.json"],
        supervisorOut: "x.sup.json",
        supervisorExit: 0,
        supervisor: supervisorRecord({ exitCode: 0, wallMs: wall.verter, peakBytes: 60 }),
        reading: null,
        spawnedAtMs: 1000,
        probeOut: null,
        phase: "done",
        phaseHistory: [{ phase: "done", atMs: 1020 }],
        probe: verterRecord(verterText),
        probeReadError: null,
      };
    const tool = step.arm === "oss-tsz";
    const stdout = tscOut(tool ? toolTuple : tscTuple);
    return {
      index,
      scenario: "synthetic",
      setting: "strict",
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command: [tool ? "tsz.exe" : "tsc.exe"],
      supervisorOut: "x.sup.json",
      supervisorExit: 1,
      supervisor: supervisorRecord({
        wallMs: tool ? wall.tool : wall.tsc,
        peakBytes: tool ? 100 : 400,
      }),
      reading: readInvocation("tsc", stdout, "", measureSource(SCENARIO)),
    };
  });
  const oss = {
    schema: OSS_SCHEMA,
    platform: "test-x64",
    verter: { binary: "/bin/probe" },
    tools,
    toolsAfter: { tsz: "s" },
    arms,
    cells: {
      "synthetic/strict": {
        id: "synthetic",
        setting: "strict",
        dir: "d",
        inputs: {
          "lib.bench.d.ts": "lib",
          "scenario.ts": sha256Text(measureSource(SCENARIO)),
          "tsconfig.json": sha256Text(tsconfigText(SETTINGS[0])),
        },
      },
    },
    plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    invocations,
  };
  oss.summary = summarizeOss(oss, EXPECTED, [SCENARIO], OPTIONS);
  return oss;
}

test("a matched OSS row is compared with tsc -p and a wrong one is a finding, not a failure", () => {
  const oss = ossRun();
  const v = validateOss(oss, EXPECTED, [SCENARIO], OPTIONS, schedule);
  assert.deepEqual(v.failures, []);
  const cell = oss.summary.cells[0];
  assert.equal(cell.arms["oss-tsz"].class, "matched");
  assert.equal(cell.headline["oss-tsz"].parallel.wallMs.verdict, "verter");
  const wrong = ossRun({ toolTuple: "[[2]]" });
  assert.deepEqual(validateOss(wrong, EXPECTED, [SCENARIO], OPTIONS, schedule).failures, []);
  assert.equal(wrong.summary.cells[0].arms["oss-tsz"].class, "mismatch");
  assert.equal(wrong.summary.cells[0].headline["oss-tsz"], undefined);
});

test("Verter runs in the OSS section as a whole process, classified as the demand section classifies it", () => {
  const oss = ossRun();
  assert.deepEqual(oss.arms, ["tsc-measure", "tsc-measure-1", VERTER_ARM, ossArm("tsz")]);
  assert.deepEqual(validateOss(oss, EXPECTED, [SCENARIO], OPTIONS, schedule).failures, []);
  const cell = oss.summary.cells[0];
  assert.equal(cell.arms[VERTER_ARM].class, "matched");
  assert.ok(Math.abs(cell.arms[VERTER_ARM].firstTypeMs.median - 1.15) < 1e-9);
  // Its whole-process figures against tsc -p, and each tool's against it.
  assert.equal(cell.headline[VERTER_ARM].parallel.wallMs.verdict, "verter");
  assert.equal(cell.headline["oss-tsz"].verter.wallMs.verdict, "verter");
  assert.equal(cell.headline["oss-tsz"].verter.peakBytes.verdict, "verter");
  const slow = ossRun({ wall: { tool: 5, tsc: 20, verter: 9 } });
  assert.equal(slow.summary.cells[0].headline["oss-tsz"].verter.wallMs.verdict, "tsc");
  // A wrong Verter answer is a finding: no validation failure, and no tool is set against it.
  const wrong = ossRun({ verterText: "2" });
  assert.deepEqual(validateOss(wrong, EXPECTED, [SCENARIO], OPTIONS, schedule).failures, []);
  assert.equal(wrong.summary.cells[0].arms[VERTER_ARM].class, "mismatch");
  assert.equal(wrong.summary.cells[0].headline[VERTER_ARM], undefined);
  assert.equal(wrong.summary.cells[0].headline["oss-tsz"].verter, null);
  const other = ossRun();
  for (const inv of other.invocations)
    if (inv.arm === VERTER_ARM) inv.command = ["/bin/other", ...inv.command.slice(1)];
  assert.match(
    validateOss(other, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /not the run's Verter probe/,
  );
});

test("the OSS report tallies each arm against tsc -p over the run", () => {
  const oss = ossRun();
  Object.assign(oss.tools.tsz, {
    pin: "v1",
    commit: "c".repeat(40),
    license: "MIT",
    source: { type: "release", sha256: "x" },
  });
  const md = renderOssMarkdown(oss);
  assert.ok(md.includes("| Verter | 1 of 1 | 1 / 0 / 0 |"), md);
  assert.ok(md.includes("| tsz | 1 of 1 | 1 / 0 / 0 |"), md);
  const slow = ossRun({ wall: { tool: 50, tsc: 20, verter: 2 } });
  Object.assign(slow.tools.tsz, oss.tools.tsz);
  assert.ok(renderOssMarkdown(slow).includes("| tsz | 1 of 1 | 0 / 1 / 0 |"));
  const noTsc = ossRun({ options: { ...OPTIONS, noTsc: true } });
  Object.assign(noTsc.tools.tsz, oss.tools.tsz);
  assert.ok(!renderOssMarkdown(noTsc).includes("Against tsc"));
});

test("under --no-tsc the OSS section runs only Verter and the tools and still classifies them against the reference", () => {
  const NO_TSC = { ...OPTIONS, noTsc: true };
  const oss = ossRun({ options: NO_TSC });
  assert.deepEqual(oss.arms, [VERTER_ARM, ossArm("tsz")]);
  assert.deepEqual(validateOss(oss, EXPECTED, [SCENARIO], NO_TSC, schedule).failures, []);
  const cell = oss.summary.cells[0];
  assert.equal(cell.arms["oss-tsz"].class, "matched");
  assert.equal(cell.headline["oss-tsz"].parallel, null);
  assert.equal(cell.headline["oss-tsz"].verter.wallMs.verdict, "verter");
  const wrong = ossRun({ toolTuple: "[[2]]", options: NO_TSC });
  assert.equal(wrong.summary.cells[0].arms["oss-tsz"].class, "mismatch");
  // The arms must be the options': a run without its references fails a
  // default validation, and a default run fails a --no-tsc one.
  assert.match(
    validateOss(oss, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /the arms are not the reference arms/,
  );
  assert.match(
    validateOss(ossRun(), EXPECTED, [SCENARIO], NO_TSC, schedule).failures.join("\n"),
    /the arms are not \(under --no-tsc\)/,
  );
});

test("the OSS section fails on a wrong tsc reference, a changed binary, a wrong cap or a tampered summary", () => {
  const badTsc = ossRun({ tscTuple: "[[2]]" });
  assert.match(
    validateOss(badTsc, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /wrong answer against the measured reference/,
  );
  const changed = ossRun();
  changed.toolsAfter.tsz = "t";
  assert.match(
    validateOss(changed, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /tsz changed during the run/,
  );
  const cap = ossRun();
  cap.invocations[0].supervisor.memLimitBytes = 1;
  assert.match(
    validateOss(cap, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /containment cap/,
  );
  const tampered = ossRun({ toolTuple: "[[2]]" });
  tampered.summary.cells[0].arms["oss-tsz"].class = "matched";
  assert.match(
    validateOss(tampered, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /stored summary/,
  );
  const plan = ossRun();
  plan.invocations.pop();
  assert.match(
    validateOss(plan, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /records for/,
  );
  const silent = ossRun();
  silent.tools.ezno = { status: "unavailable", reason: "" };
  assert.match(
    validateOss(silent, EXPECTED, [SCENARIO], OPTIONS, schedule).failures.join("\n"),
    /neither available/,
  );
});

// ---------------------------------------------------------------- the thenable catalog and Biome

test("the thenable catalog is paired, and every pair's measured answers differ", () => {
  const cases = thenableCases();
  const expected = loadThenableExpected();
  assert.equal(expected.method.measuringSuffixSha256, sha256Text(MEASURING_SUFFIX));
  const pairs = new Map();
  for (const c of cases) {
    assert.ok(
      c.source.includes("type __BenchInit = 0;") && c.source.includes("type __Probe = "),
      c.id,
    );
    const raw = expected.cases[c.id];
    assert.ok(raw, `${c.id} is measured`);
    assert.equal(raw.sourceSha256, sha256Text(c.source), `${c.id} was measured on this source`);
    assert.deepEqual(raw.codes, [], `${c.id} has no other diagnostic`);
    const e = expectedFor(expected, c.id);
    assert.equal(e.thenable, raw.thenable);
    (pairs.get(c.pair) ?? pairs.set(c.pair, {}).get(c.pair))[c.variant] = e.thenable;
  }
  for (const [pair, v] of pairs) assert.deepEqual(v, { yes: true, no: false }, pair);
});

test("the projection reads Promise-like references and unions of them, nothing else", () => {
  const t = (text) => isThenable(normalize(parseType(text)));
  assert.equal(t("Promise<1>"), true);
  assert.equal(t("PromiseLike<1>"), true);
  assert.equal(t("Promise<1> | undefined"), true);
  assert.equal(t("1 | undefined"), false);
  assert.equal(t("{ then: 1 }"), false);
  assert.equal(t("Promise<1>[]"), false);
});

test("Biome's answer is the floating-promise report on the demanded line, and nothing else", () => {
  const source = moduleText("", "Promise<1>");
  const line = demandLine(source);
  assert.equal((source + BIOME_DEMAND).split("\n")[line - 1], "__bench_probe();");
  const report = (diagnostics) => JSON.stringify({ summary: { duration: 1 }, diagnostics });
  const at = (l, category = "lint/nursery/noFloatingPromises", path = "scenario.ts") => ({
    category,
    location: { path, start: { line: l } },
    message: "m",
  });
  assert.equal(readBiomeAnswer(report([at(line)]), line).thenable, true);
  assert.equal(readBiomeAnswer(report([]), line).thenable, false);
  assert.match(readBiomeAnswer(report([at(line - 1)]), line).unreadable, /outside the demand/);
  assert.match(readBiomeAnswer(report([at(1, "parse")]), line).unreadable, /outside the demand/);
  assert.match(readBiomeAnswer("crash", line).unreadable, /no JSON/);
});

const BIOME_OPTIONS = { ...OPTIONS, repeat: 2, warmup: 1 };
function biomeRun({
  biomeThenable = true,
  verterText = "Promise<1>",
  options = BIOME_OPTIONS,
} = {}) {
  const expected = loadThenableExpected();
  const c = thenableCases().find((x) => x.id === "alias-chain-50-yes");
  const arms = biomeArms(options);
  const plan = schedule([c.id], arms, BIOME_OPTIONS.repeat, BIOME_OPTIONS.warmup);
  const reading = (arm) =>
    arm === "biome-types"
      ? { thenable: biomeThenable }
      : arm === "verter-thenable"
        ? {
            thenable: isThenable(normalize(parseType(verterText))),
            digest: canonicalDigest(verterText),
          }
        : { thenable: true, digest: canonicalDigest("Promise<1>"), codes: [] };
  const result = {
    schema: 1,
    tool: { status: "available", binary: { path: "biome.exe", sha256: "b" } },
    toolAfter: "b",
    arms,
    cells: {
      [c.id]: {
        id: c.id,
        pair: c.pair,
        variant: c.variant,
        lib: expected.method.libSha256,
        inputs: {
          measure: sha256Text(c.source + MEASURING_SUFFIX),
          verter: sha256Text(c.source),
          biome: sha256Text(c.source + BIOME_DEMAND),
        },
      },
    },
    plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    invocations: plan.map((step, index) => ({
      index,
      case: c.id,
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command: [step.arm === "biome-types" ? "biome.exe" : "x"],
      supervisor: supervisorRecord({
        exitCode: step.arm === "verter-thenable" ? 0 : 1,
        wallMs: step.arm === "biome-types" ? 30 : 8,
      }),
      reading: reading(step.arm),
      firstTypeMs: step.arm === "verter-thenable" ? 1 : null,
    })),
  };
  result.summary = summarizeBiome(result, expected, BIOME_OPTIONS);
  return { result, expected };
}

test("a Biome row is compared only when all three arms answer as tsc; a wrong answer is a finding", () => {
  const { result, expected } = biomeRun();
  assert.deepEqual(validateBiome(result, expected, BIOME_OPTIONS, schedule).failures, []);
  const cell = result.summary.cells[0];
  assert.equal(cell.arms["biome-types"].class, "matched");
  assert.equal(cell.headline.wallMs.verdict, "verter");
  const wrong = biomeRun({ biomeThenable: false });
  assert.deepEqual(
    validateBiome(wrong.result, wrong.expected, BIOME_OPTIONS, schedule).failures,
    [],
  );
  assert.equal(wrong.result.summary.cells[0].arms["biome-types"].class, "mismatch");
  assert.equal(wrong.result.summary.cells[0].headline, null);
  // Verter is held to the full answer, not only its projection.
  const partial = biomeRun({ verterText: "Promise<2>" });
  assert.equal(partial.result.summary.cells[0].arms["verter-thenable"].class, "mismatch");
  const unreadable = biomeInvocationClass(
    { supervisor: supervisorRecord(), reading: { unreadable: "u" } },
    { thenable: true },
    { budgetBytes: 1 << 30 },
  );
  assert.equal(unreadable.class, "unreadable");
});

test("under --no-tsc the Biome section compares Verter with Biome against the measured reference", () => {
  const NO_TSC = { ...BIOME_OPTIONS, noTsc: true };
  const { result, expected } = biomeRun({ options: NO_TSC });
  assert.deepEqual(result.arms, ["verter-thenable", "biome-types"]);
  assert.deepEqual(validateBiome(result, expected, NO_TSC, schedule).failures, []);
  assert.equal(result.summary.cells[0].headline.wallMs.verdict, "verter");
  assert.equal(result.summary.matched.tsc, null);
  const wrong = biomeRun({ biomeThenable: false, options: NO_TSC });
  assert.equal(wrong.result.summary.cells[0].headline, null);
  assert.match(
    validateBiome(result, expected, BIOME_OPTIONS, schedule).failures.join("\n"),
    /the arms are not every Biome-section arm/,
  );
  const full = biomeRun();
  assert.match(
    validateBiome(full.result, full.expected, NO_TSC, schedule).failures.join("\n"),
    /the arms are not the tool arms/,
  );
});

test("the Biome section fails on a tampered summary, a changed binary or a stale reference", () => {
  const { result, expected } = biomeRun({ biomeThenable: false });
  result.summary.cells[0].arms["biome-types"].class = "matched";
  assert.match(
    validateBiome(result, expected, BIOME_OPTIONS, schedule).failures.join("\n"),
    /stored summary/,
  );
  const changed = biomeRun();
  changed.result.toolAfter = "c";
  assert.match(
    validateBiome(changed.result, changed.expected, BIOME_OPTIONS, schedule).failures.join("\n"),
    /Biome changed/,
  );
  const stale = biomeRun();
  const id = Object.keys(stale.result.cells)[0];
  const moved = structuredClone(stale.expected);
  moved.cases[id].sourceSha256 = "0";
  assert.match(
    validateBiome(stale.result, moved, BIOME_OPTIONS, schedule).failures.join("\n"),
    /stale/,
  );
  const tscWrong = biomeRun();
  for (const inv of tscWrong.result.invocations)
    if (inv.arm === "tsc-thenable")
      inv.reading = { thenable: false, digest: canonicalDigest("2"), codes: [] };
  tscWrong.result.summary = summarizeBiome(tscWrong.result, tscWrong.expected, BIOME_OPTIONS);
  assert.match(
    validateBiome(tscWrong.result, tscWrong.expected, BIOME_OPTIONS, schedule).failures.join("\n"),
    /does not reproduce/,
  );
});
