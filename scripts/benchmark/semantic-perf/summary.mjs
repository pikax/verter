// The per-(scenario, setting) summary, derived only from raw invocation
// records and the measured reference. The validator recomputes it and fails
// a run whose stored summary differs.

import {
  ARMS,
  classifyVerterAnswer,
  invocationEnd,
  parseCli,
  probeAnswer,
  probeMetrics,
  stats,
  tscAnswerStatus,
  verdict,
  runLimits,
  CLASSES,
} from "./analyze.mjs";
import { canonicalDigest } from "./canonical.mjs";
import { interpretMeasurement, RESOURCE_CODES } from "./reference.mjs";
import { requiredStateComparison, summarizeSessions } from "./session-analyze.mjs";
import { allSessions } from "./sessions.mjs";

const referenceCache = new Map();
/** The interpreted reference of one (scenario, setting), or null. */
export function referenceFor(expected, scenarioId, settingId) {
  const raw = expected.scenarios?.[scenarioId]?.settings?.[settingId];
  if (!raw) return null;
  const key = `${expected.method?.measuringSuffixSha256}|${scenarioId}|${settingId}`;
  if (!referenceCache.has(key) || referenceCache.get(key).raw !== raw) {
    let answer;
    try {
      answer = interpretMeasurement(raw);
    } catch (err) {
      answer = {
        gap: `uninterpretable reference: ${err.message}`,
        unmeasurable: "uninterpretable",
        codes: raw.codes ?? [],
      };
    }
    referenceCache.set(key, { raw, answer });
  }
  return referenceCache.get(key).answer;
}

const digestCache = new Map();
function digestOf(text) {
  if (!digestCache.has(text)) {
    let digest = null;
    try {
      digest = canonicalDigest(text);
    } catch {
      digest = null;
    }
    digestCache.set(text, digest);
  }
  return digestCache.get(text);
}
/** The digest of a scenario's probe expression itself (an unevaluated answer). */
export const probeDigest = (scenario) => (scenario?.probe ? digestOf(scenario.probe) : null);
/** The digest of a scenario's constructed answer past tsc's limit, or null. */
export const beyondDigest = (scenario) => (scenario?.beyond ? digestOf(scenario.beyond) : null);

const PROBE_METRICS = [
  "engineStartMs",
  "setupMs",
  "initMs",
  "coldMs",
  "firstTypeMs",
  "warmMs",
  "observeMs",
  "teardownMs",
  "peakBytes",
  "retainedBytes",
  "observePeakBytes",
  "cpuMs",
];

