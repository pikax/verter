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
 *   - {@link enforceRunSummary}: fail on any reported test failure, a MISSING summary
 *     (a zero-exit host crash), or a 0-test execution. Every release E2E run is required.
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
  /**
   * The cross-process flush-lag poll window in ms (the summary is written as the runner's
   * LAST act, so it can be briefly invisible right after `runTests()` resolves). Default
   * 8000; tests pass a small/zero value to avoid the wait.
   */
  pollMs?: number;
  /** Poll interval in ms (default 200). */
  pollIntervalMs?: number;
}

/**
 * Enforce the mocha run summary as the authoritative pass/fail oracle. Throws — so the
 * caller counts a fixture failure — when the summary reports any failed test, when the
 * summary is MISSING, or when it reports a 0-test execution. The
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
  if ((summary.failures ?? 0) > 0) {
    throw new Error(
      `${label}: ${summary.failures} test(s) failed (per run summary)` +
        (summary.rootHookError ? `; root hook error: ${summary.rootHookError}` : ""),
    );
  }
  if ((summary.executed ?? 0) === 0) {
    throw new Error(`${label}: run executed 0 tests (vacuous pass refused)`);
  }
}
