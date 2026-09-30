// The Biome comparison (`--biome`): semantic only. Biome has no type query;
// its type inference answers only through its type-aware lint rules, and the
// one rule that projects a declared type is `noFloatingPromises`. So every
// tool answers the same BOOLEAN projection of the benchmark's demand — "is
// the declared type of `__Probe` Promise-like?" — on the thenable catalog
// (thenable.mjs), against tsc 7.0.2's measured answer
// (thenable-expected.json). Each arm reads the same library and module; only
// the line that asks the question differs, since each tool has its own
// channel for it:
//
//   tsc-thenable     tsc -p on the module plus the measuring suffix (the
//                    reference's method; it must reproduce the reference)
//   verter-thenable  the Verter probe's `run` job on the module: the declared
//                    type of `__Probe`, then its projection
//   biome-types      biome lint (only noFloatingPromises) on the module plus
//                    an unhandled call returning `__Probe`
//
// A tool's answer counts only when its projection equals tsc's; a pair of
// programs (one Promise-like, one not, differing in one place) is decided
// only when both are. The timed unit is a cold process to a decided answer:
// start, read the files, answer, print, exit — each arm under the same
// supervisor, cap and deadline, in a counterbalanced schedule.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { probeMetrics, runLimits, stats, supervisorDeadlineMs, verdict } from "../analyze.mjs";
import { canonicalDigest, normalize, parseType } from "../canonical.mjs";
import { MEASURING_SUFFIX, parseMeasurement } from "../measure-expected.mjs";
import { sha256Text } from "../provenance.mjs";
import { interpretMeasurement } from "../reference.mjs";
import { SETTINGS } from "../scenarios.mjs";
import { supervisorRecordProblems } from "../supervisor.mjs";
import { firstLine, ossInvocationEnd } from "./checkers.mjs";
import { platformKey, provisioned, provisionTool } from "./provision.mjs";
import { BIOME_DEMAND, demandLine, isThenable, thenableCases } from "./thenable.mjs";

export const BIOME_SCHEMA = 1;
export const BIOME_ARMS = {
  "tsc-thenable": { label: "tsc -p on the module plus the measuring suffix (reference)" },
  "verter-thenable": { label: "Verter probe: the declared type of __Probe, projected" },
  "biome-types": {
    label: "Biome lint, noFloatingPromises only, on an unhandled call returning __Probe",
  },
};
/** The arms a run's options select: no tsc arm under `--no-tsc`. */
export const biomeArms = (options) =>
  Object.keys(BIOME_ARMS).filter((arm) => !(options?.noTsc && arm === "tsc-thenable"));

export const FLOATING_CATEGORY = "lint/nursery/noFloatingPromises";

/** Biome's configuration: only the rule whose decision projects the type. */
export function biomeConfig() {
  return {
    vcs: { enabled: false },
    formatter: { enabled: false },
    assist: { enabled: false },
    linter: { enabled: true, rules: { preset: "none", nursery: { noFloatingPromises: "error" } } },
  };
}

/** The interpreted expected answer of one case (thenable-expected.json). */
export function expectedFor(expected, id) {
  const raw = expected?.cases?.[id];
  if (!raw) return null;
  const answer = interpretMeasurement({
    never: raw.never,
    codes: raw.codes,
    printed: raw.printed ?? undefined,
  });
  return answer.gap
    ? { gap: answer.gap }
    : { digest: answer.digest, thenable: isThenable(answer.node), codes: raw.codes };
}

/**
 * Biome's answer: Promise-like exactly when noFloatingPromises reports the
 * demanded call's line; any other diagnostic (a parse error, the rule on
 * another line) means the output is not an answer to the demand.
 */
export function readBiomeAnswer(stdout, line) {
  const start = stdout.indexOf("{");
  if (start < 0) return { unreadable: "no JSON report" };
  let report;
  try {
    report = JSON.parse(stdout.slice(start, stdout.lastIndexOf("}") + 1));
  } catch (err) {
    return { unreadable: `the report is not JSON: ${String(err.message).slice(0, 120)}` };
  }
  if (!Array.isArray(report.diagnostics))
    return { unreadable: "the report has no diagnostics array" };
  let thenable = false;
  for (const d of report.diagnostics) {
    const path = String(d.location?.path ?? "").replace(/\\/g, "/");
    if (
      d.category === FLOATING_CATEGORY &&
      path.endsWith("scenario.ts") &&
      d.location?.start?.line === line
    )
      thenable = true;
    else
      return {
        unreadable: `a diagnostic outside the demand: ${d.category ?? "?"} at ${path}:${d.location?.start?.line ?? "?"} (${String(d.message ?? "").slice(0, 100)})`,
      };
  }
  return { thenable, biomeDurationNs: report.summary?.duration ?? null };
}

