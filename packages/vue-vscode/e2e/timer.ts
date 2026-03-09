import * as fs from "fs";
import { TIMING_FILE, FIXTURE_NAME } from "./helpers";

export interface HoverSample {
  target: string;
  latencyMs: number;
}

export interface StartupTimingRecord {
  activationStartMs: number;
  typeProviderStartedMs: number | null;
  lspReadyMs: number;
  firstTypedCompletionMs: number | null;
  firstDiagnosticMs: number | null;
  providerKind: string;
}

export interface TimingReport {
  fixture: string;
  timestamp: string;
  startup: {
    activationStartMs: number | null;
    typeProviderStartedMs: number | null;
    lspReadyMs: number | null;
    firstTypedCompletionMs: number | null;
    firstDiagnosticMs: number | null;
    activationToReadyMs: number | null;
    activationToFirstTypedCompletionMs: number | null;
    readyToFirstTypedCompletionMs: number | null;
    typeProvider: string | null;
    providerKind: string | null;
  };
  hover: {
    samples: HoverSample[];
    avgMs: number | null;
    p95Ms: number | null;
    maxMs: number | null;
  };
  diagnostics: {
    timeToFirstDiagnosticMs: number | null;
    totalDiagnostics: number;
    sources: string[];
  };
}

/**
 * Collects timing data throughout the E2E test run and writes a JSON report.
 */
export class TestTimer {
  private report: TimingReport;

  constructor() {
    this.report = {
      fixture: FIXTURE_NAME,
      timestamp: new Date().toISOString(),
      startup: {
        activationStartMs: null,
        typeProviderStartedMs: null,
        lspReadyMs: null,
        firstTypedCompletionMs: null,
        firstDiagnosticMs: null,
        activationToReadyMs: null,
        activationToFirstTypedCompletionMs: null,
        readyToFirstTypedCompletionMs: null,
        typeProvider: null,
        providerKind: null,
      },
      hover: {
        samples: [],
        avgMs: null,
        p95Ms: null,
        maxMs: null,
      },
      diagnostics: {
        timeToFirstDiagnosticMs: null,
        totalDiagnostics: 0,
        sources: [],
      },
    };
  }

  recordStartup(activationToReadyMs: number): void {
    this.report.startup.activationToReadyMs = activationToReadyMs;
  }

  recordStartupTiming(record: StartupTimingRecord): void {
    this.report.startup.activationStartMs = record.activationStartMs;
    this.report.startup.typeProviderStartedMs = record.typeProviderStartedMs;
    this.report.startup.lspReadyMs = record.lspReadyMs;
    this.report.startup.firstTypedCompletionMs = record.firstTypedCompletionMs;
    this.report.startup.firstDiagnosticMs = record.firstDiagnosticMs;
    this.report.startup.typeProvider = record.providerKind;
    this.report.startup.providerKind = record.providerKind;

    this.report.startup.activationToReadyMs =
      record.lspReadyMs - record.activationStartMs;
    this.report.startup.activationToFirstTypedCompletionMs =
      record.firstTypedCompletionMs === null
        ? null
        : record.firstTypedCompletionMs - record.activationStartMs;
    this.report.startup.readyToFirstTypedCompletionMs =
      record.firstTypedCompletionMs === null
        ? null
        : record.firstTypedCompletionMs - record.lspReadyMs;

    if (record.firstDiagnosticMs !== null) {
      this.report.diagnostics.timeToFirstDiagnosticMs =
        record.firstDiagnosticMs - record.activationStartMs;
    }
  }

  recordTypeProvider(provider: string, startMs?: number): void {
    this.report.startup.typeProvider = provider;
    this.report.startup.providerKind = provider;
    if (startMs !== undefined) {
      this.report.startup.typeProviderStartedMs = startMs;
    }
  }

  recordHover(target: string, latencyMs: number): void {
    this.report.hover.samples.push({ target, latencyMs });
  }

  recordDiagnostics(
    timeToFirstMs: number,
    total: number,
    sources: string[],
  ): void {
    this.report.diagnostics.timeToFirstDiagnosticMs = timeToFirstMs;
    this.report.diagnostics.totalDiagnostics = total;
    this.report.diagnostics.sources = sources;
  }

  /**
   * Compute hover statistics and write the timing report to disk.
   */
  flush(): void {
    const samples = this.report.hover.samples;
    if (samples.length > 0) {
      const latencies = samples.map((s) => s.latencyMs).sort((a, b) => a - b);
      this.report.hover.avgMs = Math.round(
        latencies.reduce((a, b) => a + b, 0) / latencies.length,
      );
      this.report.hover.maxMs = latencies[latencies.length - 1];
      const p95Index = Math.min(
        Math.ceil(latencies.length * 0.95) - 1,
        latencies.length - 1,
      );
      this.report.hover.p95Ms = latencies[p95Index];
    }

    try {
      fs.writeFileSync(TIMING_FILE, JSON.stringify(this.report, null, 2));
    } catch {
      // Silently ignore write failures in CI
    }
  }

  getReport(): TimingReport {
    return this.report;
  }
}

/** Singleton timer instance shared across test suites. */
let _timer: TestTimer | undefined;

export function getTimer(): TestTimer {
  if (!_timer) {
    _timer = new TestTimer();
  }
  return _timer;
}
