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
 *   - {@link enforceRunSummary}: fail on any test failure, an unmanifested pending
 *     row in an exact capability run, a MISSING summary (a zero-exit host crash),
 *     or a vacuous execution. Required parity runs may account an exact pending
 *     row as a statically declared product-gap skip. Legacy multi-fixture suites may
 *     report inapplicable rows as pending, but must still prove a real pass.
 *
 * Product-gap CANARIES are the executed counterpart of a skipped gap: a canary runs
 * and is expected to fail. Its failure is tolerated (and never counted as a pass); its
 * PASS fails the run, so a repaired defect cannot stay hidden behind a manifest entry;
 * and a canary that never executed in an unfiltered run is a stale entry and fails too.
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
  /** Route-approved product gaps skipped before their test bodies executed. */
  skippedProductGaps?: ProductGapSkip[];
  /** Route-approved canaries that executed and failed, as the runner classified them. */
  failingProductGapCanaries?: ProductGapSkip[];
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

export interface ProductGapSkip {
  readonly id: string;
  readonly issue: string;
}

/**
 * One canary: the tracked defect, and the failure that defect produces in this test.
 * Only that failure is tolerated — any other failure of the same test is a regression.
 */
export interface ProductGapCanary {
  readonly issue: string;
  readonly failure: RegExp;
}

/** `test ID` -> the canary declared for it. */
export type ProductGapCanaryManifest = Readonly<Record<string, ProductGapCanary>>;

/** Whether `failure` is the test-body failure `canary` is declared to expect. */
export function isExpectedCanaryFailure(
  canary: ProductGapCanary | undefined,
  failure: RunSummaryFailure,
): boolean {
  return canary !== undefined && failure.kind === "test" && canary.failure.test(failure.err ?? "");
}

/** Where each canary of a route manifest landed in one run. */
export interface ProductGapCanaryOutcome {
  /** Canaries whose test body ran and failed: tolerated, reported as degraded coverage. */
  readonly stillFailing: readonly ProductGapSkip[];
  /** Canaries whose test body ran and passed: the defect is fixed and the entry is stale. */
  readonly unexpectedlyPassing: readonly ProductGapSkip[];
  /** Every recorded failure that is not a canary's own expected test-body failure. */
  readonly blockingFailures: readonly RunSummaryFailure[];
}

/** The manifest file a canary message points the reader at. */
export const PRODUCT_GAP_CANARY_MANIFEST = "e2e/lib/knownProductGapManifest.ts";

/**
 * Classify a run against a canary manifest. Only a canary's own TEST-body failure that
 * matches its declared failure is tolerated; a hook failure attributed to the same
 * title, and any other failure of the canary's test, stays blocking.
 */
export function classifyProductGapCanaries(
  summary: Pick<RunSummary, "passedTestIds" | "failedTests">,
  canaries: ProductGapCanaryManifest,
): ProductGapCanaryOutcome {
  const isCanaryFailure = (failure: RunSummaryFailure): boolean =>
    failure.id !== undefined && isExpectedCanaryFailure(canaries[failure.id], failure);
  const failedTests = summary.failedTests ?? [];
  const failedIds = new Set(failedTests.filter(isCanaryFailure).map((failure) => failure.id));
  const passedIds = new Set(summary.passedTestIds ?? []);
  const rows = Object.entries(canaries).map(([id, { issue }]) => ({ id, issue }));
  return {
    stillFailing: rows.filter((row) => failedIds.has(row.id)),
    unexpectedlyPassing: rows.filter((row) => passedIds.has(row.id)),
    blockingFailures: failedTests.filter((failure) => !isCanaryFailure(failure)),
  };
}

/** The operator-facing verdict for a canary that passed. */
export function unexpectedCanaryPassMessage(rows: readonly ProductGapSkip[]): string {
  return (
    `canary test(s) PASSED: ${rows.map((row) => `${row.id} (${row.issue})`).join(", ")} — ` +
    `the defect is fixed; remove the canary entry from ${PRODUCT_GAP_CANARY_MANIFEST} so the ` +
    "case is a normal required test again"
  );
}

/** The sidecar paths derived from a run's log file. */
export function runSummaryPath(logFile: string): string {
  return `${logFile}.runsummary`;
}

