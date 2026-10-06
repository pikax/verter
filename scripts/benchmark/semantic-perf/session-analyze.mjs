// Summary and validation of the session workloads (sessions.mjs), derived
// only from raw session records and the catalog's constructed answers. The
// validator recomputes the summary and fails a run whose stored session
// summary differs.
//
// Every demand is classified exactly as a probe's answer is
// (classifyVerterAnswer / tscAnswerStatus), against the constructed answer
// of that point of the script: tsc's must equal it (else validation fails),
// Verter's is a classified finding. A `meta` demand (Vue component props and
// events) is Verter-only: matched when the published names and required
// flags equal the construction's, never compared with tsc.

import {
  ARMS,
  CLASSES,
  classifyVerterAnswer,
  invocationEnd,
  observationEvidence,
  runLimits,
  stats,
  supervisorDeadlineMs,
  tscAnswerStatus,
  verdict,
} from "./analyze.mjs";
import { canonicalDigest } from "./canonical.mjs";
import { sha256Text } from "./provenance.mjs";
import { INCREMENTAL_FACILITY, sessionTsconfigText, tscFiles } from "./sessions.mjs";
import { supervisorRecordProblems } from "./supervisor.mjs";

/** The arms a session runs (a session is one live process: no cold-only or whole-program arm). */
export const SESSION_ARMS = ["verter", "verter-observe", "tsc-api"];
export const sessionArmsOf = (arms) => arms.filter((a) => SESSION_ARMS.includes(a));

/** The required-state retention counters both Verter builds must agree on (occupancy and charges, no histories or peaks). */
export const REQUIRED_RETENTION = [
  "semanticNodes",
  "semanticMemoEntries",
  "relationProofs",
  "relateKeys",
  "unionViews",
  "shapeCacheEntries",
  "retainedBytes",
  "pinnedBytes",
];

const finite = (x) => typeof x === "number" && Number.isFinite(x) && x >= 0;
const stable = (v) => JSON.stringify(v);

/** The digests of a session's inputs, as the run records them. */
export function sessionInputs(session, libText) {
  return {
    "lib.bench.d.ts": sha256Text(libText),
    "tsconfig.json": sha256Text(sessionTsconfigText(session)),
    ...Object.fromEntries(Object.entries(session.files).map(([f, t]) => [f, sha256Text(t)])),
  };
}

/**
 * The fingerprint of the REQUIRED state a Verter record retains, or null.
 * Two builds that differ only in optional capture must agree on it.
 */
export function requiredStateFingerprint(retention) {
  if (!retention) return null;
  return stable(REQUIRED_RETENTION.map((k) => retention[k] ?? null));
}

/**
 * Compare the REQUIRED state of the production arm and the observe build
 * over their completed measured invocations: "identical" when both arms
 * retain the same set of states, "differs" when they do not, and
 * "nondeterministic" when the production arm disagrees with itself (so no
 * difference can be attributed to the build).
 */
export function requiredStateComparison(production, observe) {
  const prints = (rs) => [...new Set(rs.map(requiredStateFingerprint).filter(Boolean))].sort();
  const p = prints(production);
  const o = prints(observe);
  if (!p.length || !o.length) return { state: "n/a" };
  if (p.length > 1) return { state: "nondeterministic", production: p.length };
  if (stable(p) === stable(o)) return { state: "identical" };
  const a = JSON.parse(p[0]);
  const fields = REQUIRED_RETENTION.filter((_, i) => o.some((x) => JSON.parse(x)[i] !== a[i]));
  return { state: "differs", fields };
}

function digestOrError(text) {
  try {
    return { digest: canonicalDigest(text), canonicalError: null };
  } catch (err) {
    return { digest: null, canonicalError: String(err.message ?? err) };
  }
}

/** One demand's answer, in the shape the probe classifiers read. */
function demandAnswer(end, request, tool, peakBytes) {
  const obs = request?.observation;
  const failed = typeof obs?.error === "string";
  const { digest, canonicalError } =
    !failed && typeof obs?.text === "string"
      ? digestOrError(obs.text)
      : { digest: null, canonicalError: null };
  return {
    end,
    outcome: request?.outcome ?? { kind: "missing" },
    warmKinds: [],
    warmSame: [],
    observed: Boolean(obs),
    digest,
    canonicalError,
    observeError: obs ? (obs.error ?? null) : "the answer was not observed",
    evidence: observationEvidence(tool, obs),
    errorType: obs?.errorType ?? null,
    unknownLeaves: obs?.unknownLeaves ?? 0,
    unknownSamples: obs?.unknownSamples ?? [],
    shape: obs?.shape ?? null,
    enginePeakBytes: peakBytes,
  };
}

