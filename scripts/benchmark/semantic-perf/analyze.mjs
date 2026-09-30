// Answer extraction, record checks, classification and statistics for the
// semantic benchmark. Pure functions over raw invocation records: the
// harness uses them to write its summary and the validator re-derives every
// summary field from the raw records with them, so a summary that disagrees
// with its own raw records fails validation.

import { createHash } from "node:crypto";

import { canonicalDigest } from "./canonical.mjs";
import { RESOURCE_CODES } from "./reference.mjs";

export { RESOURCE_CODES };

/** The arms, what each measures and whether it enters the head-to-head. */
export const ARMS = {
  verter: { tool: "verter", kind: "probe", headline: true, label: "Verter (production defaults)" },
  "tsc-api": { tool: "tsc", kind: "probe", headline: true, label: "tsc 7.0.2 native API" },
  "verter-obs": {
    tool: "verter",
    kind: "probe",
    headline: false,
    label: "Verter, observability on (labelled; not compared)",
  },
  "verter-counted": {
    tool: "verter",
    kind: "probe",
    headline: false,
    label: "Verter, counting allocator (instrumented; not compared)",
  },
  "tsc-cli": {
    tool: "tsc",
    kind: "cli",
    headline: false,
    label: "tsc -p, default (parallel) checkers (whole program; reference)",
  },
  "tsc-cli-1": {
    tool: "tsc",
    kind: "cli",
    headline: false,
    label: "tsc -p --singleThreaded (whole program; reference)",
  },
};

export const DEFAULT_ARMS = Object.keys(ARMS);

/** Verter classes. */
export const CLASSES = [
  "killed",
  "error",
  "refusal",
  "unverified",
  "partial",
  "mismatch",
  "no-reference",
  "beyond-tsc",
  "matched",
];

/** A finite, non-negative number. */
const finite = (x) => typeof x === "number" && Number.isFinite(x) && x >= 0;

/**
 * The phases in which each tool's ENGINE is working on the demand: from its
 * own start (Verter's host construction; tsc's server spawn) to the last warm
 * request. Statistics, observation, calibration and teardown are the
 * harness's work, never the engine's.
 */
export const ENGINE_PHASES = {
  verter: ["engine-start", "setup", "init", "cold", "warm"],
  tsc: ["spawn", "engine-start", "setup", "init", "cold", "warm"],
};

/** The limits a run holds every invocation to, from its options. */
export function runLimits(options = {}) {
  return {
    budgetBytes: (options.memMb ?? 0) * 1024 * 1024,
    timeoutMs: options.timeoutMs ?? null,
    startupAllowanceMs: options.startupAllowanceMs ?? 0,
  };
}

/** The supervisor's deadline for one invocation: the engine's deadline plus the startup allowance. */
export function supervisorDeadlineMs(options = {}) {
  return (options.timeoutMs ?? 0) + (options.startupAllowanceMs ?? 0);
}

/**
 * How long, at least, the engine had worked on the demand when the
 * supervisor killed the invocation: from the start of its first engine phase
 * (the marker's wall-clock history) to the kill. The kill came no earlier
 * than the supervisor's spawn plus the child's wall time, so this is a lower
 * bound. Null without the evidence.
 */
export function engineWorkMs(inv) {
  const tool = ARMS[inv.arm]?.tool;
  const first = (inv.phaseHistory ?? []).find((h) => ENGINE_PHASES[tool]?.includes(h.phase));
  const wall = inv.supervisor?.wallMs;
  if (!first || !finite(first.atMs) || !finite(inv.spawnedAtMs) || !finite(wall)) return null;
  return inv.spawnedAtMs + wall - first.atMs;
}

/**
 * Whether a kill of an invocation is established as its ENGINE exhausting
 * the resource. `limits` = runLimits(options).
 *
 * A whole-program arm (`tsc -p`) is one process that is the engine: its
 * deadline and its memory are its own. A probe arm must have been in an
 * engine phase (the marker) when it was killed, and:
 *
 * - memory: only a tree that is the engine alone proves it (a Verter probe;
 *   a tsc API tree also holds the node driver, whose memory during a request
 *   is not bounded, so a memory kill there is never attributed), and only
 *   when the supervisor's actual kill threshold (`killTriggerBytes`: below
 *   the cap on a sampled backend) is at least the engine budget, in an
 *   accounting of the engine's own memory (a Linux cgroup also counts page
 *   cache and kernel memory, so it proves nothing);
 * - deadline: the engine itself had worked for at least the deadline
 *   (engineWorkMs); the supervisor's deadline adds a startup allowance for
 *   the process start before the engine's first phase.
 */