/** Parse the run summary a completed run wrote beside its log. */
export function readRunSummary(logFile: string): RunSummary {
  return JSON.parse(fs.readFileSync(runSummaryPath(logFile), "utf-8")) as RunSummary;
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
   * be accounted exactly once as a pass or an explicitly allowlisted product-gap
   * skip; no extra ID may appear.
   */
  requiredTestIds?: readonly string[];
  /**
   * Exact pending-row manifest. Without this option every pending row is fatal;
   * when present, both missing and unexpected pending IDs are fatal.
   */
  allowedPendingTestIds?: readonly string[];
  /**
   * Exact route-specific product-gap manifest (`test ID` -> `ISSUE-*`). Every
   * listed row must be pending and recorded as skipped before its body executes.
   * Requires `requiredTestIds`; all failures, hooks, and newly red rows stay fatal.
   */
  allowedProductGaps?: Readonly<Record<string, string>>;
  /**
   * The route's COMPLETE canary manifest (`test ID` -> `ISSUE-*`): tests that execute
   * and are expected to fail. A failing canary is tolerated, a passing one fails the
   * run, and one that never executed is a stale entry. Requires `requiredTestIds`.
   */
  allowedProductGapCanaries?: ProductGapCanaryManifest;
  /**
   * The run selected a subset of the route inventory. A canary outside
   * `requiredTestIds` is then not evaluated; in an unfiltered run it is stale.
   */
  selectionFiltered?: boolean;
  /** Exact compiled suite-file inventory the fixture was required to load. */
  requiredLoadedFiles?: readonly string[];
}

