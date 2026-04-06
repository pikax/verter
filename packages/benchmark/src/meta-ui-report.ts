import {
  summarizeLatencySeries,
  VOLAR_PARITY_EXCLUDED_COLLECTIONS,
  type MetaUiBackend,
  type MetaUiOutcomeBucket,
  type MetaUiScenario,
  type NumericSummary,
} from "./meta-ui-core.js";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export interface MetaUiBenchmarkRunVersion {
  benchmarkPackageVersion: string;
  verterCommitSha: string;
  resolvedTargetSha: string;
  vueComponentMetaVersion: string;
  nodeVersion: string;
}

export interface ComponentResultRow {
  relativePath: string;
  componentName: string;
  latencyMs: number | null;
  outcome: MetaUiOutcomeBucket;
  error: string | null;
}

export interface MetaUiBenchmarkRunRepeat {
  index: number;
  orderStart: number;
  setupMs: number;
  warmupMs: number;
  steadyStateMs: number;
  endToEndMs: number;
  componentResults: ComponentResultRow[];
  outcomeCounts: Record<"success" | "degraded" | "query_error" | "crash", number>;
  deviationTotals: {
    exactMatches: number;
    totalMissing: number;
    totalExtra: number;
    totalFieldMismatches: number;
  };
  stats: NumericSummary;
}

export interface MetaUiBenchmarkRun {
  kind: "meta-ui-benchmark-run";
  generatedAt: string;
  version: MetaUiBenchmarkRunVersion;
  target: {
    project: string;
    repo: string;
    branch: string;
    root: string;
    componentsDir: string;
    componentCount: number;
  };
  config: {
    backend: MetaUiBackend;
    scenario: MetaUiScenario;
    repeats: number;
    warmupPasses: number;
    runtimeMode: "shared" | "dedicated";
  };
  repeats: MetaUiBenchmarkRunRepeat[];
  summary: {
    setupMs: NumericSummary;
    warmupMs: NumericSummary;
    steadyStateMs: NumericSummary;
    endToEndMs: NumericSummary;
    outcomeCounts: Record<"success" | "degraded" | "query_error" | "crash", number>;
    deviationTotals: {
      exactMatches: number;
      totalMissing: number;
      totalExtra: number;
      totalFieldMismatches: number;
    };
  };
}

export interface MetaUiAggregateReport {
  kind: "meta-ui-benchmark-report";
  generatedAt: string;
  version: MetaUiBenchmarkRunVersion;
  target: MetaUiBenchmarkRun["target"];
  missingRuns: Array<{
    backend: MetaUiBackend;
    scenario: MetaUiScenario;
  }>;
  scenarios: Record<
    MetaUiScenario,
    {
      backends: Partial<
        Record<
          MetaUiBackend,
          {
            run: MetaUiBenchmarkRun;
            relativeToVerter: number | null;
            relativeToBaseline: number | null;
          }
        >
      >;
    }
  >;
}

const SCENARIOS: MetaUiScenario[] = [
  "single_cold",
  "single_warm",
  "repo_first_pass",
  "repo_warm_second_pass",
];

const BACKENDS: MetaUiBackend[] = ["vue-component-meta", "verter"];

export function buildMetaUiAggregateReport(runs: MetaUiBenchmarkRun[]): MetaUiAggregateReport {
  if (runs.length === 0) {
    throw new Error("Cannot build a meta-ui benchmark report without run data.");
  }

  const scenarioMap = Object.fromEntries(
    SCENARIOS.map((scenario) => [scenario, { backends: {} }]),
  ) as MetaUiAggregateReport["scenarios"];

  for (const run of runs) {
    scenarioMap[run.config.scenario].backends[run.config.backend] = {
      run,
      relativeToVerter: null,
      relativeToBaseline: null,
    };
  }

  for (const scenario of SCENARIOS) {
    const entry = scenarioMap[scenario];
    const verterP50 = entry.backends.verter?.run.summary.steadyStateMs.p50 ?? null;
    const baselineP50 = entry.backends["vue-component-meta"]?.run.summary.steadyStateMs.p50 ?? null;

    for (const backend of Object.keys(entry.backends) as MetaUiBackend[]) {
      const backendEntry = entry.backends[backend];
      if (!backendEntry) {
        continue;
      }
      const p50 = backendEntry.run.summary.steadyStateMs.p50;
      backendEntry.relativeToVerter = verterP50 && verterP50 > 0 ? p50 / verterP50 : null;
      backendEntry.relativeToBaseline = baselineP50 && baselineP50 > 0 ? p50 / baselineP50 : null;
    }
  }

  const first = runs[0];
  const missingRuns = SCENARIOS.flatMap((scenario) =>
    BACKENDS.filter((backend) => !scenarioMap[scenario].backends[backend]).map((backend) => ({
      backend,
      scenario,
    })),
  );
  return {
    kind: "meta-ui-benchmark-report",
    generatedAt: new Date().toISOString(),
    version: first.version,
    target: first.target,
    missingRuns,
    scenarios: scenarioMap,
  };
}

