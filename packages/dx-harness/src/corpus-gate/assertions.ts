/**
 * The corpus-gate acceptance bar, evaluated as pure functions over a receipt.
 *
 * The bar (all thresholds env-configurable):
 *  - zero wedges, zero fatal route errors, zero budget blow-throughs;
 *  - hover p95 < bar, definition p95 < bar, completion p95 < bar,
 *    references p95 < bar — GATING only for routes measured in isolation;
 *  - bounded memory (per tracked process, when the platform can read RSS);
 *  - provable provider attribution: the sampled provider must structurally be
 *    the real provider, and no tree member may go unsampled;
 *  - zero unexpected empty results;
 *  - NON-VACUITY: every requested route reported, every route fired requests,
 *    every request kind measured at least once, and the request/response
 *    accounting identity holds exactly. A run that fired zero requests or
 *    silently skipped work FAILS — it never passes vacuously.
 *
 * FIDELITY GATING. Latency percentiles measured while other route sessions
 * shared the executor measure the box, not the server, so they are recorded as
 * ADVISORY and cannot decide pass/fail. Everything else — stability, wedge,
 * liveness, unexpected-empty, accounting, memory, attribution — gates in BOTH
 * modes: contention does not make a wedge or an empty result valid. Advisories
 * are returned from a SEPARATE function and every line is prefixed `ADVISORY`,
 * so an advisory number can never be read as a gating verdict.
 *
 * At the current tip several of these are EXPECTED to fail on a real corpus —
 * the gate's job is to report that precisely, not to hang or to hide it.
 */
import { UNPROVEN_ISOLATION } from "./topology.js";
import {
  CORPUS_REQUEST_KINDS,
  type CorpusGateReceipt,
  type CorpusGateRoute,
  type CorpusGateThresholds,
  type CorpusRouteIsolation,
  type CorpusRouteReport,
} from "./types.js";

/** Fail-closed isolation read: a report that recorded none never gates latency. */
export function reportIsolation(report: CorpusRouteReport): CorpusRouteIsolation {
  return report.isolation ?? UNPROVEN_ISOLATION;
}