/** tsc's answer from its measuring run: the reference's own reading. */
export function readTscAnswer(stdout, source) {
  try {
    const parsed = parseMeasurement(stdout, source);
    const answer = interpretMeasurement({
      never: parsed.never,
      codes: parsed.codes,
      printed: parsed.printed ?? undefined,
    });
    if (answer.gap) return { unreadable: answer.gap };
    return { thenable: isThenable(answer.node), digest: answer.digest, codes: parsed.codes };
  } catch (err) {
    return { unreadable: String(err.message ?? err).slice(0, 200) };
  }
}

/** Verter's answer from its probe record: the full answer and its projection. */
export function readVerterAnswer(record) {
  const probe = record?.probes?.[0];
  const obs = probe?.observation;
  if (record?.stage !== "complete")
    return { unreadable: `the probe record is ${record?.stage ?? "missing"}` };
  if (probe?.cold?.outcome?.kind !== "value")
    return {
      unreadable: `the request answered ${probe?.cold?.outcome?.kind}${probe?.cold?.outcome?.detail ? `: ${probe.cold.outcome.detail}` : ""}`,
    };
  if (typeof obs?.text !== "string") return { unreadable: obs?.error ?? "no printed answer" };
  if (obs.unknownLeaves > 0 || obs.shape === "conditional")
    return {
      unreadable: "the answer is partial (an unmaterialised leaf or an unevaluated conditional)",
    };
  try {
    const node = normalize(parseType(obs.text));
    return { thenable: isThenable(node), digest: canonicalDigest(obs.text) };
  } catch (err) {
    return {
      unreadable: `the printed answer is not type syntax: ${String(err.message).slice(0, 120)}`,
    };
  }
}

function materialize(base, c, libText, tsconfigText) {
  const dir = join(base, "cases", c.id);
  const files = {
    measure: c.source + MEASURING_SUFFIX,
    verter: c.source,
    biome: c.source + BIOME_DEMAND,
  };
  for (const [kind, source] of Object.entries(files)) {
    mkdirSync(join(dir, kind), { recursive: true });
    writeFileSync(join(dir, kind, "lib.bench.d.ts"), libText);
    writeFileSync(join(dir, kind, "scenario.ts"), source);
    writeFileSync(join(dir, kind, "tsconfig.json"), tsconfigText);
  }
  writeFileSync(join(dir, "biome", "biome.json"), JSON.stringify(biomeConfig(), null, 2) + "\n");
  return {
    dir,
    inputs: Object.fromEntries(Object.entries(files).map(([k, s]) => [k, sha256Text(s)])),
    lib: sha256Text(libText),
  };
}

function command(arm, ctx, dir, runBase) {
  if (arm === "tsc-thenable")
    return {
      argv: [ctx.typescript.exe, "-p", join(dir, "measure", "tsconfig.json")],
      cwd: join(dir, "measure"),
      probeOut: null,
    };
  if (arm === "verter-thenable") {
    const job = {
      schema: 1,
      dir: join(dir, "verter"),
      tsconfig: "tsconfig.json",
      lib: "lib.bench.d.ts",
      libMode: ctx.opts.libMode ?? "root-file",
      scenario: "scenario.ts",
      initAlias: "__BenchInit",
      probes: ["__Probe"],
      warmRepeats: 0,
      observability: false,
    };
    writeFileSync(`${runBase}.job.json`, JSON.stringify(job, null, 2));
    return {
      argv: [
        ctx.verterProbe,
        "run",
        "--job",
        `${runBase}.job.json`,
        "--out",
        `${runBase}.probe.json`,
      ],
      cwd: join(dir, "verter"),
      probeOut: `${runBase}.probe.json`,
    };
  }
  return {
    argv: [
      ctx.biome.binary,
      "lint",
      `--config-path=${join(dir, "biome")}`,
      "--reporter=json",
      "--max-diagnostics=none",
      "--colors=off",
      "lib.bench.d.ts",
      "scenario.ts",
    ],
    cwd: join(dir, "biome"),
    probeOut: null,
  };
}