/**
 * Enforce the mocha run summary as the authoritative pass/fail oracle. Throws — so the
 * caller counts a fixture failure — when the summary reports any failure,
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
  const summary = readRunSummary(logFile);
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

  const declaredCanaries = summary.failingProductGapCanaries ?? [];
  if (!opts.allowedProductGapCanaries && declaredCanaries.length > 0) {
    throw new Error(`${label}: run summary declares canary failures without a route manifest`);
  }
  if (opts.allowedProductGapCanaries && !opts.requiredTestIds) {
    throw new Error(`${label}: canary classification requires an exact required test manifest`);
  }
  const allCanaries = opts.allowedProductGapCanaries ?? {};
  const invalidCanaries = Object.entries(allCanaries).filter(
    ([, canary]) => !/^ISSUE-[A-Za-z0-9_-]+$/.test(canary.issue),
  );
  if (invalidCanaries.length > 0) {
    throw new Error(
      `${label}: canary manifest contains invalid rows: ` +
        invalidCanaries.map(([id, canary]) => `${id}=${canary.issue}`).join(", "),
    );
  }
  const gapAndCanary = Object.keys(allCanaries).filter(
    (id) => opts.allowedProductGaps?.[id] !== undefined,
  );
  if (gapAndCanary.length > 0) {
    throw new Error(
      `${label}: test(s) declared as both a skipped product gap and a canary: ` +
        gapAndCanary.join(", "),
    );
  }
  // A filtered selection evaluates only the canaries it selected; an unfiltered run
  // evaluates the whole route manifest, so an entry naming no executed test is caught.
  const selected = new Set(opts.requiredTestIds ?? []);
  const evaluatedCanaries = Object.fromEntries(
    Object.entries(allCanaries).filter(([id]) => !opts.selectionFiltered || selected.has(id)),
  );
  const canaryOutcome = classifyProductGapCanaries(summary, evaluatedCanaries);
  const toleratedCanaryIds = canaryOutcome.stillFailing.map((row) => row.id);

  // Only a canary's own test-body failure is subtracted; every other failure — and any
  // failure the runner counted without recording — stays fatal.
  const blockingFailureCount = failureCount - toleratedCanaryIds.length;
  if (blockingFailureCount > 0) {
    const blockingDetail =
      toleratedCanaryIds.length === 0
        ? failedDetail
        : canaryOutcome.blockingFailures
            .slice(0, 8)
            .map((f) => `${f.id ?? "?"}: ${f.err ?? "unknown"}`)
            .join(" || ") || "no detail recorded for the non-canary failure(s)";
    throw new Error(
      `${label}: ${blockingFailureCount} test(s) failed (per run summary); details: ${blockingDetail}`,
    );
  }
  if (canaryOutcome.unexpectedlyPassing.length > 0) {
    throw new Error(`${label}: ${unexpectedCanaryPassMessage(canaryOutcome.unexpectedlyPassing)}`);
  }
  const executedCanaryIds = new Set(toleratedCanaryIds);
  const staleCanaries = Object.entries(evaluatedCanaries).filter(
    ([id]) => !executedCanaryIds.has(id),
  );
  if (staleCanaries.length > 0) {
    throw new Error(
      `${label}: stale canary entr${staleCanaries.length === 1 ? "y" : "ies"} in ` +
        `${PRODUCT_GAP_CANARY_MANIFEST}: ` +
        staleCanaries.map(([id, canary]) => `${id} (${canary.issue})`).join(", ") +
        " never executed in this run; a canary must name a registered test that runs on its route",
    );
  }
  const declaredCanaryIds = declaredCanaries.map((row) => row.id).sort();
  if (
    declaredCanaryIds.join("\n") !== [...toleratedCanaryIds].sort().join("\n") ||
    declaredCanaries.some((row) => evaluatedCanaries[row.id]?.issue !== row.issue)
  ) {
    throw new Error(
      `${label}: canary failure manifest mismatch; runner declared: ` +
        `${declaredCanaries.map((row) => `${row.id}=${row.issue}`).join(", ") || "none"}` +
        `; recorded failures: ${toleratedCanaryIds.join(", ") || "none"}`,
    );
  }

  const skippedProductGaps = summary.skippedProductGaps ?? [];
  const skippedProductGapIds = skippedProductGaps.map((row) => row.id);
  if (!opts.allowedProductGaps && skippedProductGaps.length > 0) {
    throw new Error(`${label}: run summary declares product-gap skips without a route manifest`);
  }
  if (opts.allowedProductGaps) {
    const expected = opts.allowedProductGaps;
    const expectedIds = Object.keys(expected);
    const skippedCounts = countIds(skippedProductGapIds);
    const duplicate = duplicateIds(skippedCounts);
    const missing = expectedIds.filter((id) => (skippedCounts.get(id) ?? 0) === 0);
    const unexpected = skippedProductGapIds.filter((id) => expected[id] === undefined);
    const issueMismatch = skippedProductGaps.filter(
      (row) => expected[row.id] !== undefined && expected[row.id] !== row.issue,
    );
    if (
      duplicate.length > 0 ||
      missing.length > 0 ||
      unexpected.length > 0 ||
      issueMismatch.length > 0
    ) {
      throw new Error(
        `${label}: product-gap skip manifest mismatch` +
          `; duplicate: ${duplicate.join(", ") || "none"}` +
          `; missing: ${missing.join(", ") || "none"}` +
          `; unexpected: ${unexpected.join(", ") || "none"}` +
          `; issue mismatch: ${
            issueMismatch
              .map((row) => `${row.id}=${row.issue} (expected ${expected[row.id]})`)
              .join(", ") || "none"
          }`,
      );
    }
  }
  if ((summary.executed ?? 0) === 0) {
    throw new Error(`${label}: run executed 0 tests (vacuous pass refused)`);
  }
  const pending = summary.pendingTestIds ?? [];
  if (opts.requiredTestIds) {
    const allowedPending = [
      ...Object.keys(opts.allowedProductGaps ?? {}),
      ...(opts.allowedPendingTestIds ?? []),
    ];
    const allowedCounts = countIds(allowedPending);
    const pendingCounts = countIds(pending);
    const duplicate = duplicateIds(pendingCounts);
    const missing = allowedPending.filter((id) => (pendingCounts.get(id) ?? 0) === 0);
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
  if (!opts.requiredTestIds && (summary.passedTestIds?.length ?? 0) === 0) {
    throw new Error(`${label}: run reported no passing test IDs (vacuous pass refused)`);
  }
  if (opts.allowedPendingTestIds && !opts.requiredTestIds) {
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
    // A tolerated canary is accounted for, but only here: it never joins the passes.
    const outcomes = [
      ...(summary.passedTestIds ?? []),
      ...skippedProductGapIds,
      ...toleratedCanaryIds,
    ];
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