function probeArmSummary(arm, invs, ctx) {
  const measured = invs.filter((i) => !i.warmup);
  const answers = invs.map((i) => probeAnswer(i, ctx.limits));
  const digests = [
    ...new Set(answers.map((a) => a.digest?.sha256 ?? `<${a.end.kind}:${a.outcome?.kind ?? ""}>`)),
  ];
  const out = {
    invocations: invs.length,
    measured: measured.length,
    answerDigest: answers.find((a) => a.digest)?.digest ?? null,
    distinctAnswers: digests.length,
    ends: [...new Set(answers.map((a) => a.end.kind))],
  };
  if (ARMS[arm].tool === "verter") {
    const classes = answers.map((a) => classifyVerterAnswer(a, ctx));
    out.classes = [...new Set(classes.map((c) => c.class))];
    // Repetitions that disagree are a finding about Verter, reported as the
    // worst class among them (never a comparison: only `matched` is).
    const worst = CLASSES.find((c) => out.classes.includes(c)) ?? out.classes[0];
    out.class = worst;
    out.repetitionsDiffer = out.classes.length > 1 || digests.length > 1;
    const worstDetail = classes.find((c) => c.class === worst)?.detail ?? "";
    out.detail = out.repetitionsDiffer
      ? `repetitions differ (${out.classes.join(", ")}; ${digests.length} distinct answers): ${worstDetail}`
      : worstDetail;
  } else {
    const statuses = answers.map((a) =>
      tscAnswerStatus(a, ctx.reference, ctx.beyond, ctx.budgetBytes),
    );
    out.referenceProblems = [...new Set(statuses.filter((s) => s.problem).map((s) => s.problem))];
    const kinds = [...new Set(statuses.map((s) => s.status))];
    out.class = out.referenceProblems.length
      ? "inconsistent-with-reference"
      : kinds.length === 1
        ? {
            killed: "killed",
            reference: "reference",
            "by-construction": "reference-by-construction",
            unverified: "unverified",
          }[kinds[0]]
        : "inconsistent";
    out.detail = statuses.find((s) => s.detail)?.detail ?? "";
    out.errorType = answers.find((a) => a.digest)?.errorType ?? null;
  }
  // Statistics only over invocations whose demand completed; the validator
  // fails a run where a completed invocation lacks any of them.
  const completed = measured.filter((i) =>
    ["exited", "observe-killed"].includes(probeAnswer(i, ctx.limits).end.kind),
  );
  const metrics = completed.map(probeMetrics);
  out.completedMeasured = completed.length;
  out.metrics = Object.fromEntries(
    PROBE_METRICS.map((m) => [m, stats(metrics.map((x) => x?.[m]))]),
  );
  // Warm: every in-process repeat of the one live process that made them.
  out.metrics.warmMs = stats(metrics.flatMap((x) => x?.warmSamplesMs ?? []));
  // Every completed measurement's memory metric (the validator requires one
  // metric across both headline arms).
  out.memoryMetrics = [...new Set(metrics.map((m) => m?.memoryMetric ?? "<none>"))];
  out.memoryMetric = out.memoryMetrics.length === 1 ? out.memoryMetrics[0] : null;
  if (ARMS[arm].tool === "tsc") {
    for (const m of ["spawnMs", "setupRoundTripMs", "coldRoundTripMs", "warmRoundTripMs"])
      out.metrics[m] = stats(metrics.map((x) => x?.[m]));
  }
  if (arm === "verter-counted") {
    out.coldAllocations = stats(metrics.map((x) => x?.allocations?.[0]));
    out.coldAllocatedBytes = stats(metrics.map((x) => x?.allocations?.[1]));
  }
  if (arm === "verter") out.retention = metrics[0]?.retention ?? null;
  // Every completed measurement's retained state: the observe build must
  // retain the production build's REQUIRED state.
  if (arm === "verter" || arm === "verter-observe")
    out.retentions = metrics.map((m) => m?.retention ?? null);
  out.invocationWallMs = stats(measured.map((i) => i.supervisor?.wallMs));
  out.invocationTreePeakBytes = stats(measured.map((i) => i.supervisor?.peakBytes));
  return out;
}

const CLI_ENGINE_PEAK_METRICS = new Map([
  ["windows-job-object", "job-peak-commit-charge"],
  ["macos-phys-footprint", "sampled-tree-phys-footprint-sum"],
]);