/** Evaluate one route report against the bar; returns GATING failure strings. */
export function evaluateRoute(
  report: CorpusRouteReport,
  thresholds: CorpusGateThresholds,
  options: { readonly requireIsolatedLatency?: boolean } = {},
): string[] {
  const failures: string[] = [];
  const tag = `[${report.route}]`;
  const isolation = reportIsolation(report);
  const earlyStopped = report.earlyStop?.stopped === true;

  if (report.fatalError !== null) {
    failures.push(`${tag} fatal route error: ${report.fatalError}`);
  }
  if (report.wedged) {
    failures.push(`${tag} WEDGED: ${report.wedgeDetail ?? "no detail recorded"}`);
  }
  if (report.wallClock.budgetExceeded) {
    failures.push(
      `${tag} exceeded its wall-clock budget (${report.wallClock.elapsedMs}ms > ${report.wallClock.budgetMs}ms)`,
    );
  }

  // Isolation integrity: a declared-dedicated executor refuted by observed
  // concurrency is a defect in the run's own topology, not a soft downgrade.
  if (isolation.attestationContradicted) {
    failures.push(
      `${tag} isolation attestation CONTRADICTED: executor declared ${isolation.executor} but ` +
        `${isolation.observedConcurrentRoutes} concurrent route sessions were observed`,
    );
  }
  if (options.requireIsolatedLatency === true && !isolation.latencyGating) {
    failures.push(
      `${tag} latency verdict is not gating but isolation was REQUIRED: ${isolation.evidence}`,
    );
  }

  // Non-vacuity: the route must have actually driven traffic.
  const accounting = report.accounting;
  if (accounting.requestsSent === 0) {
    failures.push(`${tag} vacuous run: zero requests were fired`);
  }
  if (accounting.filesOpened === 0) {
    failures.push(`${tag} vacuous run: zero sampled files were opened`);
  }
  if (accounting.probesMined === 0) {
    failures.push(`${tag} vacuous run: zero authored probes were mined`);
  }
  const settledSum =
    accounting.requestsAnswered +
    accounting.requestsTimedOut +
    accounting.requestsErrored +
    accounting.requestsAbandoned;
  if (accounting.requestsSent !== settledSum) {
    failures.push(
      `${tag} accounting identity violated: sent=${accounting.requestsSent} !== ` +
        `answered+timedOut+errored+abandoned=${settledSum}`,
    );
  }
  if (accounting.requestsEmpty > accounting.requestsAnswered) {
    failures.push(
      `${tag} accounting identity violated: empty=${accounting.requestsEmpty} > answered=${accounting.requestsAnswered}`,
    );
  }

  const p95Bars: Readonly<Record<(typeof CORPUS_REQUEST_KINDS)[number], number>> = {
    hover: thresholds.hoverP95Ms,
    definition: thresholds.definitionP95Ms,
    completion: thresholds.completionP95Ms,
    references: thresholds.referencesP95Ms,
  };
  for (const kind of CORPUS_REQUEST_KINDS) {
    const summary = report.kinds[kind];
    if (summary.count === 0) {
      // A wedged/fatal/budget-cut/early-stopped route legitimately measured
      // less than planned; the failure that cut it short already owns that
      // outcome (early stop only ever triggers AFTER a failure was recorded).
      if (
        !report.wedged &&
        report.fatalError === null &&
        !report.wallClock.budgetExceeded &&
        !earlyStopped
      ) {
        failures.push(`${tag} vacuous kind: zero ${kind} requests were measured`);
      }
      continue;
    }
    const bar = p95Bars[kind];
    // Latency gates only where it was measured in isolation. A contended
    // breach is still REPORTED — as an advisory, never silently dropped.
    if (summary.p95Ms >= bar && isolation.latencyGating) {
      failures.push(`${tag} ${kind} p95 ${summary.p95Ms}ms breaches the < ${bar}ms bar`);
    }
    if (summary.unexpectedEmptyCount > 0) {
      failures.push(
        `${tag} ${kind} returned ${summary.unexpectedEmptyCount} unexpected empty result(s)`,
      );
    }
  }

  for (const trend of report.memory) {
    if (!trend.supported || trend.maxRssBytes === null) continue;
    if (trend.maxRssBytes > thresholds.rssMaxBytes) {
      failures.push(
        `${tag} ${trend.label} max RSS ${trend.maxRssBytes} exceeds the ${thresholds.rssMaxBytes} byte ceiling`,
      );
    }
  }

  failures.push(...evaluateProviderAttribution(report));
  return failures;
}

/**
 * Provider-sample attribution: the ceiling is meaningless if the sampler is
 * bounding the wrong process. A missing or mis-attributed provider is a LOUD
 * failure; an unobservable platform is an explicit advisory (see below), never
 * a silent pass.
 */
function evaluateProviderAttribution(report: CorpusRouteReport): string[] {
  const failures: string[] = [];
  const tag = `[${report.route}]`;
  // A route that never fired a request never had a provider to attribute; the
  // fatal/vacuity failures above already own that outcome.
  if (report.accounting.requestsSent === 0) return failures;

  const attribution = report.providerAttribution;
  if (attribution === undefined) {
    failures.push(
      `${tag} provider attribution was never recorded — the per-process RSS ceiling cannot be ` +
        `shown to have observed the real provider`,
    );
    return failures;
  }
  if (attribution.status === "missing" || attribution.status === "mismatched") {
    failures.push(
      `${tag} provider attribution ${attribution.status.toUpperCase()}: ${attribution.detail}`,
    );
  }
  if (attribution.unattributedPids.length > 0) {
    failures.push(
      `${tag} ${attribution.unattributedPids.length} process(es) in the server tree were never ` +
        `sampled (pids ${attribution.unattributedPids.join(", ")}) — their memory is unbounded`,
    );
  }
  if (
    attribution.status === "verified" &&
    attribution.sampledProcessCount === 0 &&
    report.memory.length === 0
  ) {
    failures.push(`${tag} provider attribution claims verified but zero processes were sampled`);
  }
  return failures;
}

