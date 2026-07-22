/**
 * The native TypeScript reference lane — plain `.ts`/`.tsx` operations driven
 * DIRECTLY against the same provider binaries Verter spawns (tsgo `--lsp
 * --stdio`; `node tsserver.js --useSyntaxServer=false
 * --disableAutomaticTypingAcquisition`, no plugin), with no Verter process in
 * the loop. This is the yardstick the corpus gate's `.vue` numbers are
 * compared against.
 *
 * Invoked via `pnpm --filter @verter/dx-harness test:native-reference`.
 * Requires `VERTER_CORPUS_GATE_DIR` (never committed, never hardcoded);
 * without it the lane records an HONEST EXPLICIT SKIP. The lane always writes
 * its receipt and per-engine trace JSONL files; it FAILS only when an engine
 * session could not run at all (spawn failure / zero requests) — slow-or-empty
 * native results are the reference being measured, not a defect of the lane.
 */
import { describe, expect, it } from "vitest";

import { resolveNativeReferenceEnv } from "../src/native-reference/config.js";
import { runNativeReference } from "../src/native-reference/index.js";

const resolution = resolveNativeReferenceEnv(process.env);

describe("native TypeScript reference (no Verter in the loop)", () => {
  if (resolution.kind === "skip") {
    it.skip(`native reference skipped: ${resolution.reason}`, () => {});
    return;
  }

  const config = resolution.config;
  const laneTimeoutMs = config.engines.length * 3_600_000 + 300_000;

  it(
    `measures ${config.corpusLabel} plain-TS baseline on engines: ${config.engines.join(", ")}`,
    { timeout: laneTimeoutMs },
    async () => {
      const outcome = await runNativeReference(config, (message) => console.log(message));

      for (const engine of config.engines) {
        const report = outcome.receipt.engines[engine];
        if (!report) {
          console.log(`[native-ref] ${engine}: MISSING REPORT`);
          continue;
        }
        console.log(
          `[native-ref] ${engine}: sent=${report.accounting.requestsSent} ` +
            `answered=${report.accounting.requestsAnswered} empty=${report.accounting.requestsEmpty} ` +
            `errored=${report.accounting.requestsErrored} timedOut=${report.accounting.requestsTimedOut} ` +
            `fatal=${report.fatalError ?? "-"}`,
        );
        for (const kind of ["hover", "definition", "completion", "references"] as const) {
          const summary = report.kinds[kind];
          console.log(
            `[native-ref] ${engine} ${kind}: n=${summary.count} p50=${summary.p50Ms}ms ` +
              `p90=${summary.p90Ms}ms p95=${summary.p95Ms}ms max=${summary.maxMs}ms ` +
              `empty=${summary.emptyCount} err=${summary.errorCount} timeout=${summary.timeoutCount}`,
          );
        }
        console.log(
          `[native-ref] ${engine} warmup=${JSON.stringify(report.startup.warmup)} ` +
            `initialize=${report.startup.spawnToInitializeMs}ms wall=${report.wallClockMs}ms`,
        );
      }
      console.log(`[native-ref] receipt: ${outcome.receiptPath}`);

      // The lane's own acceptance bar: every configured engine ran and issued
      // non-zero counted work. Latency/empties are the REFERENCE DATA, not a bar.
      for (const engine of config.engines) {
        const report = outcome.receipt.engines[engine];
        expect(report, `engine ${engine} produced no report`).toBeDefined();
        expect(
          report!.accounting.requestsSent,
          `engine ${engine} sent zero requests (fatal: ${report!.fatalError})`,
        ).toBeGreaterThan(0);
      }
    },
  );
});
