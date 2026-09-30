// The open-source type-checker arms (`--oss`): every pinned checker of
// tools.json checks the SAME measuring program the reference was measured on
// (the scenario plus MEASURING_SUFFIX, the benchmark library as a root file
// under noLib, the setting's tsconfig), whole program, under the same
// supervisor, cap and deadline as tsc, in its own counterbalanced schedule
// beside the reference arms that run tsc 7.0.2's own measuring method:
//
//   tsc-measure     tsc -p <tsconfig>                  (default, parallel checkers)
//   tsc-measure-1   tsc -p <tsconfig> --singleThreaded
//   oss-<tool>      the tool's own command on the same tsconfig or files
//
// A tool's row enters the head-to-head only when its answer MATCHED tsc's
// measured answer (canonical type and diagnostic codes) and the tsc arm
// reproduced the reference; wrong, unreadable, erroring or killed answers are
// findings, never wins. These are whole-program runs: they compare with the
// `tsc -p` arms, never with the demanded-probe arms (verter, tsc-api). The
// whole section is absent from a run without `--oss`.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { invocationEnd, runLimits, stats, supervisorDeadlineMs, verdict } from "../analyze.mjs";
import { canonicalDigest } from "../canonical.mjs";
import { MEASURING_SUFFIX } from "../measure-expected.mjs";
import { sha256Text } from "../provenance.mjs";
import { referenceFor } from "../summary.mjs";
import { supervisorRecordProblems } from "../supervisor.mjs";
import { classifyOssAnswer, OSS_CLASSES, readOssAnswer, tscMeasureStatus } from "./answers.mjs";
import { platformKey, provisioned, provisionTool } from "./provision.mjs";

export const OSS_SCHEMA = 1;

/** The reference arms of the OSS section: tsc 7.0.2 on the measuring program. */
export const REFERENCE_ARMS = {
  "tsc-measure": {
    label: "tsc -p on the measuring program, default (parallel) checkers (reference)",
    extra: [],
  },
  "tsc-measure-1": {
    label: "tsc -p --singleThreaded on the measuring program (reference)",
    extra: ["--singleThreaded"],
  },
};

export const ossArm = (id) => `oss-${id}`;
export const toolOfArm = (arm) => (arm.startsWith("oss-") ? arm.slice(4) : null);

/** The checker ids a checker list selects (every checker when the list is empty). */
export function selectCheckers(tools, list) {
  const checkers = Object.keys(tools).filter((id) => tools[id].kind === "checker");
  if (!list.length) return checkers;
  for (const id of list)
    if (!checkers.includes(id))
      throw new Error(
        `--oss ${id}: not a pinned tool; tools: ${[...checkers, "biome"].join(", ")}`,
      );
  return checkers.filter((id) => list.includes(id));
}

/** The measuring program one cell's OSS arms read. */
export function measureSource(scenario) {
  return scenario.source + MEASURING_SUFFIX;
}

/** Write the measuring program of one cell next to its scenario files. */
export function materializeMeasure(cellDir, scenario, tsconfigText, libText) {
  const dir = join(cellDir, "measure");
  mkdirSync(dir, { recursive: true });
  const source = measureSource(scenario);
  writeFileSync(join(dir, "lib.bench.d.ts"), libText);
  writeFileSync(join(dir, "scenario.ts"), source);
  writeFileSync(join(dir, "tsconfig.json"), tsconfigText);
  return {
    dir,
    inputs: {
      "lib.bench.d.ts": sha256Text(libText),
      "scenario.ts": sha256Text(source),
      "tsconfig.json": sha256Text(tsconfigText),
    },
  };
}