/** Read one invocation's answer (or why there is none). */
function readAnswer(arm, c, record, probeOut) {
  try {
    if (arm === "verter-thenable") {
      if (record.exitCode !== 0) return { unreadable: `exit ${record.exitCode}` };
      return readVerterAnswer(JSON.parse(readFileSync(probeOut, "utf8")));
    }
    if (![0, 1, 2].includes(record.exitCode)) {
      const line = firstLine(
        existsSync(record.stderrPath) ? readFileSync(record.stderrPath, "utf8") : "",
      );
      return { unreadable: `exit ${record.exitCode}${line ? `: ${line}` : ""}` };
    }
    const stdout = readFileSync(record.stdoutPath, "utf8");
    const read =
      arm === "tsc-thenable"
        ? readTscAnswer(stdout, c.source + MEASURING_SUFFIX)
        : readBiomeAnswer(stdout, demandLine(c.source));
    return { ...read, stdoutSha256: sha256Text(stdout) };
  } catch (err) {
    return { unreadable: String(err.message ?? err).slice(0, 200) };
  }
}

/**
 * Run the Biome comparison. `ctx`: root, outDir, opts, tools, supervisor,
 * verterProbe, typescript, runtimeEnv, libText, tsconfigText, schedule,
 * runSupervised, log.
 */
export async function runBiome(ctx) {
  const { root, outDir, opts, tools, log } = ctx;
  const base = join(outDir, "biome");
  mkdirSync(base, { recursive: true });
  let biome = provisioned(root, "biome", tools.biome);
  if (biome.unavailable && /not provisioned/.test(biome.unavailable)) {
    await provisionTool(root, "biome", tools.biome, { supervisor: ctx.supervisor, log });
    biome = provisioned(root, "biome", tools.biome);
  }
  const tool = biome.ok
    ? {
        status: "available",
        name: tools.biome.name,
        pin: tools.biome.pin,
        commit: tools.biome.commit,
        license: tools.biome.license,
        source: biome.provenance.source,
        binary: { path: biome.binary, sha256: biome.sha256 },
      }
    : {
        status: "unavailable",
        name: tools.biome.name,
        pin: tools.biome.pin,
        reason: biome.unavailable,
      };
  const result = { schema: BIOME_SCHEMA, platform: platformKey(), tool };
  if (!biome.ok) {
    log(`semantic-perf: --biome: ${biome.unavailable}`);
    return result;
  }
  const strict = SETTINGS.find((s) => s.id === "strict");
  const cases = thenableCases();
  const cells = {};
  for (const c of cases) {
    const m = materialize(base, c, ctx.libText, ctx.tsconfigText(strict));
    cells[c.id] = {
      id: c.id,
      pair: c.pair,
      variant: c.variant,
      family: c.family,
      note: c.note,
      ...m,
    };
  }
  const arms = biomeArms(opts);
  const plan = ctx.schedule(Object.keys(cells), arms, opts.repeat, opts.warmup);
  const c2 = { ...ctx, biome };
  const invocations = [];
  const byId = new Map(cases.map((c) => [c.id, c]));
  for (const [index, step] of plan.entries()) {
    const c = byId.get(step.key);
    const runDir = join(base, "runs", step.key, step.arm);
    mkdirSync(runDir, { recursive: true });
    const runBase = join(runDir, `${step.warmup ? "warmup" : "rep"}-${step.rep}`);
    const { argv, cwd, probeOut } = command(step.arm, c2, cells[step.key].dir, runBase);
    const out = `${runBase}.sup.json`;
    const r = await ctx.runSupervised(ctx.supervisor, {
      memMb: opts.memMb + opts.infraMb,
      timeoutMs: supervisorDeadlineMs(opts),
      out,
      env: ctx.runtimeEnv,
      cwd,
      argv,
      allowSampled: opts.allowSampled,
    });
    const record = r.record
      ? { ...r.record, samples: undefined, sampleCount: r.record.samples?.length ?? 0 }
      : null;
    const reading =
      record?.launched && !record.killedBy ? readAnswer(step.arm, c, record, probeOut) : null;
    let probe = null;
    if (probeOut && existsSync(probeOut)) {
      try {
        probe = JSON.parse(readFileSync(probeOut, "utf8"));
      } catch {
        probe = null;
      }
    }
    invocations.push({
      index,
      case: step.key,
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command: argv,
      supervisorOut: out,
      supervisorExit: r.supervisorExit,
      supervisor: record,
      reading,
      firstTypeMs: probe ? (probeMetrics({ probe })?.firstTypeMs ?? null) : null,
    });
    log(
      `[biome ${index + 1}/${plan.length}] ${step.key} ${step.arm}: ${record?.killedBy ? `killed:${record.killedBy}` : `exit ${record?.exitCode ?? "?"}`} ${record?.wallMs?.toFixed?.(0) ?? "?"} ms${reading?.unreadable ? ` (${reading.unreadable.slice(0, 80)})` : reading ? ` thenable=${reading.thenable}` : ""}`,
    );
  }
  return {
    ...result,
    arms,
    cells,
    plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    invocations,
    toolAfter: provisioned(root, "biome", tools.biome).sha256 ?? null,
  };
}