/** Classify a Verter `meta` step against the constructed surface. */
export function classifyMeta(step, expect) {
  if (!step) return { class: "error", detail: "no meta record" };
  if (step.outcome?.kind === "fault") return { class: "error", detail: step.outcome.detail };
  if (step.outcome?.kind !== "value" || !step.surface)
    return { class: "error", detail: `meta outcome ${step.outcome?.kind}` };
  const norm = (s) => ({
    props: [...s.props].map((p) => `${p.name}${p.required ? "" : "?"}`).sort(),
    events: [...s.events].sort(),
  });
  const got = norm(step.surface);
  const want = norm(expect);
  if (stable(got) === stable(want)) return { class: "matched", detail: "" };
  return {
    class: "mismatch",
    detail: `props ${got.props.join(", ")} / events ${got.events.join(", ")}; constructed props ${want.props.join(", ")} / events ${want.events.join(", ")}`,
  };
}

/** A step's engine time in milliseconds (null when the arm has none). */
export function stepMs(tool, step) {
  if (!step) return null;
  if (tool === "verter") {
    const us = step.kind === "demand" ? step.wallMicros : step.micros;
    return finite(us) ? us / 1000 : null;
  }
  if (step.kind === "demand") return finite(step.wallMs) ? step.wallMs : null;
  if (step.kind === "edit") return finite(step.serverMs) ? step.serverMs : null;
  return null;
}

const peakOf = (record) =>
  (record?.tool === "verter" ? record?.afterSteps : record?.serverAfterSteps)?.peakBytes ?? null;

/** The answers of one invocation: per demand and per meta step. */
export function sessionAnswers(inv, session, limits) {
  let end = invocationEnd(inv, limits);
  if (end.kind === "exited" && end.exitCode !== 0)
    end = { kind: "child-failure", detail: `exit ${end.exitCode}` };
  const record = inv.session;
  if (end.kind === "exited" && !record)
    end = { kind: "child-failure", detail: inv.sessionReadError ?? "no session record" };
  const tool = ARMS[inv.arm].tool;
  const out = [];
  session.steps.forEach((step, s) => {
    const got = end.kind === "exited" ? record.steps?.[s] : null;
    if (step.kind === "demand")
      step.requests.forEach((req, r) =>
        out.push({
          step: s,
          request: r,
          kind: "demand",
          file: req.file,
          alias: req.alias,
          expect: req.expect,
          answer: demandAnswer(end, got?.requests?.[r], tool, peakOf(record)),
        }),
      );
    else if (step.kind === "meta" && tool === "verter")
      out.push({ step: s, kind: "meta", file: step.file, expect: step.expect, got, end });
  });
  return out;
}

function classifyEntry(entry, tool, budgetBytes) {
  if (entry.kind === "meta") {
    const end = entry.end;
    if (end.kind === "killed") return { class: "killed", detail: end.detail };
    if (end.kind !== "exited")
      return {
        class: end.kind === "unattributed-kill" ? "unverified" : "error",
        detail: end.detail,
      };
    return classifyMeta(entry.got, entry.expect);
  }
  const reference = { digest: canonicalDigest(entry.expect), errorAny: false, codes: [] };
  if (tool === "verter") return classifyVerterAnswer(entry.answer, { reference, budgetBytes });
  return tscAnswerStatus(entry.answer, reference, null, budgetBytes);
}