export function killAttributed(inv, limits = {}) {
  const rec = inv.supervisor ?? {};
  const arm = ARMS[inv.arm];
  const memoryAttributable = (singleProcess) => {
    if (!singleProcess) return false;
    if (String(rec.backend ?? "").startsWith("linux")) return false;
    const trigger =
      typeof rec.killTriggerBytes === "number" ? rec.killTriggerBytes : rec.memLimitBytes;
    return (
      typeof trigger === "number" &&
      typeof limits.budgetBytes === "number" &&
      trigger >= limits.budgetBytes
    );
  };
  if (arm?.kind === "cli") {
    if (rec.killedBy === "timeout")
      return finite(rec.wallMs) && finite(limits.timeoutMs) && rec.wallMs >= limits.timeoutMs;
    return rec.killedBy === "memory" && memoryAttributable(true);
  }
  if (!ENGINE_PHASES[arm?.tool]?.includes(inv.phase)) return false;
  if (rec.killedBy === "timeout") {
    const work = engineWorkMs(inv);
    return work !== null && finite(limits.timeoutMs) && work >= limits.timeoutMs;
  }
  return rec.killedBy === "memory" && memoryAttributable(arm.tool === "verter");
}

/**
 * How an invocation ended. `limits` = runLimits(options) (needed to
 * attribute a kill).
 */
export function invocationEnd(inv, limits = {}) {
  if (inv.skipped)
    return {
      kind: "killed",
      detail: "memory (skipped after a warmup whose engine exhausted the cap)",
    };
  const rec = inv.supervisor;
  if (!rec)
    return { kind: "harness-failure", detail: inv.supervisorReadError ?? "no supervisor record" };
  if (!rec.launched)
    return { kind: "harness-failure", detail: `not launched: ${(rec.errors ?? []).join("; ")}` };
  // A supervisor error (containment lost, a tree that would not empty, …)
  // invalidates the invocation whatever else happened.
  if ((rec.errors ?? []).length) return { kind: "harness-failure", detail: rec.errors.join("; ") };
  if (rec.killedBy === "memory" || rec.killedBy === "timeout") {
    const phase = inv.phase ?? null;
    const detail = phase ? `${rec.killedBy} during ${phase}` : rec.killedBy;
    // A record exists once the demand was measured: whatever was killed
    // after that (observation, teardown) was not the engine's demand.
    if (inv.probe?.stage === "measured" || inv.probe?.stage === "complete") {
      return { kind: "observe-killed", detail: `${detail}, after the demand completed` };
    }
    if (!killAttributed(inv, limits))
      return { kind: "unattributed-kill", detail: `${detail}, not attributable to the engine` };
    return { kind: "killed", detail };
  }
  if (rec.killedBy) return { kind: "harness-failure", detail: `killed by ${rec.killedBy}` };
  return { kind: "exited", exitCode: rec.exitCode };
}

/**
 * Problems with a probe record that must hold for its numbers to be read:
 * every phase and request time present, finite and non-negative, every warm
 * repeat answered with the cold answer, the statistics present and clean.
 * `stage` is the stage the record must have reached.
 */
