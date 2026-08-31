/**
 * The E2E run-summary oracle (pure I/O logic, `vscode`-free).
 *
 * The `@vscode/test-electron` process exit code is an UNRELIABLE pass/fail signal on
 * some hosts (Windows can exit 0 even when the extension test run rejected, and the
 * editor host can crash/hang mid-suite). The authoritative oracle is the run summary the
 * mocha runner writes (`e2e/suite/index.ts` → `<logFile>.runsummary`). This module owns
 * the two decisions the runner needs:
 *   - {@link clearRunArtifacts}: delete the log and summary sidecar before a run
 *     so a STALE green summary from a prior run can never false-green a CURRENT
 *     zero-exit crash that writes no fresh summary; and
 *   - {@link enforceRunSummary}: fail on an ordinary test failure, an unmanifested
 *     pending row in an exact capability run, a MISSING summary (a zero-exit
 *     host crash), or a vacuous execution. Required parity runs may account an
 *     exact row as an explicit product gap. Legacy multi-fixture suites may
 *     report inapplicable rows as pending, but must still prove a real pass.
 *
 * Split out of `runTests.ts` (whose `main()` auto-runs) so the oracle is unit-testable
 * without launching the editor host; the poll window is injectable so the missing-summary
 * path is testable without the 8s production wait.
 */
import * as fs from "fs";

/** The parsed run-summary shape (subset the oracle keys on). */
export interface RunSummary {
  failures?: number;
  executed?: number;
  rootHookError?: string | null;
  passedTestIds?: string[];
  pendingTestIds?: string[];
  /** Detailed failure records (id + message); required for triage when failures > 0. */
  failedTests?: RunSummaryFailure[];
  /** Failed parity rows independently classified by the extension-host runner. */
  productGapTestIds?: string[];
  fixture?: string;
  typeProvider?: string;
  loadedFiles?: string[];
}

export interface RunSummaryFailure {
  id?: string;
  err?: string;
  stack?: string;
  kind?: "test" | "hook";
}

export interface ExplicitProductGap {
  readonly id: string;
  readonly issue: string;
}

/**
 * Recognize the only failure grammar a parity run may account as an expected
 * product-gap row. The marker must name the exact Mocha test title, and hooks
 * can never opt themselves into this classification.
 */
export function classifyExplicitProductGap(
  failure: RunSummaryFailure,
): ExplicitProductGap | undefined {
  if (failure.kind !== "test" || !failure.id || !failure.err) return undefined;
  const marker = /^PRODUCT_GAP (ISSUE-[A-Za-z0-9_-]+) /.exec(failure.err);
  if (!marker) return undefined;
  const issue = marker[1];
  const exactPrefix = `PRODUCT_GAP ${issue} ${failure.id}:`;
  if (failure.err !== exactPrefix && !failure.err.startsWith(`${exactPrefix} `)) return undefined;
  return { id: failure.id, issue };
}

/** The sidecar paths derived from a run's log file. */
export function runSummaryPath(logFile: string): string {
  return `${logFile}.runsummary`;
}
/**
 * Delete the log and run-summary sidecar before a run, so stale evidence
 * from a prior run can never be read after a current zero-exit crash. Best-effort: a
 * missing file is fine (`{ force: true }`).
 */
export function clearRunArtifacts(logFile: string): void {
  for (const p of [logFile, runSummaryPath(logFile)]) {
    try {
      fs.rmSync(p, { force: true });
    } catch {
      /* best-effort: the file may not exist */
    }
    // Fail closed: if the artifact SURVIVES the delete (a locked or permission-denied file,
    // common on Windows), a stale prior-run summary would later be read as a FALSE GREEN —
    // the exact hole this pre-run clear exists to close. Swallowing the `rmSync` failure is
    // not enough; refuse the run rather than let a surviving prior-run summary stand in for
    // this run's outcome.
    if (fs.existsSync(p)) {
      throw new Error(
        `clearRunArtifacts: stale artifact ${p} survived deletion before the run (locked or ` +
          `permission-denied) — a surviving prior-run summary would false-green; aborting fail-closed`,
      );
    }
  }
}

/** Options for {@link enforceRunSummary}. */
export interface EnforceRunSummaryOptions {
  /** Fixture identity expected from the extension-host process. */
  expectedFixture?: string;
  /** Provider route expected from the extension-host process. */
  expectedTypeProvider?: string;
  /**
   * The cross-process flush-lag poll window in ms (the summary is written as the runner's
   * LAST act, so it can be briefly invisible right after `runTests()` resolves). Default
   * 8000; tests pass a small/zero value to avoid the wait.
   */
  pollMs?: number;
  /** Poll interval in ms (default 200). */
  pollIntervalMs?: number;
  /**
   * Required behavioral test IDs for a release-critical run. Every required ID must
   * be accounted exactly once as a pass or an explicitly allowlisted product gap;
   * no extra ID may appear and none may be pending.
   */
  requiredTestIds?: readonly string[];
  /**
   * Exact pending-row manifest. Without this option every pending row is fatal;
   * when present, both missing and unexpected pending IDs are fatal.
   */
  allowedPendingTestIds?: readonly string[];
  /**
   * Exact route-specific product-gap manifest (`test ID` -> `ISSUE-*`). A row
   * may fail only when both its ID and issue match this manifest. Requires
   * `requiredTestIds`; ordinary failures, hooks, and newly red rows stay fatal.
   */
  allowedProductGaps?: Readonly<Record<string, string>>;
  /** Exact compiled suite-file inventory the fixture was required to load. */
  requiredLoadedFiles?: readonly string[];
}