export function buildMetaUiMarkdownReport(report: MetaUiAggregateReport): string {
  const lines: string[] = [];
  lines.push("## Meta UI Benchmark Results");
  lines.push("");
  lines.push(
    `**${report.target.project}** (\`${report.target.repo}@${report.version.resolvedTargetSha}\`) - ${report.target.componentCount.toLocaleString()} components`,
  );
  lines.push("");
  if (VOLAR_PARITY_EXCLUDED_COLLECTIONS.length > 0) {
    lines.push(
      `Parity totals exclude non-equivalent surfaces: ${VOLAR_PARITY_EXCLUDED_COLLECTIONS.join(", ")}.`,
    );
    lines.push("");
  }
  if (report.missingRuns.length > 0) {
    lines.push(
      `Partial results: missing ${report.missingRuns.length.toLocaleString()} backend/scenario run(s).`,
    );
    lines.push("");
  }

  for (const scenario of SCENARIOS) {
    const backends = report.scenarios[scenario].backends;
    const backendNames = Object.keys(backends) as MetaUiBackend[];
    if (backendNames.length === 0) {
      continue;
    }

    lines.push(`### ${scenario}`);
    lines.push("");
    lines.push(
      "| Backend | steady p50 | end-to-end p50 | p95 | stddev | vs Verter | vs vue-component-meta | exact | missing | extra | mismatches | degraded | crashes | errors |",
    );
    lines.push("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");

    for (const backend of backendNames.sort()) {
      const entry = backends[backend];
      if (!entry) {
        continue;
      }
      const run = entry.run;
      lines.push(
        `| ${backend} | ${formatMs(run.summary.steadyStateMs.p50)} | ${formatMs(run.summary.endToEndMs.p50)} | ${formatMs(run.summary.steadyStateMs.p95)} | ${formatMs(run.summary.steadyStateMs.stddev)} | ${formatRatio(entry.relativeToVerter)} | ${formatRatio(entry.relativeToBaseline)} | ${run.summary.deviationTotals.exactMatches.toLocaleString()} | ${run.summary.deviationTotals.totalMissing.toLocaleString()} | ${run.summary.deviationTotals.totalExtra.toLocaleString()} | ${run.summary.deviationTotals.totalFieldMismatches.toLocaleString()} | ${run.summary.outcomeCounts.degraded.toLocaleString()} | ${run.summary.outcomeCounts.crash.toLocaleString()} | ${run.summary.outcomeCounts.query_error.toLocaleString()} |`,
      );
    }

    lines.push("");

    // Top Outliers section — show top 10 slowest components per backend
    const outlierEntries: Array<{
      backend: MetaUiBackend;
      rows: ComponentResultRow[];
    }> = [];
    for (const backend of backendNames.sort()) {
      const entry = backends[backend];
      if (!entry) continue;
      const allResults = entry.run.repeats.flatMap((r) => r.componentResults);
      if (allResults.length === 0) continue;
      const sorted = [...allResults]
        .filter((r) => r.latencyMs !== null)
        .sort((a, b) => (b.latencyMs ?? 0) - (a.latencyMs ?? 0))
        .slice(0, 10);
      if (sorted.length > 0) {
        outlierEntries.push({ backend, rows: sorted });
      }
    }

    if (outlierEntries.length > 0) {
      lines.push(`#### Top Outliers (${scenario})`);
      lines.push("");
      for (const { backend, rows } of outlierEntries) {
        lines.push(`**${backend}**`);
        lines.push("");
        lines.push("| Component | Latency | Outcome |");
        lines.push("|---|---:|---|");
        for (const row of rows) {
          lines.push(`| ${row.componentName} | ${formatMs(row.latencyMs ?? 0)} | ${row.outcome} |`);
        }
        lines.push("");
      }
    }
  }

  return lines.join("\n");
}

