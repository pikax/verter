/**
 * @ai-generated - Measures tsserver-backed script completion latency in a Vue SFC.
 */
import { expect } from "chai";
import * as fs from "fs";
import * as vscode from "vscode";
import {
  FIXTURE_NAME,
  TYPE_PROVIDER,
  findPosition,
  measureCompletion,
  measureTimeToCompletionsMatching,
  openVueFile,
  triggerDecorationRefresh,
  ensureFixtureWarm,
  waitForFileReady,
} from "../helpers";

interface CompletionSample {
  iteration: number;
  latencyMs: number;
}

interface CompletionSummary {
  avgMs: number;
  medianMs: number;
  p95Ms: number;
  minMs: number;
  maxMs: number;
}

const benchmarkSuite = process.env.VERTER_E2E_COMPLETION_BENCHMARK === "1" ? suite : suite.skip;
const reportFile = process.env.VERTER_E2E_COMPLETION_FILE;
const iterations = readIntEnv("VERTER_E2E_COMPLETION_BENCHMARK_ITERATIONS", 10);
const relativePath = process.env.VERTER_E2E_COMPLETION_BENCHMARK_FILE ?? "src/App.vue";
const anchor = process.env.VERTER_E2E_COMPLETION_BENCHMARK_ANCHOR ?? "count.value * 2";
const anchorOffset = readIntEnv("VERTER_E2E_COMPLETION_BENCHMARK_OFFSET", 6);
const expectedLabel = process.env.VERTER_E2E_COMPLETION_BENCHMARK_LABEL ?? "value";
const triggerCharacter = process.env.VERTER_E2E_COMPLETION_BENCHMARK_TRIGGER;
const warmupAnchor = process.env.VERTER_E2E_COMPLETION_BENCHMARK_WARMUP_ANCHOR ?? "props.title";
const warmupOffset = readIntEnv("VERTER_E2E_COMPLETION_BENCHMARK_WARMUP_OFFSET", 6);
const warmupLabel = process.env.VERTER_E2E_COMPLETION_BENCHMARK_WARMUP_LABEL ?? "title";