export function probeRecordProblems(record, { tool, warmRepeats, stage = "complete" }) {
  const p = [];
  const need = (cond, what) => {
    if (!cond) p.push(what);
  };
  need(record?.schema === 2, `record schema ${record?.schema} is not 2`);
  need(record?.tool === tool, `record from ${record?.tool}, not ${tool}`);
  if (stage === "complete")
    need(record?.stage === "complete", `record stage ${record?.stage}, not complete`);
  else
    need(
      record?.stage === "measured" || record?.stage === "complete",
      `record stage ${record?.stage}`,
    );
  const probe = record?.probes?.[0];
  need(
    record?.probes?.length === 1 && probe?.alias === "__Probe",
    `expected exactly one __Probe record, got ${record?.probes?.length ?? 0}`,
  );
  if (!probe) return p;
  const warm = probe.warm ?? [];
  need(warm.length === warmRepeats, `${warm.length} warm repeats, not ${warmRepeats}`);
  if (tool === "verter") {
    const ph = record.phases ?? {};
    need(
      finite(ph.engineStart) && finite(ph.setup) && finite(ph.init),
      "a phase time is missing or invalid",
    );
    need(
      record.init?.outcome?.kind === "value" && finite(record.init?.micros),
      "the init request did not answer",
    );
    need(finite(probe.cold?.micros), "the cold time is missing or invalid");
    need(
      warm.every((w) => finite(w.micros)),
      "a warm time is missing or invalid",
    );
    need(
      finite(record.afterRequests?.peakBytes) && finite(record.afterRequests?.currentBytes),
      "no engine statistics after the requests",
    );
    need(
      ph.init === record.init?.micros,
      "the init time disagrees with the init request's own time",
    );
    need(
      Number.isInteger(record.pid) && record.afterRequests?.pid === record.pid,
      "the engine statistics are not of the probe's own process",
    );
    statsInvariants(record.afterRequests, record.afterObserve, record.stage, need);
    if (record.stage === "complete") {
      need(finite(ph.teardown), "the teardown time is missing");
      need(finite(probe.observeMicros), "the observe time is missing");
      need(
        record.afterObserve?.pid === record.pid,
        "the statistics after observation are not of the probe's own process",
      );
    }
  } else {
    const ph = record.phases ?? {};
    need(
      finite(ph.spawnMs) &&
        finite(ph.engineStartMs) &&
        finite(ph.setupMs) &&
        finite(ph.setupRoundTripMs),
      "a phase time is missing or invalid",
    );
    need(
      record.init?.outcome?.kind === "value" &&
        finite(record.init?.serverMs) &&
        finite(record.init?.roundTripMs),
      "the init request did not answer",
    );
    need(
      finite(probe.cold?.serverMs) && finite(probe.cold?.roundTripMs),
      "the cold time is missing or invalid",
    );
    need(
      warm.every((w) => finite(w.serverMs) && finite(w.roundTripMs)),
      "a warm time is missing or invalid",
    );
    need(
      finite(record.serverAfterRequests?.peakBytes) &&
        finite(record.serverAfterRequests?.currentBytes),
      "no engine statistics after the requests",
    );
    need(finite(ph.engineStartRoundTripMs), "the engine-start round trip is missing");
    need(
      ph.initMs === record.init?.serverMs,
      "the init time disagrees with the init request's own time",
    );
    need(
      Number.isInteger(record.serverPid) && record.serverAfterRequests?.pid === record.serverPid,
      "the engine statistics are not of the tsc server process",
    );
    statsInvariants(record.serverAfterRequests, record.serverAfterObserve, record.stage, need);
    if (record.stage === "complete") {
      need(finite(ph.teardownMs), "the teardown time is missing");
      need(finite(probe.observeMs), "the observe time is missing");
      need(
        record.serverAfterObserve?.pid === record.serverPid,
        "the statistics after observation are not of the tsc server process",
      );
      const cal = record.calibration?.serverMs;
      need(
        Array.isArray(cal) && cal.length === 20 && cal.every(finite),
        "the timer calibration series is missing or invalid",
      );
    }
  }
  need(
    (record.statsErrors ?? []).length === 0,
    `statistics errors: ${(record.statsErrors ?? []).join("; ")}`,
  );
  if (record.stage === "complete" && probe.cold?.outcome?.kind === "value") {
    need(
      observationEvidence(tool, probe.observation),
      "the observation lacks the evidence its tool must supply",
    );
  }
  if (record.stage === "complete" && probe.cold?.outcome?.kind === "value") {
    need(
      warm.every((w) => w.outcome?.kind === "value"),
      "a warm repeat did not answer",
    );
    need(
      warm.every((w) => w.sameAnswerAsCold === true),
      "a warm repeat answered differently from the cold request",
    );
  }
  return p;
}

/** A statistics reading: a named metric, finite non-negative figures, current within peak. */
function readingValid(reading) {
  return (
    typeof reading?.metric === "string" &&
    reading.metric.length > 0 &&
    finite(reading.peakBytes) &&
    reading.peakBytes > 0 &&
    finite(reading.currentBytes) &&
    reading.currentBytes <= reading.peakBytes
  );
}

