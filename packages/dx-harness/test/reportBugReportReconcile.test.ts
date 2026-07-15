import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  collectorEvent,
  type CollectorEventKey,
  type CollectorSignal,
  type Severity,
} from "../src/index.js";
import {
  bugReportPathForWorktree,
  buildSummary,
  reconcileBugReport,
  reconciliationSummary,
  reduceFindings,
  type DxFinding,
  type ScenarioIndex,
} from "../src/report/index.js";

const SCENARIOS: ScenarioIndex = {
  "member-access": {
    fixture: "minimal-member-access",
    probes: { p: { mappingPolicy: "strict", confidence: "high", dimension: "artifactParity" } },
  },
  "lonely-scenario": {
    fixture: "lonely-fixture",
    probes: { p: { mappingPolicy: "strict", confidence: "high", dimension: "artifactParity" } },
  },
};

function ev(
  scenario: string,
  signal: CollectorSignal,
  severity: Severity,
  data?: unknown,
  overrides: Partial<CollectorEventKey> = {},
) {
  const key: CollectorEventKey = {
    scenario,
    editStepIndex: 0,
    driver: "rawLsp",
    provider: "tsgo-rust",
    probe: "p",
    version: 1,
    anchor: "a",
    ...overrides,
  };
  return collectorEvent({
    collector: "diagnostics",
    signal,
    ok: false,
    severity,
    provenance: { detectedBy: "rawLsp" },
    key,
    detail: `${signal} at ${scenario}`,
    ...(data !== undefined ? { data } : {}),
  });
}

function buildFindings(): readonly DxFinding[] {
  return reduceFindings({
    scenarios: SCENARIOS,
    events: [
      { event: ev("member-access", "diagnostics_parity", "userVisible", { class: "verterOnly" }) },
      { event: ev("member-access", "latency_breach", "userVisible") },
      {
        event: ev("lonely-scenario", "diagnostics_parity", "userVisible", { class: "verterOnly" }),
      },
    ],
  }).findings;
}

function findBy(findings: readonly DxFinding[], scenario: string, signal: string): DxFinding {
  const found = findings.find((f) => f.scenario === scenario && f.signal === signal);
  if (found === undefined) throw new Error(`no finding for ${scenario}/${signal}`);
  return found;
}

describe("report/bugReportReconcile — absent BUG-REPORT.md", () => {
  it("emits the skipped-missing-file metadata, never throwing on a missing file", () => {
    const dir = mkdtempSync(join(tmpdir(), "dx-recon-"));
    try {
      const missing = join(dir, "lsp-bugs", "BUG-REPORT.md");
      const result = reconcileBugReport({ findings: buildFindings(), bugReportPath: missing });
      expect(result.status).toBe("skipped_missing_file");
      expect(result.bugReportPath).toBe(missing);
      // a downstream summary carries the exact documented metadata token.
      expect(reconciliationSummary(result).status).toBe("skipped_missing_file");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("derives the conventional worktree path without hardcoding an absolute one", () => {
    expect(bugReportPathForWorktree("/work/tree")).toBe(
      join("/work/tree", "lsp-bugs", "BUG-REPORT.md"),
    );
  });
});

describe("report/bugReportReconcile — present BUG-REPORT.md", () => {
  it("matches findings by fingerprint, then by heuristic, and flags the rest unmatched", () => {
    const dir = mkdtempSync(join(tmpdir(), "dx-recon-"));
    try {
      const findings = buildFindings();
      const byFingerprint = findBy(findings, "member-access", "diagnostics_parity");
      const orphanFingerprint = "f".repeat(64);
      const report = [
        "# Known LSP bugs",
        "",
        `## False red diagnostic`,
        `Tracked as fingerprint \`${byFingerprint.fingerprint}\` — verter emits a spurious error.`,
        "",
        "## Slow completions",
        "The member-access latency_breach probe regressed on large files.",
        "",
        `## Already-fixed elsewhere`,
        `Old bug fingerprint ${orphanFingerprint} no longer reproduces.`,
      ].join("\n");
      const path = join(dir, "BUG-REPORT.md");
      writeFileSync(path, report, "utf8");

      const result = reconcileBugReport({ findings, bugReportPath: path });
      expect(result.status).toBe("reconciled");

      const recon = Object.fromEntries(result.findings.map((f) => [f.fingerprint, f.status]));
      expect(recon[byFingerprint.fingerprint]).toBe("matchedByFingerprint");
      expect(recon[findBy(findings, "member-access", "latency_breach").fingerprint]).toBe(
        "matchedByHeuristic",
      );
      expect(recon[findBy(findings, "lonely-scenario", "diagnostics_parity").fingerprint]).toBe(
        "unmatched",
      );

      expect(result.matchedFindings).toBe(2);
      expect(result.unmatchedFindings).toBe(1);
      // the orphan fingerprint in the report has no live finding → surfaced, not lost.
      expect(result.knownBugsWithoutFinding).toContain(orphanFingerprint);

      // and the reconciliation flows into the run summary.
      const summary = buildSummary({
        findings,
        baselineRan: { probes: 0, probeIds: [] },
        allowlistHits: [],
        allowlistVersion: 1,
        bugReportReconciliation: reconciliationSummary(result),
      });
      expect(summary.bugReportReconciliation.status).toBe("reconciled");
      expect(summary.bugReportReconciliation.matchedFindings).toBe(2);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("does not fingerprint-match a fingerprint embedded inside a longer hex blob", () => {
    const dir = mkdtempSync(join(tmpdir(), "dx-recon-"));
    try {
      const findings = buildFindings();
      const target = findBy(findings, "lonely-scenario", "diagnostics_parity");
      // The 64-char fingerprint appears ONLY as a prefix of a 70-char hex run, on a line
      // that names neither the scenario nor the signal — so neither the strict
      // fingerprint matcher nor the co-location heuristic should fire.
      const report = [
        "# Known bugs",
        "",
        `legacy digest ${target.fingerprint}abcdef referenced once`,
      ].join("\n");
      const path = join(dir, "BUG-REPORT.md");
      writeFileSync(path, report, "utf8");

      const result = reconcileBugReport({ findings: [target], bugReportPath: path });
      const recon = result.findings.find((f) => f.fingerprint === target.fingerprint);
      // a substring-of-a-longer-blob is NOT a fingerprint match.
      expect(recon?.status).not.toBe("matchedByFingerprint");
      expect(recon?.status).toBe("unmatched");
      // the 70-char blob is not mistaken for (or split into) a known fingerprint.
      expect(result.knownBugFingerprints).not.toContain(target.fingerprint);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