// ---------------------------------------------------------------- summary and validation

function metrics(inv) {
  const s = inv.supervisor ?? {};
  return {
    wallMs: s.wallMs ?? null,
    peakBytes: s.peakBytes ?? null,
    cpuMs:
      typeof s.cpuUserMs === "number" && typeof s.cpuKernelMs === "number"
        ? s.cpuUserMs + s.cpuKernelMs
        : null,
    firstTypeMs: inv.firstTypeMs ?? null,
  };
}

const CLASS_ORDER = [
  "killed",
  "error",
  "unverified",
  "unreadable",
  "mismatch",
  "no-reference",
  "matched",
];

/** One invocation's class against the expected answer. */
export function biomeInvocationClass(inv, expected, limits) {
  const end = ossInvocationEnd(inv, limits);
  if (end.kind === "killed") return { class: "killed", detail: end.detail };
  if (end.kind === "unattributed-kill") return { class: "unverified", detail: end.detail };
  if (end.kind !== "exited") return { class: "error", detail: end.detail ?? end.kind };
  if (
    typeof inv.supervisor?.peakBytes === "number" &&
    inv.supervisor.peakBytes > limits.budgetBytes
  )
    return { class: "killed", detail: "the process peak exceeds the budget" };
  const read = inv.reading ?? { unreadable: "no output was read" };
  if (read.unreadable) return { class: "unreadable", detail: read.unreadable };
  if (!expected || expected.gap)
    return { class: "no-reference", detail: expected?.gap ?? "no measured reference" };
  if (read.thenable !== expected.thenable)
    return {
      class: "mismatch",
      detail: `answered ${read.thenable ? "Promise-like" : "not Promise-like"}, tsc ${expected.thenable ? "Promise-like" : "not Promise-like"}`,
    };
  // The full answer, where the tool gives one, must be tsc's too.
  if (read.digest && read.digest.sha256 !== expected.digest.sha256)
    return {
      class: "mismatch",
      detail: `the projection agrees but the answer is ${read.digest.preview}, tsc ${expected.digest.preview}`,
    };
  return {
    class: "matched",
    detail: read.digest ? "the full answer is tsc's" : "the projection is tsc's",
  };
}