function sessionArmSummary(arm, invs, session, limits) {
  const tool = ARMS[arm].tool;
  const measured = invs.filter((i) => !i.warmup);
  const perInv = invs.map((inv) =>
    sessionAnswers(inv, session, limits).map((e) => ({
      ...e,
      verdict: classifyEntry(e, tool, limits.budgetBytes),
    })),
  );
  const demands = (perInv[0] ?? []).map((entry, k) => {
    const verdicts = perInv.map((list) => list[k].verdict);
    const name = entry.kind === "meta" ? `${entry.file} (meta)` : `${entry.file}#${entry.alias}`;
    if (tool === "verter") {
      const classes = [...new Set(verdicts.map((v) => v.class))];
      const worst = CLASSES.find((c) => classes.includes(c)) ?? classes[0];
      return {
        step: entry.step,
        demand: name,
        class: worst,
        repetitionsDiffer: classes.length > 1,
        detail: verdicts.find((v) => v.class === worst)?.detail ?? "",
      };
    }
    const problems = [...new Set(verdicts.filter((v) => v.problem).map((v) => v.problem))];
    const kinds = [...new Set(verdicts.map((v) => v.status))];
    return {
      step: entry.step,
      demand: name,
      class: problems.length
        ? "inconsistent-with-reference"
        : kinds.length === 1
          ? kinds[0]
          : "inconsistent",
      problems,
      detail: verdicts.find((v) => v.detail)?.detail ?? "",
    };
  });
  const completed = measured.filter((i) => invocationEnd(i, limits).kind === "exited" && i.session);
  const records = completed.map((i) => i.session);
  const out = {
    invocations: invs.length,
    measured: measured.length,
    completedMeasured: completed.length,
    ends: [...new Set(invs.map((i) => invocationEnd(i, limits).kind))],
    demands,
    class:
      tool === "verter"
        ? (CLASSES.find((c) => demands.some((d) => d.class === c)) ?? "not-run")
        : demands.some((d) => d.class !== "reference")
          ? (demands.find((d) => d.class !== "reference")?.class ?? "not-run")
          : "reference",
    metrics: {
      setupMs: stats(
        records.map((r) => (tool === "verter" ? r.phases?.setup / 1000 : r.phases?.setupMs)),
      ),
      initMs: stats(
        records.map((r) => (tool === "verter" ? r.phases?.init / 1000 : r.phases?.initMs)),
      ),
      steps: session.steps.map((step, s) => ({
        step: s,
        kind: step.kind,
        ms: stats(records.map((r) => stepMs(tool, r.steps?.[s]))),
      })),
      peakBytes: stats(records.map(peakOf)),
    },
  };
  if (tool === "verter") out.retention = records.map((r) => r.retention ?? null);
  return out;
}

/** Summarise the run's sessions. `resolution` is the run's timer resolution. */
export function summarizeSessions(run, sessions, resolution = { single: 1 }) {
  const limits = runLimits(run.meta?.options ?? {});
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const groups = new Map();
  for (const inv of run.sessionInvocations ?? []) {
    if (!groups.has(inv.sessionId)) groups.set(inv.sessionId, {});
    (groups.get(inv.sessionId)[inv.arm] ??= []).push(inv);
  }
  const cells = [];
  for (const id of Object.keys(run.meta?.sessions ?? {})) {
    const session = byId.get(id);
    if (!session) continue;
    const armInvs = groups.get(id) ?? {};
    const arms = Object.fromEntries(
      Object.entries(armInvs).map(([arm, invs]) => [
        arm,
        sessionArmSummary(arm, invs, session, limits),
      ]),
    );
    // A step is compared only where every answer of it matched in Verter and
    // reproduced the constructed answer in tsc.
    const comparison = [];
    if (arms.verter && arms["tsc-api"]) {
      session.steps.forEach((step, s) => {
        if (step.kind === "meta") return;
        const verterOk = arms.verter.demands
          .filter((d) => d.step === s)
          .every((d) => d.class === "matched");
        const tscOk = arms["tsc-api"].demands
          .filter((d) => d.step === s)
          .every((d) => d.class === "reference");
        if (!verterOk || !tscOk) return;
        const times = (arm, tool) =>
          armInvs[arm]
            .filter((i) => !i.warmup && i.session)
            .map((i) => stepMs(tool, i.session.steps?.[s]));
        comparison.push({
          step: s,
          kind: step.kind,
          ...verdict(times("verter", "verter"), times("tsc-api", "tsc"), resolution.single),
        });
      });
    }
    const requiredState =
      arms.verter && arms["verter-observe"]
        ? requiredStateComparison(arms.verter.retention, arms["verter-observe"].retention)
        : null;
    cells.push({
      key: id,
      family: session.family,
      note: session.note,
      arms,
      comparison,
      requiredState,
    });
  }
  return { cells };
}