/**
 * Enforce the mocha run summary as the authoritative pass/fail oracle. Throws — so the
 * caller counts a fixture failure — when the summary reports an unclassified failure,
 * an unmanifested pending row in an exact capability run, when the summary is
 * MISSING, or when it reports a vacuous execution. The
 * delete-before-run (`clearRunArtifacts`) guarantees any summary observed here was
 * written by THIS run, never a stale prior-run leftover.
 */
export async function enforceRunSummary(
  logFile: string,
  label: string,
  opts: EnforceRunSummaryOptions,
): Promise<void> {
  const summaryPath = runSummaryPath(logFile);
  const pollMs = opts.pollMs ?? 8_000;
  const pollIntervalMs = opts.pollIntervalMs ?? 200;
  // Poll a short window before concluding the summary is genuinely absent, so the
  // cross-process flush lag is not misread as a failure.
  const deadline = Date.now() + pollMs;
  while (!fs.existsSync(summaryPath) && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, pollIntervalMs));
  }
  if (!fs.existsSync(summaryPath)) {
    throw new Error(
      `${label}: no run summary at ${summaryPath} — the run recorded no outcome ` +
        `(vacuous pass refused; every required E2E run must write a summary)`,
    );
  }
  const summary = JSON.parse(fs.readFileSync(summaryPath, "utf-8")) as RunSummary;
  const failureCount = summary.failures ?? 0;
  const failedTests = summary.failedTests ?? [];
  const failedDetail =
    failedTests.length > 0
      ? failedTests
          .slice(0, 8)
          .map((f) => `${f.id ?? "?"}: ${f.err ?? "unknown"}`)
          .join(" || ")
      : "no failedTests[] detail recorded (runner too old?)";
  if (summary.rootHookError) {
    throw new Error(
      `${label}: root hook error: ${summary.rootHookError}; details: ${failedDetail}`,
    );
  }
  if (opts.allowedProductGaps && !opts.requiredTestIds) {
    throw new Error(
      `${label}: product-gap classification requires an exact required test manifest`,
    );
  }
  if (opts.allowedProductGaps && opts.requiredTestIds) {
    const required = new Set(opts.requiredTestIds);
    const invalid = Object.entries(opts.allowedProductGaps).filter(
      ([id, issue]) => !required.has(id) || !/^ISSUE-[A-Za-z0-9_-]+$/.test(issue),
    );
    if (invalid.length > 0) {
      throw new Error(
        `${label}: product-gap manifest contains invalid or non-required rows: ` +
          invalid.map(([id, issue]) => `${id}=${issue}`).join(", "),
      );
    }
  }

  let productGapTestIds: string[] = [];
  if (failureCount > 0 && opts.allowedProductGaps) {
    if (failedTests.length !== failureCount) {
      throw new Error(
        `${label}: product-gap classification requires one failure record per reported failure; ` +
          `reported=${failureCount} recorded=${failedTests.length}`,
      );
    }
    const classified = failedTests.map(classifyExplicitProductGap);
    if (classified.some((row) => row === undefined)) {
      throw new Error(
        `${label}: parity run contains failure(s) outside the explicit PRODUCT_GAP row grammar; ` +
          `details: ${failedDetail}`,
      );
    }
    productGapTestIds = classified.map((row) => row!.id);
    const unapproved = classified.filter(
      (row) => row && opts.allowedProductGaps?.[row.id] !== row.issue,
    );
    if (unapproved.length > 0) {
      throw new Error(
        `${label}: unapproved product-gap failure(s): ` +
          unapproved
            .map(
              (row) =>
                `${row!.id}=${row!.issue} (allowed ${opts.allowedProductGaps?.[row!.id] ?? "none"})`,
            )
            .join(", "),
      );
    }
    const declared = summary.productGapTestIds ?? [];
    const declaredCounts = countIds(declared);
    const classifiedCounts = countIds(productGapTestIds);
    const duplicate = duplicateIds(declaredCounts);
    const missing = productGapTestIds.filter((id) => (declaredCounts.get(id) ?? 0) === 0);
    const unexpected = declared.filter((id) => (classifiedCounts.get(id) ?? 0) === 0);
    if (duplicate.length > 0 || missing.length > 0 || unexpected.length > 0) {
      throw new Error(
        `${label}: product-gap summary classification mismatch` +
          `; duplicate: ${duplicate.join(", ") || "none"}` +
          `; missing: ${missing.join(", ") || "none"}` +
          `; unexpected: ${unexpected.join(", ") || "none"}`,
      );
    }
  } else if (failureCount > 0) {
    throw new Error(
      `${label}: ${failureCount} test(s) failed (per run summary); details: ${failedDetail}`,
    );
  } else if ((summary.productGapTestIds?.length ?? 0) > 0) {
    throw new Error(`${label}: run summary declares product-gap rows without failed tests`);
  }
  if ((summary.executed ?? 0) === 0) {
    throw new Error(`${label}: run executed 0 tests (vacuous pass refused)`);
  }
  const pending = summary.pendingTestIds ?? [];
  if (opts.requiredTestIds && !opts.allowedPendingTestIds && pending.length > 0) {
    throw new Error(`${label}: pending test ID(s) in required run: ${pending.join(", ")}`);
  }
  if (!opts.requiredTestIds && (summary.passedTestIds?.length ?? 0) === 0) {
    throw new Error(`${label}: run reported no passing test IDs (vacuous pass refused)`);
  }
  if (opts.allowedPendingTestIds) {
    const allowedCounts = countIds(opts.allowedPendingTestIds);
    if (duplicateIds(allowedCounts).length > 0) {
      throw new Error(`${label}: allowed-pending manifest itself contains duplicate IDs`);
    }
    const pendingCounts = countIds(pending);
    const duplicate = duplicateIds(pendingCounts);
    const missing = opts.allowedPendingTestIds.filter((id) => (pendingCounts.get(id) ?? 0) === 0);
    const unexpected = pending.filter((id) => (allowedCounts.get(id) ?? 0) === 0);
    if (duplicate.length > 0 || missing.length > 0 || unexpected.length > 0) {
      throw new Error(
        `${label}: pending manifest mismatch` +
          `; duplicate: ${duplicate.join(", ") || "none"}` +
          `; missing: ${missing.join(", ") || "none"}` +
          `; unexpected: ${unexpected.join(", ") || "none"}`,
      );
    }
  }
  if (opts.expectedFixture && summary.fixture !== opts.expectedFixture) {
    throw new Error(
      `${label}: run summary fixture mismatch; expected ${opts.expectedFixture}, got ${String(summary.fixture)}`,
    );
  }
  if (opts.expectedTypeProvider && summary.typeProvider !== opts.expectedTypeProvider) {
    throw new Error(
      `${label}: provider route mismatch; expected ${opts.expectedTypeProvider}, got ${String(summary.typeProvider)}`,
    );
  }
  if (opts.requiredLoadedFiles) {
    const required = [...opts.requiredLoadedFiles].sort();
    const loaded = [...(summary.loadedFiles ?? [])].sort();
    const requiredSet = new Set(required);
    const loadedSet = new Set(loaded);
    if (requiredSet.size !== required.length || loadedSet.size !== loaded.length) {
      throw new Error(`${label}: loaded suite inventory contains duplicate paths`);
    }
    const missing = required.filter((file) => !loadedSet.has(file));
    const unexpected = loaded.filter((file) => !requiredSet.has(file));
    if (missing.length > 0 || unexpected.length > 0) {
      throw new Error(
        `${label}: loaded suite inventory mismatch` +
          `; missing: ${missing.join(", ") || "none"}` +
          `; unexpected: ${unexpected.join(", ") || "none"}`,
      );
    }
  }
  if (opts.requiredTestIds) {
    const required = new Set(opts.requiredTestIds);
    if (required.size !== opts.requiredTestIds.length) {
      throw new Error(`${label}: required capability manifest itself contains duplicate IDs`);
    }
    const outcomes = [...(summary.passedTestIds ?? []), ...productGapTestIds];
    const counts = countIds(outcomes);
    const duplicates = duplicateIds(counts);
    const missing = opts.requiredTestIds.filter((id) => (counts.get(id) ?? 0) === 0);
    const unexpected = [...counts.keys()].filter((id) => !required.has(id));
    if (duplicates.length > 0 || missing.length > 0 || unexpected.length > 0) {
      throw new Error(
        `${label}: capability contract mismatch` +
          `; duplicate: ${duplicates.join(", ") || "none"}` +
          `; missing: ${missing.join(", ") || "none"}` +
          `; unexpected: ${unexpected.join(", ") || "none"}`,
      );
    }
  }
}

function countIds(ids: readonly string[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const id of ids) counts.set(id, (counts.get(id) ?? 0) + 1);
  return counts;
}

function duplicateIds(counts: ReadonlyMap<string, number>): string[] {
  return [...counts].filter(([, count]) => count > 1).map(([id]) => id);
}
