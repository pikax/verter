/**
 * Reconciliation against a hand-maintained `lsp-bugs/BUG-REPORT.md`.
 *
 * `DX-FINDINGS.md` carries each finding's stable fingerprint; this module closes the
 * loop by matching the live findings against an existing bug report so the harness
 * does not silently re-file a bug a human already tracked. The bug report path (or the
 * worktree root it lives under) is a PARAMETER — never a hardcoded absolute path — so
 * the reconciliation runs identically across worktrees and CI checkouts. When the file
 * is absent the run is not a failure: it emits the documented
 * `bug_report_reconciliation: skipped_missing_file` metadata instead.
 */

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { normalizeEol } from "../normalize/index.js";
import type { BugReportReconciliationSummary, DxFinding } from "./findings.js";

/** The conventional location of the bug report under a worktree root. */
export function bugReportPathForWorktree(worktreeRoot: string): string {
  return join(worktreeRoot, "lsp-bugs", "BUG-REPORT.md");
}

/** Whether (and how) a finding was reconciled against the bug report. */
export type FindingReconciliationStatus =
  | "matchedByFingerprint"
  | "matchedByHeuristic"
  | "unmatched";

/** One finding's reconciliation verdict. */
export interface FindingReconciliation {
  readonly fingerprint: string;
  readonly scenario: string;
  readonly signal: string;
  readonly status: FindingReconciliationStatus;
  /** The matched bug-report line, for a heuristic match. */
  readonly bugRef?: string;
}

/** The reconciliation status of the whole run. */
export type BugReportReconciliationStatus = "reconciled" | "skipped_missing_file";

/** The result of reconciling the run's findings against `BUG-REPORT.md`. */
export interface BugReportReconciliation {
  readonly status: BugReportReconciliationStatus;
  readonly bugReportPath: string;
  readonly findings: readonly FindingReconciliation[];
  readonly matchedFindings: number;
  readonly unmatchedFindings: number;
  /** Every fingerprint the bug report cites. */
  readonly knownBugFingerprints: readonly string[];
  /** Cited fingerprints with no corresponding live finding (a bug that may be fixed). */
  readonly knownBugsWithoutFinding: readonly string[];
}

/** Inputs to {@link reconcileBugReport}. */
export interface ReconcileBugReportInput {
  readonly findings: readonly DxFinding[];
  /** The explicit path to the bug report (see {@link bugReportPathForWorktree}). */
  readonly bugReportPath: string;
}

/** Extract every distinct sha256-shaped fingerprint the bug report cites. */
function extractFingerprints(text: string): string[] {
  const seen = new Set<string>();
  // Maximal hex runs, kept only when exactly 64 chars — so a stray longer hex blob is
  // not mistaken for (or split into) a fingerprint.
  for (const run of text.match(/[0-9a-f]{64,}/g) ?? []) {
    if (run.length === 64) seen.add(run);
  }
  return [...seen].sort();
}

/** Reconcile one finding: strict-fingerprint match first, then a co-located scenario+signal heuristic. */
function reconcileFinding(
  finding: DxFinding,
  knownFingerprints: ReadonlySet<string>,
  lines: readonly string[],
): FindingReconciliation {
  const identity = {
    fingerprint: finding.fingerprint,
    scenario: finding.scenario,
    signal: finding.signal,
  };
  // Membership in the STRICT 64-char extraction — never a substring scan, so a
  // fingerprint embedded in a longer hex blob is not a false match.
  if (knownFingerprints.has(finding.fingerprint)) {
    return { ...identity, status: "matchedByFingerprint" };
  }
  const line = lines.find(
    (candidate) => candidate.includes(finding.scenario) && candidate.includes(finding.signal),
  );
  if (line !== undefined) {
    return { ...identity, status: "matchedByHeuristic", bugRef: line.trim() };
  }
  return { ...identity, status: "unmatched" };
}

/**
 * Reconcile the run's findings against `BUG-REPORT.md`. A present file yields a
 * per-finding verdict (fingerprint match, then heuristic, then unmatched) plus the
 * cited fingerprints with no live finding; an absent file yields the
 * `skipped_missing_file` status and never throws.
 */
export function reconcileBugReport(input: ReconcileBugReportInput): BugReportReconciliation {
  const { findings, bugReportPath } = input;
  if (!existsSync(bugReportPath)) {
    return {
      status: "skipped_missing_file",
      bugReportPath,
      findings: [],
      matchedFindings: 0,
      unmatchedFindings: 0,
      knownBugFingerprints: [],
      knownBugsWithoutFinding: [],
    };
  }
  const text = normalizeEol(readFileSync(bugReportPath, "utf8"));
  const lines = text.split("\n");
  const knownBugFingerprints = extractFingerprints(text);
  const knownFingerprintSet = new Set(knownBugFingerprints);
  const liveFingerprints = new Set(findings.map((finding) => finding.fingerprint));
  const reconciled = findings.map((finding) =>
    reconcileFinding(finding, knownFingerprintSet, lines),
  );
  const matchedFindings = reconciled.filter((entry) => entry.status !== "unmatched").length;
  return {
    status: "reconciled",
    bugReportPath,
    findings: reconciled,
    matchedFindings,
    unmatchedFindings: reconciled.length - matchedFindings,
    knownBugFingerprints,
    knownBugsWithoutFinding: knownBugFingerprints.filter((fp) => !liveFingerprints.has(fp)),
  };
}

/** Project a reconciliation result into the {@link BugReportReconciliationSummary} the summary embeds. */
export function reconciliationSummary(
  result: BugReportReconciliation,
): BugReportReconciliationSummary {
  if (result.status === "skipped_missing_file") {
    return { status: "skipped_missing_file", bugReportPath: result.bugReportPath };
  }
  return {
    status: "reconciled",
    bugReportPath: result.bugReportPath,
    matchedFindings: result.matchedFindings,
    unmatchedFindings: result.unmatchedFindings,
    knownBugsWithoutFinding: result.knownBugsWithoutFinding.length,
  };
}