/** The summary of the Biome comparison, derived from its records only. */
export function summarizeBiome(result, expectedFile, options) {
  if (result.tool?.status !== "available")
    return { status: "unavailable", reason: result.tool?.reason ?? null };
  const limits = runLimits(options);
  const groups = new Map();
  for (const inv of result.invocations ?? []) {
    if (!groups.has(inv.case)) groups.set(inv.case, {});
    (groups.get(inv.case)[inv.arm] ??= []).push(inv);
  }
  const tscRan = (result.arms ?? []).includes("tsc-thenable");
  const cells = [];
  for (const [id, cell] of Object.entries(result.cells ?? {})) {
    const expected = expectedFor(expectedFile, id);
    const arms = {};
    for (const [arm, invs] of Object.entries(groups.get(id) ?? {})) {
      const results = invs.map((i) => biomeInvocationClass(i, expected, limits));
      const classes = [...new Set(results.map((r) => r.class))];
      const worst = CLASS_ORDER.find((c) => classes.includes(c)) ?? classes[0];
      const measured = invs.filter((i) => !i.warmup);
      const m = measured.map(metrics);
      arms[arm] = {
        class: worst,
        repetitionsDiffer: classes.length > 1,
        detail: results.find((r) => r.class === worst)?.detail ?? "",
        wallMs: stats(m.map((x) => x.wallMs)),
        peakBytes: stats(m.map((x) => x.peakBytes)),
        cpuMs: stats(m.map((x) => x.cpuMs)),
        firstTypeMs: arm === "verter-thenable" ? stats(m.map((x) => x.firstTypeMs)) : null,
      };
    }
    const pick = (arm, key) =>
      (groups.get(id)?.[arm] ?? []).filter((i) => !i.warmup).map((i) => metrics(i)[key]);
    const ok = (arm) => arms[arm]?.class === "matched" && !arms[arm].repetitionsDiffer;
    // The tsc arm, where it ran, must reproduce the reference for the row to count.
    const headline =
      (!tscRan || ok("tsc-thenable")) && ok("verter-thenable") && ok("biome-types")
        ? {
            wallMs: verdict(pick("verter-thenable", "wallMs"), pick("biome-types", "wallMs"), 1),
            peakBytes: verdict(
              pick("verter-thenable", "peakBytes"),
              pick("biome-types", "peakBytes"),
              0,
            ),
            cpuMs: verdict(pick("verter-thenable", "cpuMs"), pick("biome-types", "cpuMs"), 1),
          }
        : null;
    cells.push({
      id,
      pair: cell.pair,
      variant: cell.variant,
      family: cell.family,
      note: cell.note,
      expected: expected
        ? {
            thenable: expected.thenable ?? null,
            preview: expected.digest?.preview ?? null,
            gap: expected.gap ?? null,
          }
        : null,
      arms,
      headline,
    });
  }
  const pairs = {};
  for (const c of cells) {
    const p = (pairs[c.pair] ??= {
      pair: c.pair,
      family: c.family,
      note: c.note,
      verter: true,
      biome: true,
    });
    p.verter &&=
      c.arms["verter-thenable"]?.class === "matched" &&
      !c.arms["verter-thenable"].repetitionsDiffer;
    p.biome &&=
      c.arms["biome-types"]?.class === "matched" && !c.arms["biome-types"].repetitionsDiffer;
  }
  const count = (arm) => cells.filter((c) => c.arms[arm]?.class === "matched").length;
  return {
    status: "compared",
    cells,
    pairs: Object.values(pairs),
    matched: {
      verter: count("verter-thenable"),
      biome: count("biome-types"),
      tsc: tscRan ? count("tsc-thenable") : null,
      of: cells.length,
    },
  };
}

const stable = (value) =>
  JSON.stringify(value, (_k, v) =>
    typeof v === "number" && !Number.isInteger(v) ? Number(v.toPrecision(12)) : v,
  );

/**
 * Re-read every Biome-section invocation's raw files (the supervisor record,
 * the stdout or probe record it names) and re-derive its reading: a stored
 * copy that differs is reported.
 */