/**
 * Non-gating observations for one route. Every line is prefixed `ADVISORY` and
 * these never enter `assertionFailures` — an advisory can never flip `pass`,
 * and a gating verdict can never be mistaken for one of these.
 */
export function evaluateRouteAdvisories(
  report: CorpusRouteReport,
  thresholds: CorpusGateThresholds,
): string[] {
  const advisories: string[] = [];
  const tag = `[${report.route}]`;
  const isolation = reportIsolation(report);

  if (!isolation.latencyGating) {
    advisories.push(
      `ADVISORY ${tag} latency percentiles are NOT gating (${isolation.mode}): ${isolation.evidence}`,
    );
    const p95Bars: Readonly<Record<(typeof CORPUS_REQUEST_KINDS)[number], number>> = {
      hover: thresholds.hoverP95Ms,
      definition: thresholds.definitionP95Ms,
      completion: thresholds.completionP95Ms,
      references: thresholds.referencesP95Ms,
    };
    for (const kind of CORPUS_REQUEST_KINDS) {
      const summary = report.kinds[kind];
      if (summary.count === 0) continue;
      if (summary.p95Ms >= p95Bars[kind]) {
        advisories.push(
          `ADVISORY ${tag} ${kind} p95 ${summary.p95Ms}ms would breach the < ${p95Bars[kind]}ms bar ` +
            `(recorded under contention — NOT a gating verdict)`,
        );
      }
    }
  }
  if (report.providerAttribution?.status === "unobservable") {
    advisories.push(
      `ADVISORY ${tag} provider attribution unobservable: ${report.providerAttribution.detail}`,
    );
  }
  if (report.earlyStop?.stopped === true) {
    advisories.push(
      `ADVISORY ${tag} route stopped early (fast mode): ${report.earlyStop.reason ?? "no reason recorded"} ` +
        `— per-kind census counts are deliberately incomplete`,
    );
  }
  return advisories;
}

/**
 * Evaluate the whole receipt: per-route bars plus completeness — every
 * requested route must be present (a silently missing route is a FAIL).
 */
export function evaluateCorpusGate(
  receipt: CorpusGateReceipt,
  requestedRoutes: readonly CorpusGateRoute[],
  thresholds: CorpusGateThresholds,
  options: { readonly requireIsolatedLatency?: boolean } = {},
): string[] {
  const failures: string[] = [];
  for (const route of requestedRoutes) {
    const report = receipt.routes[route];
    if (!report) {
      failures.push(`[${route}] requested route produced no report (silent skip is a FAIL)`);
      continue;
    }
    failures.push(...evaluateRoute(report, thresholds, options));
  }
  if (Object.keys(receipt.routes).length === 0) {
    failures.push("receipt contains zero route reports (vacuous run)");
  }
  if (receipt.budget?.exceeded === true && receipt.budget.fatal) {
    failures.push(
      `whole-gate wall clock ${receipt.budget.actualMs}ms exceeds the ${receipt.budget.targetMs}ms budget`,
    );
  }
  return failures;
}

/** Whole-receipt advisories (never gating, always reported). */
export function evaluateCorpusGateAdvisories(
  receipt: CorpusGateReceipt,
  requestedRoutes: readonly CorpusGateRoute[],
  thresholds: CorpusGateThresholds,
): string[] {
  const advisories: string[] = [];
  for (const route of requestedRoutes) {
    const report = receipt.routes[route];
    if (!report) continue;
    advisories.push(...evaluateRouteAdvisories(report, thresholds));
  }
  if (receipt.budget !== undefined && receipt.budget.exceeded && !receipt.budget.fatal) {
    advisories.push(
      `ADVISORY whole-gate wall clock ${receipt.budget.actualMs}ms exceeds the ` +
        `${receipt.budget.targetMs}ms budget (reported, not fatal)`,
    );
  }
  return advisories;
}
