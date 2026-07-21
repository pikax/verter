/**
 * Shard aggregation for the fanned-out corpus gate.
 *
 * The CI topology runs ONE route per machine — each shard is a single-route
 * serial gate on a dedicated executor, so the wall clock collapses to the
 * slowest route while every route's latency stays a valid, gating measurement.
 * This module recombines those per-shard receipts into one verdict.
 *
 * It is the place a fan-out quietly loses coverage, so it is deliberately
 * suspicious: an expected route with no receipt is a FAILURE (never a skip),
 * two receipts claiming the same route is a FAILURE (ambiguous evidence), and
 * a shard that reported failures keeps them — the summary can only ever be as
 * green as the reds it was handed.
 */
import type { CorpusGateReceipt, CorpusGateRoute } from "./types.js";

export interface ShardWallClock {
  readonly perRoute: readonly { readonly route: CorpusGateRoute; readonly ms: number }[];
  /** Wall clock the fan-out actually costs: the slowest shard. */
  readonly fanOutMs: number;
  /** Machine time consumed across every shard (cost, not latency). */
  readonly totalMachineMs: number;
}

export interface ShardSummary {
  readonly expectedRoutes: readonly CorpusGateRoute[];
  readonly coveredRoutes: readonly CorpusGateRoute[];
  readonly missingRoutes: readonly CorpusGateRoute[];
  readonly latencyGatingRoutes: readonly CorpusGateRoute[];
  readonly latencyAdvisoryRoutes: readonly CorpusGateRoute[];
  readonly failures: readonly string[];
  readonly advisories: readonly string[];
  readonly wallClock: ShardWallClock;
  readonly budget: {
    readonly targetMs: number;
    readonly fanOutMs: number;
    readonly exceeded: boolean;
  };
  readonly pass: boolean;
}

/**
 * Recombine shard receipts into one verdict (pure).
 *
 * `pass` requires every expected route to be covered exactly once AND every
 * shard to have passed its own bar. Budget breach is REPORTED, not fatal —
 * the product-side latency work moves that number, and a slow-but-correct
 * gate is not a broken gate.
 */
export function summarizeShards(
  receipts: readonly CorpusGateReceipt[],
  expectedRoutes: readonly CorpusGateRoute[],
  budgetMs: number,
): ShardSummary {
  const failures: string[] = [];
  const advisories: string[] = [];
  const seen = new Map<CorpusGateRoute, number>();
  const perRoute: { route: CorpusGateRoute; ms: number }[] = [];
  const latencyGatingRoutes: CorpusGateRoute[] = [];
  const latencyAdvisoryRoutes: CorpusGateRoute[] = [];

  for (const receipt of receipts) {
    for (const [route, report] of Object.entries(receipt.routes) as [
      CorpusGateRoute,
      CorpusGateReceipt["routes"][CorpusGateRoute],
    ][]) {
      if (report === undefined) continue;
      seen.set(route, (seen.get(route) ?? 0) + 1);
      perRoute.push({ route, ms: report.wallClock.elapsedMs });
      if (report.isolation?.latencyGating === true) latencyGatingRoutes.push(route);
      else latencyAdvisoryRoutes.push(route);
    }
    failures.push(...receipt.assertionFailures);
    advisories.push(...(receipt.advisories ?? []));
  }

  for (const route of expectedRoutes) {
    const count = seen.get(route) ?? 0;
    if (count === 0) {
      failures.push(
        `[${route}] shard produced no receipt — a missing shard is a FAIL, never a silent skip`,
      );
    } else if (count > 1) {
      failures.push(
        `[${route}] ${count} shard receipts claim this route — ambiguous evidence, refusing to merge`,
      );
    }
  }
  for (const route of seen.keys()) {
    if (!expectedRoutes.includes(route)) {
      advisories.push(`ADVISORY [${route}] shard receipt covers a route that was not expected`);
    }
  }

  const shardWallClocks = receipts.map(
    (receipt) =>
      receipt.budget?.actualMs ??
      Object.values(receipt.routes).reduce(
        (total, report) => total + (report?.wallClock.elapsedMs ?? 0),
        0,
      ),
  );
  const fanOutMs = shardWallClocks.length > 0 ? Math.max(...shardWallClocks) : 0;
  const totalMachineMs = shardWallClocks.reduce((total, value) => total + value, 0);
  const exceeded = fanOutMs > budgetMs;
  if (exceeded) {
    advisories.push(
      `ADVISORY fan-out wall clock ${fanOutMs}ms exceeds the ${budgetMs}ms gate budget ` +
        `(slowest shard; total machine time ${totalMachineMs}ms)`,
    );
  }

  const missingRoutes = expectedRoutes.filter((route) => (seen.get(route) ?? 0) === 0);
  return {
    expectedRoutes: [...expectedRoutes],
    coveredRoutes: [...seen.keys()],
    missingRoutes,
    latencyGatingRoutes,
    latencyAdvisoryRoutes,
    failures,
    advisories,
    wallClock: { perRoute, fanOutMs, totalMachineMs },
    budget: { targetMs: budgetMs, fanOutMs, exceeded },
    pass: failures.length === 0,
  };
}

/** Render a shard summary as human-readable lines. */
export function formatShardSummary(summary: ShardSummary): string[] {
  const lines: string[] = [];
  lines.push(
    `routes expected: ${summary.expectedRoutes.join(", ") || "(none)"} | ` +
      `covered: ${summary.coveredRoutes.join(", ") || "(none)"}`,
  );
  lines.push(
    `latency GATING: ${summary.latencyGatingRoutes.join(", ") || "(none)"} | ` +
      `ADVISORY: ${summary.latencyAdvisoryRoutes.join(", ") || "(none)"}`,
  );
  for (const entry of summary.wallClock.perRoute) {
    lines.push(`  ${entry.route}: ${entry.ms}ms`);
  }
  lines.push(
    `wall clock (fan-out) ${summary.wallClock.fanOutMs}ms vs ${summary.budget.targetMs}ms budget ` +
      `— ${summary.budget.exceeded ? "OVER BUDGET" : "within budget"}; ` +
      `total machine time ${summary.wallClock.totalMachineMs}ms`,
  );
  for (const advisory of summary.advisories) lines.push(advisory);
  for (const failure of summary.failures) lines.push(`FAIL ${failure}`);
  lines.push(summary.pass ? "corpus gate: PASS" : "corpus gate: FAIL");
  return lines;
}