function cliArmSummary(invs, limits) {
  const budgetBytes = limits.budgetBytes;
  const measured = invs.filter((i) => !i.warmup);
  const ends = invs.map((i) => invocationEnd(i, limits));
  const parsed = invs.map((i, k) =>
    ends[k].kind === "exited" ? parseCli(i.cliStdout ?? "") : null,
  );
  const codeSets = [
    ...new Set(parsed.map((p, k) => (p ? p.codes.join(",") : `<${ends[k].kind}>`))),
  ];
  const completed = measured.filter((i) => invocationEnd(i, limits).kind === "exited");
  const measuredParsed = completed.map((i) => parseCli(i.cliStdout ?? ""));
  // Cgroup peaks include cache and kernel charges; they are containment telemetry,
  // not the CLI engine's memory. Only known backend/metric pairs are attributable.
  const memory = completed.filter(
    (i) =>
      CLI_ENGINE_PEAK_METRICS.has(i.supervisor?.backend) &&
      CLI_ENGINE_PEAK_METRICS.get(i.supervisor?.backend) === i.supervisor?.peakMetric &&
      ["hard", "sampled"].includes(i.supervisor?.containment),
  );
  const overBudget = memory.filter(
    (i) => typeof i.supervisor?.peakBytes === "number" && i.supervisor.peakBytes > budgetBytes,
  ).length;
  // A statistic over the attributable subset is not the cell's engine memory:
  // unless every completed invocation is attributable, the cell refuses to
  // publish one (peak bytes, peak metric and the budget verdict alike).
  const attributable = memory.length === completed.length;
  const killed = invs.filter((i) =>
    ["killed", "unattributed-kill"].includes(invocationEnd(i, limits).kind),
  );
  return {
    invocations: invs.length,
    measured: measured.length,
    status:
      killed.length === invs.length
        ? `killed (${invocationEnd(killed[0], limits).detail})`
        : killed.length
          ? "inconsistent"
          : attributable && overBudget
            ? "over the engine budget"
            : "completed",
    ends: [...new Set(ends.map((e) => e.kind))],
    codes: parsed.find(Boolean)?.codes ?? null,
    distinctCodeSets: codeSets.length,
    wallMs: stats(completed.map((i) => i.supervisor?.wallMs)),
    terminationMs: stats(
      measured
        .filter((i) => invocationEnd(i, limits).kind === "killed" && !i.skipped)
        .map((i) => i.supervisor?.wallMs),
    ),
    peakBytes: attributable ? stats(memory.map((i) => i.supervisor?.peakBytes)) : null,
    peakMetric: attributable ? (memory[0]?.supervisor?.peakMetric ?? null) : null,
    memoryUnavailable: attributable
      ? null
      : "supervisor accounting is not attributable to the engine",
    cpuMs: stats(
      completed.map((i) => (i.supervisor?.cpuUserMs ?? NaN) + (i.supervisor?.cpuKernelMs ?? NaN)),
    ),
    tscMemoryUsedBytes: stats(measuredParsed.map((p) => p.memoryUsedBytes)),
    tscCheckMs: stats(measuredParsed.map((p) => p.checkMs)),
    tscTotalMs: stats(measuredParsed.map((p) => p.totalMs)),
  };
}

/**
 * The resolution below which a timing difference is not claimed. Every tsc
 * probe times a dedicated series of 20 trivial warm requests after its
 * measurement.
 *
 * - If none reads 0 ms the clock resolves the trivial request: the quantum
 *   is at most the smallest calibration time (basis "calibration").
 * - If any reads 0 ms the clock is coarse, and the calibration only shows
 *   that the quantum exceeds the trivial request; it cannot bound it. The
 *   quantum is then ESTIMATED as the smallest positive tsc server time of the
 *   run (basis "workload heuristic", labelled so in the report): it depends
 *   on which requests the run happened to time.
 *
 * Verter's timer is in microseconds. One request: at least 1 ms and two
 * quanta (a difference of two readings); a sum of three separately timed
 * requests (first type handle): at least 2 ms and four quanta. This is a
 * descriptive threshold, not a confidence bound.
 */
export function timerResolution(run) {
  const calibration = [];
  const server = [];
  for (const inv of run.invocations ?? []) {
    const r = inv.probe;
    if (inv.arm !== "tsc-api" || r?.tool !== "tsc") continue;
    for (const t of r.calibration?.serverMs ?? [])
      if (typeof t === "number" && Number.isFinite(t)) calibration.push(t);
    const p = r.probes?.[0];
    for (const t of [
      r.phases?.engineStartMs,
      r.phases?.setupMs,
      r.init?.serverMs,
      p?.cold?.serverMs,
      ...(p?.warm ?? []).map((w) => w.serverMs),
      ...calibration,
    ]) {
      if (typeof t === "number" && Number.isFinite(t) && t > 0) server.push(t);
    }
  }
  const zeroShare = calibration.length
    ? calibration.filter((t) => t === 0).length / calibration.length
    : null;
  const coarse = zeroShare !== null && zeroShare > 0;
  const pool = coarse ? server : calibration.filter((t) => t > 0);
  const quantum = pool.length ? Math.min(...pool) : null;
  return {
    tscQuantumMs: quantum,
    clock: calibration.length === 0 ? "uncalibrated" : coarse ? "coarse" : "fine",
    basis: calibration.length === 0 ? "none" : coarse ? "workload heuristic" : "calibration",
    calibrationSamples: calibration.length,
    calibrationZeroShare: zeroShare,
    single: Math.max(1, 2 * (quantum ?? 0)),
    sum: Math.max(2, 4 * (quantum ?? 0)),
  };
}