export function aggregateRunFromRepeats(
  run: Omit<MetaUiBenchmarkRun, "summary">,
): MetaUiBenchmarkRun {
  const setupSeries = run.repeats.map((repeat) => repeat.setupMs);
  const warmupSeries = run.repeats.map((repeat) => repeat.warmupMs);
  const steadySeries = run.repeats.map((repeat) => repeat.steadyStateMs);
  const endToEndSeries = run.repeats.map((repeat) => repeat.endToEndMs);

  return {
    ...run,
    summary: {
      setupMs: summarizeLatencySeries(setupSeries),
      warmupMs: summarizeLatencySeries(warmupSeries),
      steadyStateMs: summarizeLatencySeries(steadySeries),
      endToEndMs: summarizeLatencySeries(endToEndSeries),
      outcomeCounts: sumOutcomeCounts(run.repeats),
      deviationTotals: sumDeviationTotals(run.repeats),
    },
  };
}

function sumOutcomeCounts(repeats: MetaUiBenchmarkRunRepeat[]) {
  return repeats.reduce(
    (totals, repeat) => ({
      success: totals.success + repeat.outcomeCounts.success,
      degraded: totals.degraded + repeat.outcomeCounts.degraded,
      query_error: totals.query_error + repeat.outcomeCounts.query_error,
      crash: totals.crash + repeat.outcomeCounts.crash,
    }),
    { success: 0, degraded: 0, query_error: 0, crash: 0 },
  );
}

function sumDeviationTotals(repeats: MetaUiBenchmarkRunRepeat[]) {
  return repeats.reduce(
    (totals, repeat) => ({
      exactMatches: totals.exactMatches + repeat.deviationTotals.exactMatches,
      totalMissing: totals.totalMissing + repeat.deviationTotals.totalMissing,
      totalExtra: totals.totalExtra + repeat.deviationTotals.totalExtra,
      totalFieldMismatches:
        totals.totalFieldMismatches + repeat.deviationTotals.totalFieldMismatches,
    }),
    { exactMatches: 0, totalMissing: 0, totalExtra: 0, totalFieldMismatches: 0 },
  );
}

function formatMs(value: number): string {
  if (!Number.isFinite(value)) {
    return "N/A";
  }
  if (value >= 1000) {
    return `${(value / 1000).toFixed(2)}s`;
  }
  return `${value.toFixed(2)}ms`;
}

function formatRatio(value: number | null): string {
  if (!value || !Number.isFinite(value)) {
    return "N/A";
  }
  return `${value.toFixed(2)}x`;
}

export function collectMetaUiRuns(inputDir: string): MetaUiBenchmarkRun[] {
  return readdirSync(inputDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => join(inputDir, entry.name))
    .sort()
    .map((filePath) => JSON.parse(readFileSync(filePath, "utf8")) as MetaUiBenchmarkRun)
    .filter((run) => run.kind === "meta-ui-benchmark-run");
}

function parseReportArgs(argv: string[]) {
  const args = {
    inputDir: "",
    markdownOut: "",
    jsonOut: "",
  };

  for (const arg of argv) {
    if (arg.startsWith("--input-dir=")) {
      args.inputDir = arg.slice("--input-dir=".length);
    } else if (arg.startsWith("--markdown-out=")) {
      args.markdownOut = arg.slice("--markdown-out=".length);
    } else if (arg.startsWith("--json-out=")) {
      args.jsonOut = arg.slice("--json-out=".length);
    }
  }

  return args;
}

async function main() {
  const args = parseReportArgs(process.argv.slice(2));
  if (!args.inputDir) {
    throw new Error("Missing --input-dir=/path/to/results");
  }

  const runs = collectMetaUiRuns(args.inputDir);
  if (runs.length === 0) {
    throw new Error(`No meta-ui benchmark runs found in ${args.inputDir}`);
  }

  const report = buildMetaUiAggregateReport(runs);
  const markdown = buildMetaUiMarkdownReport(report);
  const json = JSON.stringify(report, null, 2);

  if (args.markdownOut) {
    writeFileSync(args.markdownOut, markdown);
  } else {
    process.stdout.write(`${markdown}\n`);
  }

  if (args.jsonOut) {
    writeFileSync(args.jsonOut, json);
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]).replace(/\\/g, "/") ===
    fileURLToPath(import.meta.url).replace(/\\/g, "/")
) {
  main().catch((error) => {
    console.error(error instanceof Error ? (error.stack ?? error.message) : error);
    process.exitCode = 1;
  });
}