/** The command of one OSS-section arm on one cell's measuring program. */
export function ossCommand(arm, { tscExe, tools, available, measureDir }) {
  if (REFERENCE_ARMS[arm])
    return [tscExe, "-p", join(measureDir, "tsconfig.json"), ...REFERENCE_ARMS[arm].extra];
  const id = toolOfArm(arm);
  const tool = tools[id];
  const fill = (a) =>
    a
      .replace("{tsconfig}", join(measureDir, "tsconfig.json"))
      .replace("{scenario}", join(measureDir, "scenario.ts"))
      .replace("{lib}", join(measureDir, "lib.bench.d.ts"));
  return [available[id].binary, ...tool.argv.map(fill)];
}

/**
 * How one OSS-section invocation ended. Each is one process that is its
 * engine from its start, so a kill is attributed by the rule of the `tsc -p`
 * arms (reference.mjs: the kill threshold at the budget, or the whole
 * deadline run).
 */
export const ossInvocationEnd = (inv, limits) => invocationEnd({ ...inv, arm: "tsc-cli" }, limits);

const ANSI = /\x1b\[[0-9;]*m/g;

/**
 * The first message line of a tool's output, for a finding's detail: colour
 * codes and box-drawing gutters removed, and a bare severity word, a bare
 * `file:line:column` location and an echoed source line (behind its line
 * number) skipped.
 */
export function firstLine(text) {
  return (
    String(text ?? "")
      .replace(ANSI, "")
      .split(/\r?\n/)
      .map((l) =>
        l
          .replace(/[─-╿]/g, " ")
          .trim()
          .replace(/^[\^~\s]+/, ""),
      )
      .find(
        (l) =>
          l && !/^(error|warning|info):?$/i.test(l) && !/:\d+:\d+$/.test(l) && !/^\d+\s/.test(l),
      ) ?? ""
  ).slice(0, 200);
}

/**
 * The stored reading of one invocation's output: what the classifier needs,
 * derived from the whole stdout at run time and re-derived by the validator
 * from the raw file.
 */
export function readInvocation(format, stdout, stderr, source) {
  const read = readOssAnswer(format, stdout, source);
  const line = firstLine(stdout) || firstLine(stderr);
  return {
    stdoutSha256: sha256Text(stdout),
    stdoutBytes: stdout.length,
    firstLine: line,
    ...(read.unreadable
      ? { unreadable: line ? `${read.unreadable} (output: ${line})` : read.unreadable }
      : {
          answer: {
            digest: read.answer.digest,
            errorAny: read.answer.errorAny,
            codes: read.answer.codes,
          },
        }),
  };
}

/** Provision (or find) every selected tool; unavailable ones carry their reason. */
export async function prepareTools(root, tools, ids, { supervisor, log, jobs }) {
  const available = {};
  const status = {};
  for (const id of ids) {
    let p = provisioned(root, id, tools[id]);
    if (p.unavailable && /not provisioned/.test(p.unavailable)) {
      await provisionTool(root, id, tools[id], { supervisor, log, jobs });
      p = provisioned(root, id, tools[id]);
    }
    if (p.ok) {
      available[id] = p;
      status[id] = {
        status: "available",
        name: tools[id].name,
        pin: tools[id].pin,
        commit: tools[id].commit,
        license: tools[id].license,
        answers: tools[id].answers,
        format: tools[id].format,
        source: p.provenance.source,
        build: p.provenance.build
          ? { argv: p.provenance.build.argv, rustc: p.provenance.build.rustc }
          : null,
        binary: { path: p.binary, sha256: p.sha256 },
      };
    } else {
      status[id] = {
        status: "unavailable",
        name: tools[id].name,
        pin: tools[id].pin,
        commit: tools[id].commit,
        reason: p.unavailable,
      };
      log(`semantic-perf: --oss ${id}: ${p.unavailable}`);
    }
  }
  return { available, status };
}

/**
 * Run the OSS section: the measuring program of every cell, every arm, in
 * a counterbalanced schedule, each invocation under the supervisor with the
 * run's cap and deadline. `ctx`: root, outDir, cells (the semantic run's,
 * each with scenario, setting, dir), opts, tools, ids, supervisor (pinned),
 * typescript, runtimeEnv, libText, tsconfigText, schedule, runSupervised,
 * log.
 */
export async function runOss(ctx) {
  const { outDir, cells, opts, tools, ids, typescript, runtimeEnv, log } = ctx;
  const { available, status } = await prepareTools(ctx.root, tools, ids, {
    supervisor: ctx.supervisor,
    log,
    jobs: ctx.jobs,
  });
  const arms = [...Object.keys(REFERENCE_ARMS), ...ids.filter((id) => available[id]).map(ossArm)];
  const cellMeta = {};
  for (const [key, cell] of cells) {
    const m = materializeMeasure(
      cell.dir,
      cell.scenario,
      ctx.tsconfigText(cell.setting),
      ctx.libText,
    );
    cellMeta[key] = {
      id: cell.scenario.id,
      setting: cell.setting.id,
      dir: m.dir,
      inputs: m.inputs,
    };
  }
  const plan = ctx.schedule(Object.keys(cellMeta), arms, opts.repeat, opts.warmup);
  const invocations = [];
  for (const [index, step] of plan.entries()) {
    const meta = cellMeta[step.key];
    const cell = cells.get(step.key);
    const runDir = join(outDir, "oss-runs", meta.id, meta.setting, step.arm);
    mkdirSync(runDir, { recursive: true });
    const runBase = join(runDir, `${step.warmup ? "warmup" : "rep"}-${step.rep}`);
    const command = ossCommand(step.arm, {
      tscExe: typescript.exe,
      tools,
      available,
      measureDir: meta.dir,
    });
    const supOut = `${runBase}.sup.json`;
    const result = await ctx.runSupervised(ctx.supervisor, {
      memMb: opts.memMb + opts.infraMb,
      timeoutMs: supervisorDeadlineMs(opts),
      out: supOut,
      env: runtimeEnv,
      cwd: meta.dir,
      argv: command,
      allowSampled: opts.allowSampled,
    });
    const record = result.record
      ? { ...result.record, samples: undefined, sampleCount: result.record.samples?.length ?? 0 }
      : null;
    const tool = toolOfArm(step.arm);
    let reading = null;
    if (record?.launched && !record.killedBy && record.stdoutPath) {
      try {
        const stdout = readFileSync(record.stdoutPath, "utf8");
        const stderr = existsSync(record.stderrPath) ? readFileSync(record.stderrPath, "utf8") : "";
        reading = readInvocation(
          tool ? tools[tool].format : "tsc",
          stdout,
          stderr,
          measureSource(cell.scenario),
        );
      } catch (err) {
        reading = { unreadable: `the output could not be read: ${err.message}` };
      }
    }
    invocations.push({
      index,
      scenario: meta.id,
      setting: meta.setting,
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command,
      supervisorOut: supOut,
      supervisorExit: result.supervisorExit,
      supervisorReadError: result.readError ?? result.spawnError ?? null,
      supervisor: record,
      reading,
    });
    const end = record?.killedBy ? `killed:${record.killedBy}` : `exit ${record?.exitCode ?? "?"}`;
    log(
      `[oss ${index + 1}/${plan.length}] ${step.key} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: ${end} ${record?.wallMs?.toFixed?.(0) ?? "?"} ms`,
    );
  }
  const toolsAfter = Object.fromEntries(
    Object.entries(available).map(([id, a]) => [
      id,
      provisioned(ctx.root, id, tools[id]).sha256 ?? null,
    ]),
  );
  return {
    schema: OSS_SCHEMA,
    platform: platformKey(),
    tools: status,
    toolsAfter,
    arms,
    cells: cellMeta,
    plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    invocations,
  };
}

// ---------------------------------------------------------------- summary

const beyondCache = new Map();
function beyondDigestOf(scenario) {
  if (!scenario?.beyond) return null;
  if (!beyondCache.has(scenario.id)) {
    let d = null;
    try {
      d = canonicalDigest(scenario.beyond);
    } catch {
      d = null;
    }
    beyondCache.set(scenario.id, d);
  }
  return beyondCache.get(scenario.id);
}

/** Per-invocation figures of a whole-program run (supervisor-measured). */
function runMetrics(inv) {
  const s = inv.supervisor ?? {};
  const cpu =
    typeof s.cpuUserMs === "number" && typeof s.cpuKernelMs === "number"
      ? s.cpuUserMs + s.cpuKernelMs
      : null;
  return { wallMs: s.wallMs ?? null, peakBytes: s.peakBytes ?? null, cpuMs: cpu };
}

function overBudget(inv, limits) {
  const peak = inv.supervisor?.peakBytes;
  return typeof peak === "number" && peak > limits.budgetBytes;
}

/** One invocation's class (tool arms) or reference status (tsc arms). */
export function invocationClass(inv, reference, beyond, limits) {
  let end = ossInvocationEnd(inv, limits);
  if (end.kind === "exited" && overBudget(inv, limits))
    end = {
      kind: "killed",
      detail: `the process peak ${(inv.supervisor.peakBytes / 1048576).toFixed(0)} MiB exceeds the ${(limits.budgetBytes / 1048576).toFixed(0)} MiB budget`,
    };
  const read = inv.reading ?? { unreadable: "no output was read" };
  if (REFERENCE_ARMS[inv.arm]) return tscMeasureStatus(end, read, reference);
  return classifyOssAnswer(end, read, { reference, beyond });
}

/** Summarise the OSS section against the measured reference. */
export function summarizeOss(oss, expected, scenarios, options) {
  const limits = runLimits(options);
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  const groups = new Map();
  for (const inv of oss.invocations ?? []) {
    const key = `${inv.scenario}/${inv.setting}`;
    if (!groups.has(key)) groups.set(key, {});
    (groups.get(key)[inv.arm] ??= []).push(inv);
  }
  const cells = [];
  const counts = {};
  for (const [key, meta] of Object.entries(oss.cells ?? {})) {
    const scenario = byId.get(meta.id);
    const reference = referenceFor(expected, meta.id, meta.setting);
    const beyond = beyondDigestOf(scenario);
    const arms = {};
    for (const [arm, invs] of Object.entries(groups.get(key) ?? {})) {
      const measured = invs.filter((i) => !i.warmup);
      const results = invs.map((i) => invocationClass(i, reference, beyond, limits));
      const completed = measured.filter((i, k) => {
        const r = results[invs.indexOf(i)];
        return r.class === "matched" || r.status === "reference";
      });
      const metrics = completed.map(runMetrics);
      const s = {
        invocations: invs.length,
        measured: measured.length,
        completedMeasured: completed.length,
        wallMs: stats(metrics.map((m) => m.wallMs)),
        peakBytes: stats(metrics.map((m) => m.peakBytes)),
        cpuMs: stats(metrics.map((m) => m.cpuMs)),
      };
      if (REFERENCE_ARMS[arm]) {
        const problems = [...new Set(results.filter((r) => r.problem).map((r) => r.problem))];
        const kinds = [...new Set(results.map((r) => r.status))];
        s.status = problems.length ? "problem" : kinds.length === 1 ? kinds[0] : "inconsistent";
        s.problems = problems;
        s.detail = results.find((r) => r.detail)?.detail ?? "";
      } else {
        const classes = [...new Set(results.map((r) => r.class))];
        const worst = OSS_CLASSES.find((c) => classes.includes(c)) ?? classes[0];
        const digests = new Set(
          invs.map(
            (i) => i.reading?.answer?.digest?.sha256 ?? `<${i.reading?.unreadable ?? "none"}>`,
          ),
        );
        s.class = worst;
        s.classes = classes;
        s.repetitionsDiffer = classes.length > 1 || digests.size > 1;
        const worstDetail = results.find((r) => r.class === worst)?.detail ?? "";
        s.detail = s.repetitionsDiffer
          ? `repetitions differ (${classes.join(", ")}; ${digests.size} distinct answers): ${worstDetail}`
          : worstDetail;
        s.answer = invs.find((i) => i.reading?.answer)?.reading.answer.digest.preview ?? null;
        counts[toolOfArm(arm)] ??= {};
        counts[toolOfArm(arm)][worst] = (counts[toolOfArm(arm)][worst] ?? 0) + 1;
      }
      arms[arm] = s;
    }
    // The head-to-head of each tool with the default tsc arm (and the
    // single-threaded one beside it): only where the tool matched on every
    // invocation and tsc reproduced the reference.
    const headline = {};
    const tscOk = (arm) => arms[arm]?.status === "reference";
    for (const [arm, s] of Object.entries(arms)) {
      if (REFERENCE_ARMS[arm] || s.class !== "matched" || s.repetitionsDiffer) continue;
      const pick = (a, m) =>
        (groups.get(key)[a] ?? []).filter((i) => !i.warmup).map((i) => runMetrics(i)[m]);
      const vs = (tscArm) =>
        tscOk(tscArm)
          ? {
              wallMs: verdict(pick(arm, "wallMs"), pick(tscArm, "wallMs"), 1),
              peakBytes: verdict(pick(arm, "peakBytes"), pick(tscArm, "peakBytes"), 0),
              cpuMs: verdict(pick(arm, "cpuMs"), pick(tscArm, "cpuMs"), 1),
            }
          : null;
      headline[arm] = { parallel: vs("tsc-measure"), single: vs("tsc-measure-1") };
    }
    cells.push({
      key,
      scenario: meta.id,
      setting: meta.setting,
      family: scenario?.family ?? null,
      reference: reference
        ? {
            digest: reference.digest ?? null,
            errorAny: reference.errorAny ?? null,
            codes: reference.codes ?? [],
            gap: reference.gap ?? null,
          }
        : null,
      arms,
      headline,
    });
  }
  return { cells, classCounts: counts };
}

// ---------------------------------------------------------------- validation

const stable = (value) =>
  JSON.stringify(value, (_k, v) =>
    typeof v === "number" && !Number.isInteger(v) ? Number(v.toPrecision(12)) : v,
  );

/**
 * Validate the OSS section. Fails on a harness problem (a tool binary that
 * changed during the run or does not match its provisioning, a plan that is
 * not the counterbalanced schedule, a missing, duplicated or failed record,
 * the wrong cap, inputs that are not the reference's, a tsc reference arm
 * that does not reproduce the reference, a stored summary that disagrees
 * with its records). A tool's wrong, unreadable or failed answer is a
 * finding, never a failure.
 */
export function validateOss(oss, expected, scenarios, options, schedule) {
  const failures = [];
  const fail = (m) => failures.push(`oss: ${m}`);
  if (oss?.schema !== OSS_SCHEMA) {
    fail(`schema ${oss?.schema} is not ${OSS_SCHEMA}`);
    return { ok: false, failures };
  }
  for (const [id, t] of Object.entries(oss.tools ?? {})) {
    if (t.status === "available") {
      if (!t.binary?.sha256 || oss.toolsAfter?.[id] !== t.binary.sha256)
        fail(`wrong binary: ${id} changed during the run`);
    } else if (typeof t.reason !== "string" || !/^unavailable on /.test(t.reason))
      fail(`${id} is neither available nor unavailable with a reason`);
  }
  const wantArms = [
    ...Object.keys(REFERENCE_ARMS),
    ...Object.entries(oss.tools ?? {})
      .filter(([, t]) => t.status === "available")
      .map(([id]) => ossArm(id)),
  ];
  if (stable(wantArms) !== stable(oss.arms))
    fail("the arms are not the reference arms plus every available tool");
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  for (const [key, cell] of Object.entries(oss.cells ?? {})) {
    const scenario = byId.get(cell.id);
    if (!scenario) {
      fail(`${key}: not a catalog scenario`);
      continue;
    }
    if (cell.inputs?.["scenario.ts"] !== sha256Text(measureSource(scenario)))
      fail(`${key}: the measuring program is not the catalog's source plus the measuring suffix`);
    const receipt = expected.scenarios?.[cell.id]?.settings?.[cell.setting]?.receipt;
    if (receipt && receipt.sourceSha256 !== cell.inputs?.["scenario.ts"])
      fail(`${key}: the measuring program is not the one the reference was measured on`);
    if (receipt && receipt.tsconfigSha256 !== cell.inputs?.["tsconfig.json"])
      fail(`${key}: the tsconfig is not the one the reference was measured with`);
    if (expected.method?.libSha256 !== cell.inputs?.["lib.bench.d.ts"])
      fail(`${key}: the library is not the one the reference was measured with`);
  }
  const expectedPlan = schedule(
    Object.keys(oss.cells ?? {}),
    oss.arms ?? [],
    options.repeat,
    options.warmup,
  ).map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`);
  if (stable(expectedPlan) !== stable(oss.plan))
    fail("the recorded plan is not the counterbalanced schedule");
  const invs = oss.invocations ?? [];
  if (invs.length !== (oss.plan ?? []).length)
    fail(`${invs.length} records for ${(oss.plan ?? []).length} planned invocations`);
  const cap = (options.memMb + options.infraMb) * 1024 * 1024;
  const limits = runLimits(options);
  invs.forEach((inv, position) => {
    const id = `${inv.scenario}/${inv.setting}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
    if (inv.index !== position || oss.plan?.[position] !== id)
      fail(`record ${position} is ${id}; the plan says ${oss.plan?.[position] ?? "<nothing>"}`);
    for (const p of supervisorRecordProblems(inv.supervisor)) fail(`${id}: ${p}`);
    const sup = inv.supervisor;
    if (!sup) return;
    if (sup.containment !== "hard" && !(sup.containment === "sampled" && options.allowSampled))
      fail(`${id}: containment ${sup.containment} without consent (--allow-sampled)`);
    if (sup.memLimitBytes !== cap)
      fail(`${id}: containment cap ${sup.memLimitBytes} is not the run's (${cap})`);
    if (sup.timeoutMs !== supervisorDeadlineMs(options))
      fail(`${id}: deadline ${sup.timeoutMs} is not the run's ${supervisorDeadlineMs(options)} ms`);
    const end = ossInvocationEnd(inv, limits);
    if (end.kind === "harness-failure") fail(`${id}: failed child: ${end.detail}`);
    const tool = toolOfArm(inv.arm);
    if (tool && inv.command?.[0] !== oss.tools?.[tool]?.binary?.path)
      fail(`${id}: ran ${inv.command?.[0]}, not the provisioned ${tool}`);
    if (end.kind === "exited" && !inv.reading)
      fail(`${id}: an exited run has no reading of its output`);
  });
  const recomputed = summarizeOss(oss, expected, scenarios, options);
  for (const cell of recomputed.cells)
    for (const [arm, s] of Object.entries(cell.arms)) {
      if (!REFERENCE_ARMS[arm]) continue;
      for (const p of s.problems ?? [])
        fail(`${cell.key}|${arm}: wrong answer against the measured reference: ${p}`);
      if (s.status === "inconsistent") fail(`${cell.key}|${arm}: inconsistent repetitions`);
    }
  if (stable(oss.summary) !== stable(recomputed))
    fail("the stored summary disagrees with its raw records");
  return { ok: failures.length === 0, failures };
}