/** Memory-reading invariants: both readings valid, one metric, peaks non-decreasing. */
function statsInvariants(afterRequests, afterObserve, stage, need) {
  need(
    readingValid(afterRequests),
    "the engine statistics are not a valid reading (a metric, finite figures, current within peak)",
  );
  if (stage === "complete") {
    need(readingValid(afterObserve), "the statistics after observation are not a valid reading");
    need(
      afterObserve?.metric === afterRequests?.metric,
      "the statistics after observation use another metric",
    );
    need(
      afterObserve?.peakBytes >= afterRequests?.peakBytes,
      "the peak after observation is below the peak before it",
    );
  }
}

/** Whether an observation carries the evidence its tool must supply. */
export function observationEvidence(tool, obs) {
  if (!obs || typeof obs !== "object") return false;
  // Two variants: a FAILED observation (an error, never a comparable answer,
  // whatever text it carries) and a SUCCESSFUL one, which must carry its
  // answer and every piece of evidence its tool supplies.
  if (obs.error !== undefined && obs.error !== null) return typeof obs.error === "string";
  const textual = typeof obs.text === "string" || obs.textElided === true;
  if (!textual) return false;
  if (tool === "tsc") return typeof obs.errorType === "boolean";
  return (
    Number.isInteger(obs.unknownLeaves) &&
    obs.unknownLeaves >= 0 &&
    Array.isArray(obs.unknownSamples) &&
    Number.isInteger(obs.conditionalNodes) &&
    (typeof obs.shape === "string" || obs.shape === null)
  );
}

/** The observed answer of one probe invocation (arm kind "probe"). */
export function probeAnswer(inv, limits = {}) {
  let end = invocationEnd(inv, limits);
  if (end.kind === "exited" && end.exitCode !== 0)
    end = { kind: "child-failure", detail: `exit ${end.exitCode}` };
  if (end.kind !== "exited" && end.kind !== "observe-killed") return { end };
  const result = inv.probe;
  if (!result)
    return { end: { kind: "child-failure", detail: inv.probeReadError ?? "no probe record" } };
  const probe = result.probes?.[0];
  if (!probe || result.probes.length !== 1) {
    return {
      end: {
        kind: "child-failure",
        detail: `expected exactly one probe record, got ${result.probes?.length ?? 0}`,
      },
    };
  }
  const obs = probe.observation ?? {};
  let digest = null;
  let canonicalError = null;
  // A failed observation supplies no answer, whatever text it carries.
  if (typeof obs.error === "string") {
    // no digest
  } else if (obs.textElided) {
    digest = obs.canonical ?? null;
    canonicalError = obs.canonicalError ?? null;
  } else if (typeof obs.text === "string") {
    try {
      digest = canonicalDigest(obs.text);
    } catch (err) {
      canonicalError = String(err.message ?? err);
    }
  }
  const stats = result.tool === "verter" ? result.afterRequests : result.serverAfterRequests;
  return {
    end,
    alias: probe.alias,
    outcome: probe.cold?.outcome ?? { kind: "missing" },
    warmKinds: (probe.warm ?? []).map((w) => w.outcome?.kind),
    warmSame: (probe.warm ?? []).map((w) => w.sameAnswerAsCold),
    observed: Boolean(probe.observation),
    digest,
    canonicalError,
    observeError: probe.observation ? (obs.error ?? null) : "the answer was not observed",
    evidence: observationEvidence(result.tool, probe.observation),
    errorType: obs.errorType ?? null,
    unknownLeaves: obs.unknownLeaves ?? 0,
    unknownSamples: obs.unknownSamples ?? [],
    shape: obs.shape ?? null,
    enginePeakBytes: stats?.peakBytes ?? null,
  };
}

function isBudgetFault(detail) {
  return /budget|limit|exhaust|depth/i.test(detail ?? "");
}

/** Whether a reference holds tsc's answer. */
export function referenceHasAnswer(reference) {
  return Boolean(reference && !reference.gap && reference.digest);
}

const overBudget = (answer, budgetBytes) =>
  typeof budgetBytes === "number" &&
  typeof answer.enginePeakBytes === "number" &&
  answer.enginePeakBytes > budgetBytes;

const mib = (bytes) => `${(bytes / 1048576).toFixed(0)} MiB`;

/**
 * Classify one Verter answer. `ctx`: `reference` (the interpreted tsc
 * answer), `beyond` / `probe` (digests of the scenario's constructed answer
 * and of its probe expression), `budgetBytes` (the engine budget),
 * `tscKilled` (the tsc arm was killed on this demand).
 */