/** The fixed floors of the resolution (the report states them). */
export const TIME_RESOLUTION_MS = { single: 1, sum: 2 };

function comparison(verterInvs, tscInvs, resolution) {
  const v = verterInvs.filter((i) => !i.warmup).map(probeMetrics);
  const t = tscInvs.filter((i) => !i.warmup).map(probeMetrics);
  const pick = (xs, m) => xs.map((x) => x?.[m]);
  const out = {};
  for (const m of ["coldMs", "setupMs", "initMs"])
    out[m] = verdict(pick(v, m), pick(t, m), resolution.single);
  // Warm: the in-process repeats of each arm's one live process.
  out.warmMs = verdict(
    v.flatMap((x) => x?.warmSamplesMs ?? []),
    t.flatMap((x) => x?.warmSamplesMs ?? []),
    resolution.single,
  );
  out.firstTypeMs = verdict(pick(v, "firstTypeMs"), pick(t, "firstTypeMs"), resolution.sum);
  for (const m of ["peakBytes", "retainedBytes"]) out[m] = verdict(pick(v, m), pick(t, m), 0);
  return out;
}

/**
 * The Capacity row of one cell: each arm's outcome at the engine budget —
 * its class or status, its engine peak and time when it completed, and for
 * the invocations the supervisor killed, the tree's peak at the kill and the
 * time to it (with whether the kill is attributed to the engine). The
 * memory cap stays the machine's protection: nothing here raises it.
 */
function capacityRow(cell, armInvs, limits) {
  const arm = (name) => {
    const s = cell.arms[name];
    if (!s) return null;
    const kills = (armInvs[name] ?? []).filter(
      (i) => !i.skipped && ["killed", "unattributed-kill"].includes(invocationEnd(i, limits).kind),
    );
    return {
      outcome: s.class ?? s.status,
      peakBytes: s.metrics?.peakBytes ?? s.peakBytes ?? null,
      timeMs: s.metrics?.firstTypeMs ?? s.wallMs ?? null,
      memoryUnavailable: s.memoryUnavailable ?? null,
      kills: kills.length,
      attributedKills: kills.filter((i) => invocationEnd(i, limits).kind === "killed").length,
      killedBy: [...new Set(kills.map((i) => i.supervisor?.killedBy))].sort(),
      peakAtKillBytes: stats(kills.map((i) => i.supervisor?.peakBytes)),
      peakAtKillMetric: kills[0]
        ? `${kills[0].supervisor?.backend}:${kills[0].supervisor?.peakMetric}`
        : null,
      timeToKillMs: stats(kills.map((i) => i.supervisor?.wallMs)),
    };
  };
  return {
    key: cell.key,
    budgetBytes: limits.budgetBytes,
    tscLimitCodes: (cell.reference?.codes ?? []).filter((c) => RESOURCE_CODES.includes(c)),
    verter: arm("verter"),
    tscApi: arm("tsc-api"),
    tscCli: arm("tsc-cli"),
    tscCliSingle: arm("tsc-cli-1"),
  };
}