benchmarkSuite(`Completion Benchmark [${FIXTURE_NAME}]`, function () {
  this.timeout(120_000);

  test("records script member-access completion latency", async function () {
    if (FIXTURE_NAME !== "single-project" || TYPE_PROVIDER !== "tsserver") {
      this.skip();
      return;
    }

    await ensureFixtureWarm();
    const doc = await openVueFile(relativePath);
    const warmupPosition = findPosition(doc, warmupAnchor, warmupOffset);
    expect(warmupPosition, `Expected warmup anchor "${warmupAnchor}"`).to.exist;
    await waitForFileReady(doc, {
      probePosition: warmupPosition!,
      expectedLabel: warmupLabel,
    });

    const position = findPosition(doc, anchor, anchorOffset);
    expect(position, `Expected benchmark anchor "${anchor}"`).to.exist;

    await waitForFileReady(doc, {
      probePosition: position!,
      expectedLabel,
      triggerCharacter,
    });
    const { completions: initialCompletions } = await measureCompletion(
      doc.uri,
      position!,
      triggerCharacter,
    );
    if (!hasTypedCompletion(initialCompletions, expectedLabel)) {
      console.log(`    Initial completions: ${completionPreview(initialCompletions)}`);
    }
    expect(
      hasTypedCompletion(initialCompletions, expectedLabel),
      `Expected an initial typed completion for "${expectedLabel}"`,
    ).to.equal(true);

    const warmSamples: CompletionSample[] = [];
    for (let iteration = 1; iteration <= iterations; iteration += 1) {
      const { completions, latencyMs } = await measureCompletion(
        doc.uri,
        position!,
        triggerCharacter,
      );
      if (!hasTypedCompletion(completions, expectedLabel)) {
        console.log(`    Warm completions: ${completionPreview(completions)}`);
      }
      expect(
        hasTypedCompletion(completions, expectedLabel),
        `Warm completion ${iteration} should include "${expectedLabel}" with a typed kind`,
      ).to.equal(true);
      warmSamples.push({ iteration, latencyMs });
    }

    const afterEditSamples: CompletionSample[] = [];
    for (let iteration = 1; iteration <= iterations; iteration += 1) {
      await triggerDecorationRefresh();
      const { completions, latencyMs } = await measureTimeToCompletionsMatching(
        doc.uri,
        position!,
        {
          triggerCharacter,
          intervalMs: 50,
          stableMs: 0,
          predicate: (list: vscode.CompletionList | undefined) =>
            hasTypedCompletion(list, expectedLabel),
        },
      );
      if (!hasTypedCompletion(completions, expectedLabel)) {
        console.log(`    Post-edit completions: ${completionPreview(completions)}`);
      }
      expect(
        hasTypedCompletion(completions, expectedLabel),
        `Post-edit completion ${iteration} should include "${expectedLabel}" with a typed kind`,
      ).to.equal(true);
      afterEditSamples.push({ iteration, latencyMs });
    }

    const report = {
      fixture: FIXTURE_NAME,
      providerKind: TYPE_PROVIDER,
      generatedAt: new Date().toISOString(),
      target: {
        relativePath,
        anchor,
        anchorOffset,
        expectedLabel,
        triggerCharacter,
      },
      iterations,
      warmRequest: {
        summary: summarizeCompletionSamples(warmSamples),
        samples: warmSamples,
      },
      afterEditTyped: {
        summary: summarizeCompletionSamples(afterEditSamples),
        samples: afterEditSamples,
      },
    };

    if (reportFile) {
      fs.writeFileSync(reportFile, JSON.stringify(report, null, 2));
    }

    console.log(
      `    Warm median=${report.warmRequest.summary.medianMs}ms, ` +
        `after-edit median=${report.afterEditTyped.summary.medianMs}ms`,
    );
  });
});

function hasTypedCompletion(list: vscode.CompletionList | undefined, label: string): boolean {
  const match = list?.items.find(
    (item) => (typeof item.label === "string" ? item.label : item.label.label) === label,
  );
  return match?.kind !== undefined && match.kind !== vscode.CompletionItemKind.Text;
}

function completionPreview(list: vscode.CompletionList | undefined): string {
  if (!list || list.items.length === 0) {
    return "<empty>";
  }

  return list.items
    .slice(0, 10)
    .map((item) => {
      const label = typeof item.label === "string" ? item.label : item.label.label;
      const kind =
        item.kind !== undefined
          ? (vscode.CompletionItemKind[item.kind] ?? String(item.kind))
          : "undefined";
      return `${label}:${kind}`;
    })
    .join(", ");
}

function summarizeCompletionSamples(samples: readonly CompletionSample[]): CompletionSummary {
  const latencies = samples.map((sample) => sample.latencyMs).sort((left, right) => left - right);

  const avgMs = Math.round(
    latencies.reduce((total, latency) => total + latency, 0) / latencies.length,
  );
  const p95Index = Math.min(Math.ceil(latencies.length * 0.95) - 1, latencies.length - 1);

  return {
    avgMs,
    medianMs: percentile(latencies, 0.5),
    p95Ms: latencies[p95Index],
    minMs: latencies[0],
    maxMs: latencies[latencies.length - 1],
  };
}

function percentile(sortedValues: readonly number[], percentileValue: number): number {
  if (sortedValues.length === 0) {
    return 0;
  }

  const index = Math.min(
    sortedValues.length - 1,
    Math.max(0, Math.ceil(sortedValues.length * percentileValue) - 1),
  );
  return sortedValues[index];
}

function readIntEnv(name: string, fallback: number): number {
  const raw = process.env[name];
  if (!raw) {
    return fallback;
  }

  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}