export function classifyVerterAnswer(answer, ctx = {}) {
  const {
    reference = null,
    beyond = null,
    probe = null,
    budgetBytes = null,
    tscKilled = false,
  } = ctx;
  const end = answer.end;
  if (end.kind === "killed") return { class: "killed", detail: end.detail };
  if (end.kind === "unattributed-kill") return { class: "unverified", detail: end.detail };
  if (end.kind !== "exited" && end.kind !== "observe-killed")
    return { class: "error", detail: end.detail };
  if (answer.outcome.kind === "fault") {
    return isBudgetFault(answer.outcome.detail)
      ? { class: "refusal", detail: answer.outcome.detail }
      : { class: "error", detail: answer.outcome.detail };
  }
  if (answer.outcome.kind !== "value")
    return { class: "error", detail: `request outcome ${answer.outcome.kind}` };
  if (answer.warmKinds.some((k) => k !== "value"))
    return { class: "error", detail: "a warm repeat did not answer" };
  if (overBudget(answer, budgetBytes)) {
    return {
      class: "killed",
      detail: `the engine's peak ${mib(answer.enginePeakBytes)} exceeds the ${mib(budgetBytes)} budget`,
    };
  }
  if (end.kind === "observe-killed")
    return { class: "unverified", detail: `stopped while its answer was observed (${end.detail})` };
  if (answer.warmSame.some((s) => s !== true))
    return { class: "error", detail: "a warm repeat answered differently" };
  if (!answer.evidence)
    return { class: "unverified", detail: "the observation lacks its completeness evidence" };
  if (answer.observeError) return { class: "partial", detail: answer.observeError };
  if (answer.unknownLeaves > 0) {
    return {
      class: "partial",
      detail: `the answer holds ${answer.unknownLeaves} unmaterialised leaf/leaves (${answer.unknownSamples.join(", ")})`,
    };
  }
  if (answer.shape === "conditional")
    return { class: "partial", detail: "the answer is an unevaluated conditional" };
  if (!answer.digest)
    return { class: "partial", detail: answer.canonicalError ?? "no printable answer" };
  if (!referenceHasAnswer(reference)) {
    // Beyond tsc only where tsc's exhaustion is established on the demand
    // itself: the measuring program ran out and so did the API arm.
    if (reference?.killed && tscKilled && beyond && answer.digest.sha256 === beyond.sha256) {
      return {
        class: "beyond-tsc",
        detail: "tsc exhausts the cap on this demand; Verter's answer is the constructed one",
      };
    }
    return { class: "no-reference", detail: reference?.gap ?? "no measured reference" };
  }
  const limitCodes = reference.codes.filter((c) => RESOURCE_CODES.includes(c));
  if (!reference.errorAny && answer.digest.sha256 === reference.digest.sha256) {
    return {
      class: "matched",
      detail: limitCodes.length ? "equal to tsc's answer at its resource limit" : "",
    };
  }
  if (limitCodes.length && beyond && answer.digest.sha256 === beyond.sha256) {
    return { class: "beyond-tsc", detail: `tsc stops with TS${limitCodes.join("/TS")}` };
  }
  if (probe && answer.digest.sha256 === probe.sha256)
    return { class: "partial", detail: "the answer is the probe expression, unevaluated" };
  return {
    class: "mismatch",
    detail: `answered ${answer.digest.preview}, tsc ${reference.errorAny ? "error-any" : reference.digest.preview}`,
  };
}

/**
 * The tsc arm's standing for one invocation:
 *   "killed"          the supervisor killed it during the demand, or its
 *                     engine exceeded the budget (a valid observation);
 *   "unverified"      stopped while observing, or its answer cannot be
 *                     checked (no reference and no constructed answer);
 *   "reference"       it reproduces the measured reference;
 *   "by-construction" the measurement has no answer, but the API's answer is
 *                     the scenario's constructed one;
 *   "problem"         it contradicts the reference or failed (fails validation).
 */