/** Summarise a run against the measured reference (and its sessions against their constructed answers). */
export function summarize(run, expected, scenarios, sessions = allSessions()) {
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  const limits = runLimits(run.meta?.options ?? {});
  const budgetBytes = limits.budgetBytes;
  const resolution = timerResolution(run);
  const groups = new Map();
  for (const inv of run.invocations) {
    const key = `${inv.scenario}/${inv.setting}`;
    if (!groups.has(key)) groups.set(key, {});
    const arms = groups.get(key);
    (arms[inv.arm] ??= []).push(inv);
  }
  const cells = [];
  for (const key of Object.keys(run.meta.scenarios)) {
    const meta = run.meta.scenarios[key];
    const scenario = byId.get(meta.id);
    const measured = referenceFor(expected, meta.id, meta.setting);
    const armInvs = groups.get(key) ?? {};
    const base = {
      beyond: beyondDigest(scenario),
      probe: probeDigest(scenario),
      budgetBytes,
      limits,
    };
    const arms = {};
    // The tsc arm first: when the measurement holds no answer but the API
    // answered with the constructed one, that is the reference the Verter
    // arms are classified against; and a Verter answer past tsc's limit
    // counts as beyond tsc only where the tsc arm was killed on the demand.
    if (armInvs["tsc-api"])
      arms["tsc-api"] = probeArmSummary("tsc-api", armInvs["tsc-api"], {
        ...base,
        reference: measured,
      });
    const reference =
      arms["tsc-api"]?.class === "reference-by-construction"
        ? {
            digest: beyondDigest(scenario),
            errorAny: false,
            codes: measured?.codes ?? [],
            byConstruction: true,
            measuredGap: measured?.gap ?? null,
          }
        : measured;
    const ctx = { ...base, reference, tscKilled: arms["tsc-api"]?.class === "killed" };
    for (const [arm, invs] of Object.entries(armInvs)) {
      if (arm === "tsc-api") continue;
      arms[arm] =
        ARMS[arm].kind === "cli" ? cliArmSummary(invs, limits) : probeArmSummary(arm, invs, ctx);
    }
    const comparableTsc = ["reference", "reference-by-construction"].includes(
      arms["tsc-api"]?.class,
    );
    const headline =
      arms.verter?.class === "matched" && comparableTsc
        ? comparison(armInvs.verter, armInvs["tsc-api"], resolution)
        : null;
    const obsCost =
      armInvs.verter && armInvs["verter-obs"]
        ? {
            coldMs: verdict(
              armInvs.verter.filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              armInvs["verter-obs"].filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              resolution.single,
            ),
          }
        : null;
    const observeBuild =
      armInvs.verter && armInvs["verter-observe"]
        ? {
            coldMs: verdict(
              armInvs.verter.filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              armInvs["verter-observe"]
                .filter((i) => !i.warmup)
                .map((i) => probeMetrics(i)?.coldMs),
              resolution.single,
            ),
            requiredState: requiredStateComparison(
              arms.verter.retentions ?? [],
              arms["verter-observe"].retentions ?? [],
            ),
          }
        : null;
    cells.push({
      key,
      scenario: meta.id,
      setting: meta.setting,
      family: meta.family,
      note: meta.note,
      reference: reference
        ? {
            digest: reference.digest ?? null,
            errorAny: reference.errorAny ?? null,
            codes: reference.codes ?? [],
            gap: reference.gap ?? null,
            killed: reference.killed ?? null,
            byConstruction: reference.byConstruction ?? false,
            measuredGap: reference.measuredGap ?? null,
          }
        : null,
      arms,
      headline,
      obsCost,
      observeBuild,
    });
    cells.at(-1).capacity = capacityRow(cells.at(-1), armInvs, limits);
  }
  const counts = {};
  for (const cell of cells) {
    const c = cell.arms.verter?.class ?? "not-run";
    counts[c] = (counts[c] ?? 0) + 1;
  }
  const out = { cells, verterClassCounts: counts, resolution };
  if (run.meta?.sessions) out.sessions = summarizeSessions(run, sessions, resolution);
  return out;
}