export function biomeRawFileProblems(result) {
  const problems = [];
  const cases = new Map(thenableCases().map((c) => [c.id, c]));
  for (const inv of result.invocations ?? []) {
    const id = `biome ${inv.case}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
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
    if (!inv.reading) continue;
    const probeOut =
      inv.arm === "verter-thenable"
        ? inv.supervisorOut.replace(/\.sup\.json$/, ".probe.json")
        : null;
    const again = readAnswer(inv.arm, cases.get(inv.case), sup, probeOut);
    if (stable(again) !== stable(inv.reading))
      problems.push(`${id}: the reading re-derived from the raw output differs`);
  }
  return problems;
}

/**
 * Validate the Biome comparison: harness problems only (a binary that
 * changed, inputs that are not the catalog's, a plan that is not the
 * schedule, a missing or failed record, the wrong cap, the tsc arm not
 * reproducing the reference, a stored summary that disagrees with its
 * records). A tool's wrong or unreadable answer is a finding.
 */
export function validateBiome(result, expectedFile, options, schedule) {
  const failures = [];
  const fail = (m) => failures.push(`biome: ${m}`);
  if (result?.schema !== BIOME_SCHEMA)
    return { ok: false, failures: [`biome: schema ${result?.schema}`] };
  if (result.tool?.status !== "available") {
    if (!/^unavailable on /.test(result.tool?.reason ?? ""))
      fail("the tool is neither available nor unavailable with a reason");
    return { ok: failures.length === 0, failures };
  }
  if (result.toolAfter !== result.tool.binary.sha256)
    fail("wrong binary: Biome changed during the run");
  if (expectedFile?.method?.measuringSuffixSha256 !== sha256Text(MEASURING_SUFFIX))
    fail("the thenable reference used another measuring method");
  const cases = new Map(thenableCases().map((c) => [c.id, c]));
  for (const [id, cell] of Object.entries(result.cells ?? {})) {
    const c = cases.get(id);
    if (!c) {
      fail(`${id}: not a catalog case`);
      continue;
    }
    if (
      cell.inputs?.measure !== sha256Text(c.source + MEASURING_SUFFIX) ||
      cell.inputs?.verter !== sha256Text(c.source) ||
      cell.inputs?.biome !== sha256Text(c.source + BIOME_DEMAND)
    )
      fail(`${id}: the programs are not the catalog's`);
    const raw = expectedFile?.cases?.[id];
    if (!raw) fail(`${id}: no measured reference`);
    else {
      if (raw.sourceSha256 !== sha256Text(c.source))
        fail(`${id}: the reference is stale (measured on another source)`);
      if (expectedFile.method?.libSha256 !== cell.lib)
        fail(`${id}: the reference used another library`);
    }
  }
  if (stable(result.arms) !== stable(biomeArms(options)))
    fail(
      `the arms are not ${options?.noTsc ? "the tool arms (--no-tsc)" : "every Biome-section arm"}`,
    );
  const want = schedule(
    Object.keys(result.cells ?? {}),
    result.arms ?? [],
    options.repeat,
    options.warmup,
  ).map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`);
  if (stable(want) !== stable(result.plan))
    fail("the recorded plan is not the counterbalanced schedule");
  const cap = (options.memMb + options.infraMb) * 1024 * 1024;
  const invs = result.invocations ?? [];
  if (invs.length !== want.length)
    fail(`${invs.length} records for ${want.length} planned invocations`);
  invs.forEach((inv, position) => {
    const key = `${inv.case}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
    if (inv.index !== position || result.plan?.[position] !== key)
      fail(`record ${position} is ${key}; the plan says ${result.plan?.[position]}`);
    for (const p of supervisorRecordProblems(inv.supervisor)) fail(`${key}: ${p}`);
    const sup = inv.supervisor;
    if (!sup) return;
    if (sup.containment !== "hard" && !(sup.containment === "sampled" && options.allowSampled))
      fail(`${key}: containment ${sup.containment} without consent`);
    if (sup.memLimitBytes !== cap)
      fail(`${key}: containment cap ${sup.memLimitBytes} is not the run's (${cap})`);
    if (sup.timeoutMs !== supervisorDeadlineMs(options))
      fail(`${key}: deadline ${sup.timeoutMs} is not the run's`);
    if (ossInvocationEnd(inv, runLimits(options)).kind === "harness-failure")
      fail(`${key}: failed child`);
    if (inv.arm === "biome-types" && inv.command?.[0] !== result.tool.binary.path)
      fail(`${key}: ran another Biome than the provisioned one`);
  });
  const summary = summarizeBiome(result, expectedFile, options);
  for (const cell of summary.cells ?? []) {
    const t = cell.arms["tsc-thenable"];
    if (t && t.class !== "matched" && t.class !== "killed")
      fail(
        `${cell.id}|tsc-thenable: tsc does not reproduce its measured reference (${t.class}: ${t.detail})`,
      );
    if (t?.repetitionsDiffer) fail(`${cell.id}|tsc-thenable: inconsistent repetitions`);
  }
  if (stable(result.summary) !== stable(summary))
    fail("the stored summary disagrees with its raw records");
  return { ok: failures.length === 0, failures };
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
const word = (v) =>
  !v || v.verdict === "n/a"
    ? "—"
    : v.verdict === "overlap"
      ? "overlap"
      : `${v.verdict === "verter" ? "Verter" : "Biome"}${v.ratio && v.ratioMeaningful !== false ? ` (×${v.ratio.toFixed(2)})` : ""}`;