export function tscAnswerStatus(answer, reference, beyond, budgetBytes = null) {
  const end = answer.end;
  if (end.kind === "killed") return { status: "killed", detail: end.detail };
  if (end.kind === "unattributed-kill") return { status: "unverified", detail: end.detail };
  if (end.kind !== "exited" && end.kind !== "observe-killed")
    return { status: "problem", problem: `tsc invocation failed: ${end.detail}` };
  if (answer.outcome.kind !== "value")
    return {
      status: "problem",
      problem: `tsc request outcome ${answer.outcome.kind} ${answer.outcome.detail ?? ""}`,
    };
  if (answer.warmKinds.some((k) => k !== "value"))
    return { status: "problem", problem: "a warm tsc request did not answer" };
  if (overBudget(answer, budgetBytes)) {
    return {
      status: "killed",
      detail: `the engine's peak ${mib(answer.enginePeakBytes)} exceeds the ${mib(budgetBytes)} budget`,
    };
  }
  if (end.kind === "observe-killed")
    return {
      status: "unverified",
      detail: `stopped while its answer was observed (${end.detail})`,
    };
  if (answer.warmSame.some((s) => s !== true))
    return { status: "problem", problem: "a warm tsc request answered differently" };
  if (!answer.evidence)
    return { status: "problem", problem: "the tsc observation lacks its error-type evidence" };
  if (answer.observeError)
    return {
      status: "problem",
      problem: `the tsc answer could not be observed: ${answer.observeError}`,
    };
  if (!reference)
    return { status: "problem", problem: "no measured reference for this scenario and setting" };
  if (!answer.digest)
    return {
      status: "problem",
      problem: `tsc printed no answer: ${answer.observeError ?? answer.canonicalError}`,
    };
  if (!referenceHasAnswer(reference)) {
    if (!answer.errorType && beyond && answer.digest.sha256 === beyond.sha256)
      return { status: "by-construction" };
    return { status: "unverified", detail: reference.gap };
  }
  if (answer.digest.sha256 !== reference.digest.sha256) {
    return {
      status: "problem",
      problem: `tsc answered ${answer.digest.preview}; the measured reference is ${reference.digest.preview}`,
    };
  }
  if (Boolean(answer.errorType) !== Boolean(reference.errorAny)) {
    return {
      status: "problem",
      problem: `tsc's error-type flag is ${answer.errorType}; the reference says errorAny ${reference.errorAny}`,
    };
  }
  return { status: "reference" };
}

export function stats(values) {
  const xs = values
    .filter((v) => typeof v === "number" && Number.isFinite(v))
    .sort((a, b) => a - b);
  if (!xs.length) return null;
  const median =
    xs.length % 2 ? xs[(xs.length - 1) / 2] : (xs[xs.length / 2 - 1] + xs[xs.length / 2]) / 2;
  return { n: xs.length, min: xs[0], median, max: xs.at(-1) };
}

/**
 * A tsc request's time: the server's own processing time, as it reports it.
 * The server's clock is coarse on Windows (it can report 0, or more than the
 * round trip took); the verdict's resolution, derived from the run's own
 * observed quantum, absorbs that rather than clipping one side.
 */
export const tscRequestMs = (request) => (finite(request?.serverMs) ? request.serverMs : null);

/** Per-invocation timing and memory of a probe arm, in milliseconds and bytes. */
export function probeMetrics(inv) {
  const r = inv.probe;
  const p = r?.probes?.[0];
  if (!p) return null;
  const sum = (...xs) =>
    xs.every((x) => typeof x === "number") ? xs.reduce((a, b) => a + b, 0) : null;
  if (r.tool === "verter") {
    const ms = (us) => (typeof us === "number" ? us / 1000 : null);
    const setup = ms(r.phases?.setup);
    const init = ms(r.init?.micros);
    const cold = ms(p.cold?.micros);
    return {
      engineStartMs: ms(r.phases?.engineStart),
      setupMs: setup,
      initMs: init,
      coldMs: cold,
      firstTypeMs: sum(setup, init, cold),
      warmMs: stats((p.warm ?? []).map((w) => ms(w.micros)))?.median ?? null,
      observeMs: ms(p.observeMicros),
      teardownMs: ms(r.phases?.teardown),
      peakBytes: r.afterRequests?.peakBytes ?? null,
      retainedBytes: r.afterRequests?.currentBytes ?? null,
      observePeakBytes: r.afterObserve?.peakBytes ?? null,
      cpuMs:
        typeof r.afterRequests?.cpuMicros === "number" ? r.afterRequests.cpuMicros / 1000 : null,
      memoryMetric: r.afterRequests?.metric ?? null,
      allocations: p.coldAllocations ?? null,
      retention: r.retention ?? null,
    };
  }
  const setup = finite(r.phases?.setupMs) ? r.phases.setupMs : null;
  const init = tscRequestMs(r.init);
  const cold = tscRequestMs(p.cold);
  const engineStart = finite(r.phases?.engineStartMs) ? r.phases.engineStartMs : null;
  return {
    spawnMs: r.phases?.spawnMs ?? null,
    engineStartMs: engineStart,
    setupMs: setup,
    setupRoundTripMs: r.phases?.setupRoundTripMs ?? null,
    initMs: init,
    coldMs: cold,
    coldRoundTripMs: p.cold?.roundTripMs ?? null,
    firstTypeMs: sum(setup, init, cold),
    warmMs: stats((p.warm ?? []).map(tscRequestMs))?.median ?? null,
    warmRoundTripMs: stats((p.warm ?? []).map((w) => w.roundTripMs))?.median ?? null,
    observeMs: p.observeMs ?? null,
    teardownMs: r.phases?.teardownMs ?? null,
    peakBytes: r.serverAfterRequests?.peakBytes ?? null,
    retainedBytes: r.serverAfterRequests?.currentBytes ?? null,
    observePeakBytes: r.serverAfterObserve?.peakBytes ?? null,
    cpuMs:
      typeof r.serverAfterRequests?.cpuMicros === "number"
        ? r.serverAfterRequests.cpuMicros / 1000
        : null,
    memoryMetric: r.serverAfterRequests?.metric ?? null,
  };
}