/** Problems with one session record against the script. */
export function sessionRecordProblems(record, session, { tool }) {
  const p = [];
  const need = (cond, what) => {
    if (!cond) p.push(what);
  };
  need(record?.schema === 1, `session record schema ${record?.schema} is not 1`);
  need(record?.tool === tool, `record from ${record?.tool}, not ${tool}`);
  need(record?.kind === "session", "not a session record");
  need(record?.stage === "complete", `record stage ${record?.stage}, not complete`);
  need(record?.initOutcome?.kind === "value", "the init request did not answer");
  const steps = record?.steps ?? [];
  need(
    steps.length === session.steps.length,
    `${steps.length} steps recorded, the script has ${session.steps.length}`,
  );
  session.steps.forEach((step, s) => {
    const got = steps[s];
    if (!got) return;
    need(got.kind === step.kind, `step ${s} is ${got.kind}, the script says ${step.kind}`);
    if (step.kind === "demand") {
      const reqs = got.requests ?? [];
      need(
        stable(reqs.map((r) => [r.file, r.alias])) ===
          stable(step.requests.map((r) => [r.file, r.alias])),
        `step ${s} answered other demands than the script's`,
      );
      need(
        Boolean(got.concurrent) === Boolean(step.concurrent && step.requests.length > 1),
        `step ${s} concurrency differs from the script`,
      );
      need(stepMs(tool, got) !== null, `step ${s} has no valid time`);
    } else if (step.kind === "edit") {
      need(got.file === step.file, `step ${s} edited ${got.file}, not ${step.file}`);
      need(stepMs(tool, got) !== null, `step ${s} has no valid time`);
    } else if (step.kind === "meta") {
      need(got.file === step.file, `step ${s} read the metadata of ${got.file}, not ${step.file}`);
      if (tool === "verter") need(finite(got.micros), `step ${s} has no valid time`);
      else need(got.applicable === false, `step ${s}: tsc has no component metadata`);
    }
  });
  const reading = tool === "verter" ? record?.afterSteps : record?.serverAfterSteps;
  need(
    typeof reading?.metric === "string" && finite(reading?.peakBytes) && reading.peakBytes > 0,
    "no engine statistics after the steps",
  );
  need(
    (record?.statsErrors ?? []).length === 0,
    `statistics errors: ${(record?.statsErrors ?? []).join("; ")}`,
  );
  return p;
}

/** tsc's incremental evidence: each edit notified through updateSnapshot, snapshots advancing. */
export function tscIncrementalProblems(record, session, projectDir) {
  const p = [];
  if (record?.incremental !== INCREMENTAL_FACILITY)
    p.push(`tsc's incremental facility is ${record?.incremental}, not ${INCREMENTAL_FACILITY}`);
  let last = 0;
  session.steps.forEach((step, s) => {
    if (step.kind !== "edit") return;
    const got = record?.steps?.[s];
    const want = [`${projectDir}/${step.file}`].map((f) => f.replace(/\\/g, "/").toLowerCase());
    const changed = (got?.fileChanges?.changed ?? []).map((f) =>
      String(f).replace(/\\/g, "/").toLowerCase(),
    );
    if (stable(changed) !== stable(want))
      p.push(`step ${s}: the snapshot update did not name the edited file`);
    if (!(Number.isInteger(got?.snapshot) && got.snapshot > last))
      p.push(`step ${s}: the edit did not advance the API snapshot`);
    last = got?.snapshot ?? last;
  });
  const roots = (record?.rootFiles ?? []).map((f) => f.replace(/\\/g, "/").toLowerCase()).sort();
  const want = ["lib.bench.d.ts", ...tscFiles(session)]
    .map((f) => `${projectDir}/${f}`.replace(/\\/g, "/").toLowerCase())
    .sort();
  if (stable(roots) !== stable(want))
    p.push(`tsc program roots ${JSON.stringify(record?.rootFiles)} are not the session's`);
  return p;
}

/**
 * Validate the run's sessions. `selected` are the sessions the run's tier
 * (or --only) selects, `bins` the run's pinned binaries.
 */
