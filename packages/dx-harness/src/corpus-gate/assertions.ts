/**
 * The corpus-gate acceptance bar, evaluated as pure functions over a receipt.
 *
 * The bar (all thresholds env-configurable):
 *  - zero wedges, zero fatal route errors, zero budget blow-throughs;
 *  - hover p95 < bar, definition p95 < bar, completion p95 < bar,
 *    references p95 < bar;
 *  - bounded memory (per tracked process, when the platform can read RSS);
 *  - zero unexpected empty results;
 *  - NON-VACUITY: every requested route reported, every route fired requests,
 *    every request kind measured at least once, and the request/response
 *    accounting identity holds exactly. A run that fired zero requests or
 *    silently skipped work FAILS — it never passes vacuously.
 *
 * At the current tip several of these are EXPECTED to fail on a real corpus —
 * the gate's job is to report that precisely, not to hang or to hide it.
 */
import {
  CORPUS_REQUEST_KINDS,
  type CorpusGateReceipt,
  type CorpusGateRoute,
  type CorpusGateThresholds,
  type CorpusRouteReport,
} from "./types.js";

/** Evaluate one route report against the bar; returns failure strings. */
export function evaluateRoute(
  report: CorpusRouteReport,
  thresholds: CorpusGateThresholds,
): string[] {
  const failures: string[] = [];
  const tag = `[${report.route}]`;

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
      // A wedged/fatal/budget-cut route legitimately measured less than planned;
      // the wedge/fatal/budget failure above already owns that outcome.
      if (!report.wedged && report.fatalError === null && !report.wallClock.budgetExceeded) {
        failures.push(`${tag} vacuous kind: zero ${kind} requests were measured`);
      }
      continue;
    }
    const bar = p95Bars[kind];
    if (summary.p95Ms >= bar) {
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
  return failures;
}

/**
 * Evaluate the whole receipt: per-route bars plus completeness — every
 * requested route must be present (a silently missing route is a FAIL).
 */
export function evaluateCorpusGate(
  receipt: CorpusGateReceipt,
  requestedRoutes: readonly CorpusGateRoute[],
  thresholds: CorpusGateThresholds,
): string[] {
  const failures: string[] = [];
  for (const route of requestedRoutes) {
    const report = receipt.routes[route];
    if (!report) {
      failures.push(`[${route}] requested route produced no report (silent skip is a FAIL)`);
      continue;
    }
    failures.push(...evaluateRoute(report, thresholds));
  }
  if (Object.keys(receipt.routes).length === 0) {
    failures.push("receipt contains zero route reports (vacuous run)");
  }
  return failures;
}