/** The Biome sections of results.md (only in a run with `--biome`). */
export function renderBiomeMarkdown(result) {
  const lines = [];
  const push = (...l) => lines.push(...l);
  push("# Biome vs Verter (`--biome`): semantic answers on the thenable catalog", "");
  push(`Validation of this section: **${result.validation?.ok ? "PASSED" : "FAILED"}**`, "");
  const t = result.tool ?? {};
  if (t.status !== "available") {
    push(`Biome ${esc(t.pin)}: **${esc(t.reason)}**`, "");
    return lines.join("\n") + "\n";
  }
  push(
    `- Biome ${esc(t.pin)} (commit \`${String(t.commit).slice(0, 12)}\`, release file sha256 \`${String(t.source?.sha256).slice(0, 12)}\`, binary sha256 \`${t.binary.sha256.slice(0, 12)}\`, ${esc(t.license)}).`,
    "- Biome has no type query: its type inference answers only through its type-aware lint rules. Every tool therefore answers the same **boolean projection** of the demand — is the declared type of `__Probe` Promise-like? — Biome through `noFloatingPromises` on an unhandled call returning `__Probe`, Verter through its declared type (which must also equal tsc's in full), tsc through the reference's measuring program. The expected answer is tsc 7.0.2's, measured.",
    "- Every feature is a **pair** of programs differing in one place, one Promise-like and one not: a tool that does not evaluate the feature answers both alike and cannot decide the pair.",
    "- Timed unit: a **cold process to a decided answer** (start, read the library and the module, answer, print, exit), every arm under the same supervisor, cap and deadline, counterbalanced. Verter's process also reads its own OS statistics and prints the full answer; Biome's builds its project scan. Verter's in-engine first type handle is shown apart. A row is compared only when every arm that ran matched (tsc's arm does not run under `--no-tsc`); a verdict names a winner only when every measured repetition of one arm beats every repetition of the other (by more than 1 ms for times); ×N is Biome's median over Verter's (above 1 favours Verter).",
    "",
  );
  const s = result.summary ?? {};
  push(
    `Matched: Verter ${s.matched?.verter ?? 0}, Biome ${s.matched?.biome ?? 0}, tsc ${s.matched?.tsc ?? "not run"} of ${s.matched?.of ?? 0} programs.`,
    "",
  );
  push("## Pairs (a feature is decided when both of its programs are answered as tsc answers)", "");
  push("| pair | family | feature | Verter | Biome |", "|---|---|---|---|---|");
  for (const p of s.pairs ?? [])
    push(
      `| ${p.pair} | ${p.family} | ${esc(p.note)} | ${p.verter ? "decided" : "**not decided**"} | ${p.biome ? "decided" : "**not decided**"} |`,
    );
  push("");
  push("## Answers and times (every program)", "");
  push(
    "| program | tsc (measured) | tsc arm | Verter | Biome | Verter process wall | Biome process wall | wall | Verter peak MB | Biome peak MB | peak | Verter first type handle | tsc -p wall |",
    "|---|---|---|---|---|---:|---:|---|---:|---:|---|---:|---:|",
  );
  for (const c of s.cells ?? []) {
    const v = c.arms["verter-thenable"];
    const b = c.arms["biome-types"];
    const tt = c.arms["tsc-thenable"];
    const cls = (a) =>
      a
        ? `**${a.class}**${a.class === "matched" ? "" : ` ${esc(a.detail).slice(0, 90)}`}`
        : "not run";
    push(
      `| ${c.id} | ${c.expected?.gap ? esc(c.expected.gap) : `\`${esc(c.expected?.preview)}\` (${c.expected?.thenable ? "Promise-like" : "not Promise-like"})`} | ${tt?.class ?? "not run"} | ${cls(v)} | ${cls(b)} | ${fmtMs(v?.wallMs)} | ${fmtMs(b?.wallMs)} | ${word(c.headline?.wallMs)} | ${fmtMb(v?.peakBytes)} | ${fmtMb(b?.peakBytes)} | ${word(c.headline?.peakBytes)} | ${fmtMs(v?.firstTypeMs)} | ${fmtMs(tt?.wallMs)} |`,
    );
  }
  push("");
  if (!result.validation?.ok) {
    push("## Validation failures of this section", "");
    for (const failure of result.validation?.failures ?? []) push(`- ${esc(failure)}`);
    push("");
  }
  return lines.join("\n") + "\n";
}