/** Parse a `tsc -p --extendedDiagnostics` stdout. */
export function parseCli(stdout) {
  const num = (label) => {
    const m = new RegExp(`^${label}:\\s+([\\d,.]+)(K|s)?\\s*$`, "m").exec(stdout);
    if (!m) return null;
    const value = Number(m[1].replace(/,/g, ""));
    return m[2] === "K" ? value * 1024 : m[2] === "s" ? value * 1000 : value;
  };
  return {
    codes: [...new Set([...stdout.matchAll(/error TS(\d+)/g)].map((m) => Number(m[1])))].sort(
      (a, b) => a - b,
    ),
    memoryUsedBytes: num("Memory used"),
    checkMs: num("Check time"),
    totalMs: num("Total time"),
    parseMs: num("Parse time"),
    bindMs: num("Bind time"),
    complete: /^Total time:/m.test(stdout),
  };
}

/**
 * The verdict of one metric between the two headline arms — a descriptive
 * rule, not a statistical test: a winner only when every measured repetition
 * of one arm beats every repetition of the other by more than `resolution`
 * (the metric's timer resolution); otherwise "overlap".
 */
export function verdict(verterValues, tscValues, resolution = 0) {
  const v = stats(verterValues);
  const t = stats(tscValues);
  if (!v || !t) return { verdict: "n/a", ratio: null };
  const ratio = v.median > 0 ? t.median / v.median : null;
  // Below the timer's resolution a median is not a measurement to divide.
  const ratioMeaningful = Math.min(v.median, t.median) >= resolution;
  if (v.max + resolution < t.min) return { verdict: "verter", ratio, ratioMeaningful };
  if (t.max + resolution < v.min) return { verdict: "tsc", ratio, ratioMeaningful };
  return { verdict: "overlap", ratio, ratioMeaningful };
}

/** Printed answers longer than this are embedded in results.json as their digest. */
export const ELIDE_TEXT_ABOVE = 4096;

/**
 * The copy of a probe record embedded in results.json: identical except
 * that an answer text longer than ELIDE_TEXT_ABOVE is replaced by its
 * canonical digest and the raw text's sha256. The raw record stays on disk;
 * the validator recomputes this compaction from it and compares.
 */
export function compactProbeRecord(record) {
  if (!record || !Array.isArray(record.probes)) return record;
  return {
    ...record,
    probes: record.probes.map((probe) => {
      const obs = probe.observation;
      if (!obs || typeof obs.text !== "string" || obs.text.length <= ELIDE_TEXT_ABOVE) return probe;
      let canonical = null;
      let canonicalError = null;
      try {
        canonical = canonicalDigest(obs.text);
      } catch (err) {
        canonicalError = String(err.message ?? err);
      }
      return {
        ...probe,
        observation: {
          ...obs,
          text: null,
          textElided: true,
          textLength: obs.text.length,
          textSha256: createHash("sha256").update(obs.text).digest("hex"),
          canonical,
          canonicalError,
        },
      };
    }),
  };
}