/**
 * Re-read every OSS invocation's raw files from disk (the supervisor record
 * and the stdout it names) and re-derive its reading: a stored copy that
 * differs is reported.
 */
export function ossRawFileProblems(oss, tools, scenarios) {
  const problems = [];
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  for (const inv of oss.invocations ?? []) {
    const id = `oss ${inv.scenario}/${inv.setting}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
    let sup;
    try {
      sup = JSON.parse(readFileSync(inv.supervisorOut, "utf8"));
    } catch (err) {
      problems.push(`${id}: cannot read ${inv.supervisorOut}: ${err.message}`);
      continue;
    }
    const { samples: _s, ...rest } = sup;
    const embedded = { ...inv.supervisor };
    delete embedded.samples;
    delete embedded.sampleCount;
    if (stable(rest) !== stable(embedded))
      problems.push(`${id}: the supervisor record on disk differs from results.json`);
    if (!inv.reading || !sup.stdoutPath) continue;
    try {
      const stdout = readFileSync(sup.stdoutPath, "utf8");
      const stderr = existsSync(sup.stderrPath) ? readFileSync(sup.stderrPath, "utf8") : "";
      const tool = toolOfArm(inv.arm);
      const again = readInvocation(
        tool ? tools[tool]?.format : "tsc",
        stdout,
        stderr,
        measureSource(byId.get(inv.scenario)),
      );
      if (stable(again) !== stable(inv.reading))
        problems.push(`${id}: the reading re-derived from the raw output differs`);
    } catch (err) {
      problems.push(`${id}: cannot re-read the output: ${err.message}`);
    }
  }
  return problems;
}

// ---------------------------------------------------------------- report

const f = (v) => (v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2));
const fmtMs = (s) =>
  !s ? "—" : s.n > 1 ? `${f(s.median)} [${f(s.min)}–${f(s.max)}]` : f(s.median);
const fmtMb = (s) => (!s ? "—" : (s.median / 1048576).toFixed(1));
const esc = (s) =>
  String(s ?? "")
    .replace(/\|/g, "\\|")
    .replace(/\n/g, " ");
const word = (v, name) =>
  !v || v.verdict === "n/a"
    ? "—"
    : v.verdict === "verter"
      ? `${name}${v.ratio && v.ratioMeaningful !== false ? ` (×${v.ratio.toFixed(2)})` : ""}`
      : v.verdict === "tsc"
        ? `tsc${v.ratio && v.ratioMeaningful !== false ? ` (×${v.ratio.toFixed(2)})` : ""}`
        : "overlap";

/** The OSS sections of results.md (only in a run with `--oss`). */
export function renderOssMarkdown(oss) {
  const lines = [];
  const push = (...l) => lines.push(...l);
  const summary = oss.summary ?? { cells: [], classCounts: {} };
  push("# Open-source type checkers (`--oss`): whole-program runs on the measuring program", "");
  push(
    `Validation of this section: **${oss.validation?.ok ? "PASSED" : "FAILED"}**`,
    "",
    "- Every arm checks the **same measuring program** tsc 7.0.2's reference was measured on (the scenario plus the measuring suffix, `lib.bench.d.ts` as a root file under `noLib`, the setting's tsconfig), as one fresh process under the same supervisor, cap and deadline, in its own counterbalanced schedule. `tsc-measure` runs the reference's own command; `tsc-measure-1` adds `--singleThreaded`.",
    "- A tool's answer is read exactly as the reference is (the TS2322 head line on the measuring assignment, the never check, every other diagnostic's code) and compared by canonical structure. **matched** needs the same answer and the same diagnostic codes as tsc; anything else is a finding and never enters a comparison.",
    "- These are **whole-program** runs: wall time includes process start, parsing and checking the whole program, for every arm alike; memory is the supervisor's peak for the process tree. They compare with the `tsc -p` arms only, never with the demanded-probe arms (Verter, tsc API) of the sections above. There is no warm figure (a one-shot CLI has no in-process repeat).",
    "- A verdict names a winner only when every measured repetition of one arm beats every repetition of the other (by more than 1 ms for times); ×N is tsc's median over the tool's (above 1 favours the tool).",
    "",
  );
  push("## Tools", "");
  push("| tool | pin | status | how it answers | provenance |", "|---|---|---|---|---|");
  for (const [id, t] of Object.entries(oss.tools ?? {})) {
    const prov =
      t.status === "available"
        ? `${t.source?.type === "git" ? `built from \`${t.commit.slice(0, 12)}\` (${esc(t.build?.argv?.join(" "))}; ${esc((t.build?.rustc ?? "").split("\n")[0])})` : `release file sha256 \`${String(t.source?.sha256).slice(0, 12)}\` (commit \`${t.commit.slice(0, 12)}\`)`}; binary sha256 \`${t.binary.sha256.slice(0, 12)}\`; ${esc(t.license)}`
        : "—";
    push(
      `| ${esc(t.name)} (\`${id}\`) | ${esc(t.pin)} | ${t.status === "available" ? "available" : `**${esc(t.reason)}**`} | ${esc(t.answers ?? "")} | ${prov} |`,
    );
  }
  push("");
  const toolArms = (oss.arms ?? []).filter((a) => !REFERENCE_ARMS[a]);
  const rows = summary.cells.flatMap((c) =>
    Object.entries(c.headline ?? {}).map(([arm, h]) => ({ c, arm, h })),
  );
  push("## Head-to-head with tsc -p (matched answers only)", "");
  if (!rows.length)
    push("_No row has a matched answer from a tool and a reproduced reference from tsc._", "");
  else {
    push(
      "| scenario | setting | tool | tool wall | tsc wall | wall | tsc single wall | wall vs single | tool peak MB | tsc peak MB | peak | tool CPU | tsc CPU |",
      "|---|---|---|---:|---:|---|---:|---|---:|---:|---|---:|---:|",
    );
    for (const { c, arm, h } of rows) {
      const t = c.arms[arm];
      const p = c.arms["tsc-measure"];
      const s = c.arms["tsc-measure-1"];
      const name = oss.tools?.[toolOfArm(arm)]?.name ?? arm;
      push(
        `| ${c.scenario} | ${c.setting} | ${esc(name)} | ${fmtMs(t.wallMs)} | ${fmtMs(p?.wallMs)} | ${word(h.parallel?.wallMs, name)} | ${fmtMs(s?.wallMs)} | ${word(h.single?.wallMs, name)} | ${fmtMb(t.peakBytes)} | ${fmtMb(p?.peakBytes)} | ${word(h.parallel?.peakBytes, name)} | ${fmtMs(t.cpuMs)} | ${fmtMs(p?.cpuMs)} |`,
      );
    }
    push("");
  }
  push("## Answers (every row)", "");
  push(
    `| scenario | setting | tsc 7.0.2 (measured) | tsc-measure | ${toolArms.map((a) => esc(oss.tools?.[toolOfArm(a)]?.name ?? a)).join(" | ")} |`,
    `|---|---|---|---|${toolArms.map(() => "---|").join("")}`,
  );
  for (const c of summary.cells) {
    const ref = c.reference
      ? c.reference.gap
        ? esc(c.reference.gap)
        : `${c.reference.errorAny ? "error any" : `\`${esc(c.reference.digest?.preview?.slice(0, 40))}\``}${c.reference.codes.length ? ` + TS${c.reference.codes.join("/TS")}` : ""}`
      : "no reference";
    const t = c.arms["tsc-measure"];
    push(
      `| ${c.scenario} | ${c.setting} | ${ref} | ${t?.status ?? "not run"} | ${toolArms
        .map((a) => {
          const s = c.arms[a];
          return s
            ? `**${s.class}**${s.detail ? ` ${esc(s.detail).slice(0, 110)}` : ""}`
            : "not run";
        })
        .join(" | ")} |`,
    );
  }
  push("");
  const counts = Object.entries(summary.classCounts ?? {})
    .map(
      ([id, c]) =>
        `${oss.tools?.[id]?.name ?? id}: ${Object.entries(c)
          .map(([k, n]) => `${k} ${n}`)
          .join(", ")}`,
    )
    .join("; ");
  if (counts) push(`Classes — ${counts}.`, "");
  if (!oss.validation?.ok) {
    push("## Validation failures of this section", "");
    for (const failure of oss.validation?.failures ?? []) push(`- ${esc(failure)}`);
    push("");
  }
  return lines.join("\n") + "\n";
}