export function validateSessions(run, selected, { schedule, bins, libText = null } = {}) {
  const failures = [];
  const fail = (m) => failures.push(m);
  const meta = run.meta ?? {};
  const opts = meta.options ?? {};
  const limits = runLimits(opts);
  const ids = Object.keys(meta.sessions ?? {});
  if (stable([...ids].sort()) !== stable(selected.map((s) => s.id).sort()))
    fail("the recorded sessions are not the ones the tier (or --only) selects");
  const byId = new Map(selected.map((s) => [s.id, s]));
  for (const [id, cell] of Object.entries(meta.sessions ?? {})) {
    const session = byId.get(id);
    if (!session) continue;
    const want = sessionInputs(session, "");
    for (const [file, sha] of Object.entries(want)) {
      if (file === "lib.bench.d.ts") continue;
      if (cell.inputs?.[file] !== sha) fail(`session ${id}: ${file} is not the catalog's`);
    }
    if (libText !== null && cell.inputs?.["lib.bench.d.ts"] !== sha256Text(libText))
      fail(`session ${id}: the library is not the benchmark's`);
  }
  const arms = sessionArmsOf(opts.arms ?? []);
  const plan = schedule(ids, arms, opts.repeat ?? 0, opts.warmup ?? 0).map(
    (p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`,
  );
  if (stable(meta.sessionPlan ?? []) !== stable(plan))
    fail(
      "the recorded session plan is not the counterbalanced schedule for the run's sessions and arms",
    );
  const invs = run.sessionInvocations ?? [];
  if (ids.length && !invs.length) fail("zero session records");
  const seen = new Set();
  invs.forEach((inv, position) => {
    const entry = `${inv.sessionId}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
    if (inv.index !== position)
      fail(`session record ${entry} has index ${inv.index} at position ${position}`);
    if (seen.has(entry)) fail(`duplicate session record ${entry}`);
    seen.add(entry);
    if (plan[position] !== entry)
      fail(
        `session record ${position} is ${entry}; the plan says ${plan[position] ?? "<nothing>"}`,
      );
  });
  for (const entry of plan) if (!seen.has(entry)) fail(`missing session record ${entry}`);

  for (const inv of invs) {
    const id = `${inv.sessionId}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
    const session = byId.get(inv.sessionId);
    if (!session) continue;
    const sup = inv.supervisor;
    for (const p of supervisorRecordProblems(sup)) fail(`${id}: ${p}`);
    if (!sup) continue;
    if (sup.containment !== "hard" && !(sup.containment === "sampled" && opts.allowSampled))
      fail(`${id}: containment ${sup.containment} without consent (--allow-sampled)`);
    const cap = ((opts.memMb ?? 0) + (opts.infraMb ?? 0)) * 1024 * 1024;
    if (sup.memLimitBytes !== cap)
      fail(
        `${id}: containment cap ${sup.memLimitBytes} is not the run's budget plus allowance (${cap})`,
      );
    if (sup.timeoutMs !== supervisorDeadlineMs(opts))
      fail(`${id}: deadline ${sup.timeoutMs} is not the run's ${supervisorDeadlineMs(opts)} ms`);
    const tool = ARMS[inv.arm]?.tool;
    if (tool === "verter") {
      const exe = inv.arm === "verter-observe" ? bins?.observe?.pinned : bins?.probe?.pinned;
      if (inv.command?.[0] !== exe || inv.command?.[1] !== "session")
        fail(
          `${id}: ran ${inv.command?.slice(0, 2).join(" ")}, not the pinned probe's session runner`,
        );
    }
    const end = invocationEnd(inv, limits);
    if (end.kind === "harness-failure") {
      fail(`${id}: failed child: ${end.detail}`);
      continue;
    }
    if (end.kind !== "exited") continue;
    if (sup.exitCode !== 0) {
      fail(`${id}: failed child: exit ${sup.exitCode}`);
      continue;
    }
    if (!inv.session) {
      fail(`${id}: failed child: no session record (${inv.sessionReadError})`);
      continue;
    }
    for (const p of sessionRecordProblems(inv.session, session, { tool })) fail(`${id}: ${p}`);
    if (tool === "verter") {
      if (inv.session.observability !== false)
        fail(`${id}: a session runs the production configuration`);
      if (inv.session.captureAvailable !== (inv.arm === "verter-observe"))
        fail(`${id}: capture availability ${inv.session.captureAvailable} is wrong for ${inv.arm}`);
    } else {
      for (const p of tscIncrementalProblems(inv.session, session, inv.projectDir))
        fail(`${id}: ${p}`);
    }
  }

  const recomputed = meta.sessions
    ? summarizeSessions(run, selected, run.summary?.resolution)
    : null;
  for (const cell of recomputed?.cells ?? []) {
    for (const d of cell.arms["tsc-api"]?.demands ?? [])
      for (const p of d.problems ?? [])
        fail(
          `${cell.key}|tsc-api step ${d.step} ${d.demand}: wrong answer against the constructed answer: ${p}`,
        );
  }
  if (stable(run.summary?.sessions ?? null) !== stable(recomputed))
    fail("the stored session summary disagrees with its raw records");
  return failures;
}
