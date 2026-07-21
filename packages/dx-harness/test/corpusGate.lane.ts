/**
 * The corpus benchmark gate lane — the REAL all-route run.
 *
 * Invoked via `pnpm --filter @verter/dx-harness test:corpus-gate`. Requires an
 * EXTERNAL corpus root in `VERTER_CORPUS_GATE_DIR` (never committed, never
 * hardcoded); without it the lane records an HONEST EXPLICIT SKIP. With it,
 * the lane runs every configured route (tsserver, managed tsgo, shared tsgo)
 * under hard wall-clock caps, always emits the receipt (pass or fail), prints
 * the diff against `VERTER_CORPUS_GATE_BASELINE` when configured, and then
 * enforces the acceptance bar. At a tip where the system strands at scale the
 * lane FAILS with the precise failure list — reporting red precisely, never
 * hanging, is this lane's job.
 *
 * Prerequisites for a real run (same as the endurance lanes):
 *   cargo build -p verter_lsp            # target/debug/verter-lsp[.exe]
 *   cargo build -p verter_relay_shim     # only for the shared-tsgo route
 *   pnpm --filter @verter/typescript-plugin build   # only for tsserver
 */
import { describe, expect, it } from "vitest";

import { resolveCorpusGateEnv } from "../src/corpus-gate/config.js";
import { runCorpusGate } from "../src/corpus-gate/index.js";

const resolution = resolveCorpusGateEnv(process.env);

describe("corpus gate (all-route benchmark)", () => {
  if (resolution.kind === "skip") {
    // Honest explicit skip: visible in the run output, never a silent pass.
    it.skip(`corpus gate skipped: ${resolution.reason}`, () => {});
    return;
  }

  const config = resolution.config;
  // Harness-level hard cap: every route session is already raced against
  // routeBudgetMs (+90s teardown grace) inside runCorpusGate; the vitest
  // timeout sits above the sum so the reporter, not the timeout, tells the story.
  const laneTimeoutMs = config.routes.length * (config.routeBudgetMs + 120_000) + 300_000;

  it(
    `benchmarks ${config.corpusLabel} on routes: ${config.routes.join(", ")}`,
    { timeout: laneTimeoutMs },
    async () => {
      const outcome = await runCorpusGate(config);

      // Non-vacuity attestation happens inside the bar (zero-request routes and
      // missing routes fail), but surface the accounting here for the log.
      for (const route of config.routes) {
        const report = outcome.receipt.routes[route];
        console.log(
          `[corpus-gate] ${route}: sent=${report?.accounting.requestsSent ?? "MISSING"} ` +
            `answered=${report?.accounting.requestsAnswered ?? "-"} ` +
            `timedOut=${report?.accounting.requestsTimedOut ?? "-"} ` +
            `abandoned=${report?.accounting.requestsAbandoned ?? "-"} ` +
            `wedged=${report?.wedged ?? "-"} ` +
            `latency=${report?.isolation?.latencyGating === true ? "GATING" : "ADVISORY"} ` +
            `(${report?.isolation?.mode ?? "unrecorded"}) ` +
            `provider=${report?.providerAttribution?.status ?? "unrecorded"}`,
        );
      }
      // Advisories are recorded observations that DO NOT gate — printed apart
      // from the failure list so the two can never be confused.
      for (const advisory of outcome.advisories) console.log(`[corpus-gate] ${advisory}`);
      const budget = outcome.receipt.budget;
      if (budget) {
        console.log(
          `[corpus-gate] wall clock ${budget.actualMs}ms vs ${budget.targetMs}ms budget ` +
            `(${budget.exceeded ? "OVER BUDGET" : "within budget"})`,
        );
      }
      console.log(`[corpus-gate] receipt: ${outcome.receiptPath}`);

      // The acceptance bar. At the current tip a real corpus run is EXPECTED
      // to fail here (wedges + latency); the receipt and this failure list are
      // the deliverable either way.
      expect(
        outcome.failures,
        `corpus-gate acceptance bar failures:\n${outcome.failures.join("\n")}`,
      ).toEqual([]);
    },
  );
});
